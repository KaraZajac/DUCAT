use std::{
    cmp::min,
    collections::HashMap,
    convert::TryInto,
    fmt, io,
    path::{Path, PathBuf},
};

use flume::{unbounded, Receiver, Sender};
// DUCAT modification: pieces and payloads hash with BLAKE3, the hash
// the rest of this stack already speaks (see ../STIGMERGE-NOTICE.md).
use blake3::Hasher as Blake3;
use tokio::{
    fs::{File, OpenOptions},
    io::{AsyncReadExt, AsyncSeekExt},
    select, spawn,
    sync::watch,
    task::JoinSet,
};

pub use anyhow::{Error, Result};

/// Block size in bytes. Blocks are the smallest unit of file transfer in stigmerge.
pub const BLOCK_SIZE_BYTES: usize = 32768;

/// Piece size in blocks. A piece is a verified unit of transferred content in stigmerge.
pub const PIECE_SIZE_BLOCKS: usize = 32; // 32 * 32KB blocks = 1MB

/// One piece is 32 blocks long, or 1MiB.
pub const PIECE_SIZE_BYTES: usize = PIECE_SIZE_BLOCKS * BLOCK_SIZE_BYTES;

/// How many pieces a file of `len` bytes occupies under the piece-aligned
/// layout (zero-length files occupy none).
pub fn piece_count_for_len(len: usize) -> usize {
    len / PIECE_SIZE_BYTES + if len % PIECE_SIZE_BYTES > 0 { 1 } else { 0 }
}

// DUCAT modification (see ../../STIGMERGE-NOTICE.md): what an index off the
// wire is allowed to declare.
//
// An index is written by the publisher and read by everyone who fetches the
// share. Upstream believed every number in it: the piece and slice lengths
// were taken as read, and `Indexer::from_wanted` then created and sized
// every file the index named before a single byte was verified. A share
// whose index said "one file, 900 GB" cost the publisher one DHT write and
// the reader its whole disk; one that pointed a file's slice past the end
// of the pieces list panicked the verifier instead.
//
// These are absolute ceilings — not the per-fetch budget, which the
// embedding application sets per kind of thing (see `Mode::Fetch::max_bytes`
// in stigmerge-peer). They only say what cannot be a real DUCAT share at
// all, so that the shape can be refused before anything is created.

/// The largest payload any index may declare, whatever the caller's budget.
///
/// u64 rather than usize: 16 GiB does not fit a 32-bit usize, and the check
/// must not itself overflow on a 32-bit target.
pub const MAX_PAYLOAD_BYTES: u64 = 16 * 1024 * 1024 * 1024;

/// The most files one share may name.
pub const MAX_FILES: usize = 65_536;

/// The most pieces one index may carry: a 16 GiB payload is at most 16 384
/// whole pieces, plus at most one short tail piece per file.
pub const MAX_PIECES: usize = (MAX_PAYLOAD_BYTES / PIECE_SIZE_BYTES as u64) as usize + MAX_FILES;

/// Why an index off the wire was refused before anything was created for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShapeError {
    /// More files than any real share carries.
    TooManyFiles(usize),
    /// More pieces than a legal payload can be cut into.
    TooManyPieces(usize),
    /// A piece longer than a piece, or empty. Every piece of a real index is
    /// between one byte and `PIECE_SIZE_BYTES`.
    PieceLength { piece_index: usize, length: usize },
    /// The pieces add up to more than any share may be.
    PayloadTooLarge(u64),
    /// A file's slice starts inside a piece. DUCAT shares are piece-aligned
    /// (see the notice): every file starts on a fresh piece, for ever, so a
    /// non-zero offset is either a foreign index or an attempt to push a
    /// write past the end of a file.
    UnalignedSlice { file_index: usize },
    /// A file's pieces are not all in the pieces list.
    SliceOutsidePieces {
        file_index: usize,
        starting_piece: usize,
        piece_count: usize,
        have_pieces: usize,
    },
    /// A file's own pieces do not add up to its declared length.
    SliceLengthMismatch {
        file_index: usize,
        declared: usize,
        pieces: u64,
    },
    /// Two files claim the same piece, or no file claims it. Either way the
    /// index does not describe a payload anybody could have indexed.
    PieceNotClaimedOnce { piece_index: usize, claims: usize },
    /// The files and the pieces disagree about how long the payload is, or
    /// the header does.
    LengthMismatch { declared: u64, pieces: u64, files: u64 },
}

impl fmt::Display for ShapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShapeError::TooManyFiles(n) => {
                write!(f, "the index names {n} files, more than {MAX_FILES}")
            }
            ShapeError::TooManyPieces(n) => {
                write!(f, "the index carries {n} pieces, more than {MAX_PIECES}")
            }
            ShapeError::PieceLength {
                piece_index,
                length,
            } => write!(
                f,
                "piece {piece_index} declares {length} bytes; a piece is 1..={PIECE_SIZE_BYTES}"
            ),
            ShapeError::PayloadTooLarge(n) => {
                write!(f, "the index declares {n} bytes, more than {MAX_PAYLOAD_BYTES}")
            }
            ShapeError::UnalignedSlice { file_index } => write!(
                f,
                "file {file_index} starts inside a piece; shares are piece-aligned"
            ),
            ShapeError::SliceOutsidePieces {
                file_index,
                starting_piece,
                piece_count,
                have_pieces,
            } => write!(
                f,
                "file {file_index} wants pieces {starting_piece}..{} of {have_pieces}",
                starting_piece + piece_count
            ),
            ShapeError::SliceLengthMismatch {
                file_index,
                declared,
                pieces,
            } => write!(
                f,
                "file {file_index} declares {declared} bytes; its pieces hold {pieces}"
            ),
            ShapeError::PieceNotClaimedOnce {
                piece_index,
                claims,
            } => write!(f, "piece {piece_index} is claimed by {claims} files"),
            ShapeError::LengthMismatch {
                declared,
                pieces,
                files,
            } => write!(
                f,
                "the index declares {declared} bytes, its pieces hold {pieces} and its files {files}"
            ),
        }
    }
}

impl std::error::Error for ShapeError {}

/// Refuse an index whose declared shape could not describe real content.
///
/// DUCAT modification (see ../../STIGMERGE-NOTICE.md). Called at decode,
/// where the payload length is not yet known (it lives in the share header),
/// and again once header and index have met — `declared_length` is `Some`
/// on the second pass.
///
/// Everything checked here is true of every index this indexer produces, so
/// an honest share is unaffected; each rule is one way an index off the wire
/// could otherwise make a fetcher create, size or seek something the
/// publisher chose.
pub fn check_index_shape(
    pieces: &[PayloadPiece],
    files: &[FileSpec],
    declared_length: Option<usize>,
) -> std::result::Result<(), ShapeError> {
    if files.len() > MAX_FILES {
        return Err(ShapeError::TooManyFiles(files.len()));
    }
    if pieces.len() > MAX_PIECES {
        return Err(ShapeError::TooManyPieces(pieces.len()));
    }
    let mut pieces_total: u64 = 0;
    for (piece_index, piece) in pieces.iter().enumerate() {
        if piece.length == 0 || piece.length > PIECE_SIZE_BYTES {
            return Err(ShapeError::PieceLength {
                piece_index,
                length: piece.length,
            });
        }
        pieces_total += piece.length as u64;
        if pieces_total > MAX_PAYLOAD_BYTES {
            return Err(ShapeError::PayloadTooLarge(pieces_total));
        }
    }

    // Every piece claimed by exactly one file. Without this an index can
    // balance its arithmetic by handing one piece to two files and leaving
    // another to nobody — the sums below would still agree.
    let mut claims: Vec<usize> = vec![0; pieces.len()];
    let mut files_total: u64 = 0;
    for (file_index, file) in files.iter().enumerate() {
        if file.contents.piece_offset != 0 {
            return Err(ShapeError::UnalignedSlice { file_index });
        }
        let piece_count = piece_count_for_len(file.contents.length);
        let starting_piece = file.contents.starting_piece;
        let end = match starting_piece.checked_add(piece_count) {
            Some(end) if end <= pieces.len() => end,
            _ => {
                return Err(ShapeError::SliceOutsidePieces {
                    file_index,
                    starting_piece,
                    piece_count,
                    have_pieces: pieces.len(),
                })
            }
        };
        let mut slice_total: u64 = 0;
        for piece_index in starting_piece..end {
            claims[piece_index] += 1;
            slice_total += pieces[piece_index].length as u64;
        }
        if slice_total != file.contents.length as u64 {
            return Err(ShapeError::SliceLengthMismatch {
                file_index,
                declared: file.contents.length,
                pieces: slice_total,
            });
        }
        files_total += file.contents.length as u64;
    }
    for (piece_index, n) in claims.into_iter().enumerate() {
        if n != 1 {
            return Err(ShapeError::PieceNotClaimedOnce { piece_index, claims: n });
        }
    }
    if files_total != pieces_total {
        return Err(ShapeError::LengthMismatch {
            declared: declared_length.map(|d| d as u64).unwrap_or(pieces_total),
            pieces: pieces_total,
            files: files_total,
        });
    }
    if let Some(declared) = declared_length {
        if declared as u64 != pieces_total {
            return Err(ShapeError::LengthMismatch {
                declared: declared as u64,
                pieces: pieces_total,
                files: files_total,
            });
        }
    }
    Ok(())
}

/// Internal buffer size used when concurrently indexing a file.
const INDEX_BUFFER_SIZE: usize = 67108864; // 64MB

/// An Index represents a map of filesystem contents in a given layout, with
/// content digests on each piece of each file, as well as a digest of the
/// overall content.
#[derive(Debug, PartialEq, Clone)]
pub struct Index {
    root: PathBuf,
    payload: PayloadSpec,
    files: Vec<FileSpec>,
}

impl Index {
    /// Create a new Index for the given root path, content and file layout.
    pub fn new(root: PathBuf, payload: PayloadSpec, files: Vec<FileSpec>) -> Index {
        Index {
            root,
            payload,
            files,
        }
    }

    /// Get the root path that the index represents.
    pub fn root(&self) -> &Path {
        self.root.as_ref()
    }

    /// Get the content layout that the index represents.
    pub fn payload(&self) -> &PayloadSpec {
        &self.payload
    }

    /// Get the file layout that the index represents.
    pub fn files(&self) -> &Vec<FileSpec> {
        &self.files
    }

    /// How many bytes this index says the payload is.
    ///
    /// DUCAT modification (see ../../STIGMERGE-NOTICE.md): the number a
    /// caller's byte ceiling is compared against, before anything is
    /// created on disk for the share. It is the publisher's claim — worth
    /// nothing on its own, which is why [`Index::check_shape`] must pass
    /// first: after that the header's length, the pieces and the files all
    /// agree, so refusing on this number refuses the whole fetch.
    pub fn declared_length(&self) -> u64 {
        self.payload.length as u64
    }

    /// Refuse an index off the wire whose declared shape could not describe
    /// real content (DUCAT modification; see [`check_index_shape`]).
    pub fn check_shape(&self) -> std::result::Result<(), ShapeError> {
        check_index_shape(&self.payload.pieces, &self.files, Some(self.payload.length))
    }

    /// Create a new empty index with the same root path.
    pub fn empty(&self) -> Index {
        Index::empty_root(&self.root)
    }

    pub fn file_index_for_piece(&self, piece_index: usize) -> usize {
        let mut file_index = 0;
        for (i, file) in self.files().iter().enumerate() {
            if file.contents.starting_piece > piece_index {
                break;
            }
            file_index = i;
        }
        file_index
    }

    /// Create a new empty index at a given root path.
    pub fn empty_root(root: &Path) -> Index {
        Index {
            root: root.into(),
            payload: PayloadSpec {
                digest: [0u8; 32],
                length: 0,
                pieces: vec![],
            },
            files: vec![],
        }
    }

    /// Canonicalize the index, by sorting items into a deterministic and
    /// reproducible order.
    pub fn canonicalize(&mut self) {
        self.files.sort_by(|a, b| a.path.cmp(&b.path));
    }

    /// Reconcile an index calculated over actual filesystem state with desired
    /// filesystem state, returning the file blocks that are missing or
    /// incomplete.
    pub fn diff(&self, have: &Index) -> IndexDiff {
        let mut want_file_blocks = vec![];
        let mut have_file_blocks = vec![];
        let have_files_map: HashMap<&Path, &FileSpec> =
            have.files.iter().map(|f| (f.path(), f)).collect();

        for (want_file_index, want_file) in self.files.iter().enumerate() {
            if have_files_map.contains_key(want_file.path()) {
                // Compare pieces considering the PayloadSlice
                let want_pieces = self.payload.pieces();
                let have_pieces = have.payload.pieces();
                let want_slice = want_file.contents();

                for piece_index in want_slice.starting_piece
                    ..(want_slice.starting_piece
                        + (want_slice.length / PIECE_SIZE_BYTES)
                        + if want_slice.length % PIECE_SIZE_BYTES > 0 {
                            1
                        } else {
                            0
                        })
                {
                    // TODO: Deal with payload slices that have a non-zero piece
                    // offset. This becomes a concern with multi-file indexes,
                    // where payload slices might not be aligned to piece
                    // boundaries.
                    if let Some(want_piece) = want_pieces.get(piece_index) {
                        if let Some(have_piece) = have_pieces.get(piece_index) {
                            if want_piece.digest() != have_piece.digest() {
                                let mut piece_position = 0;
                                for block_index in 0..want_piece.block_count() {
                                    let block_length =
                                        min(want_piece.length() - piece_position, BLOCK_SIZE_BYTES);
                                    want_file_blocks.push(FileBlockRef {
                                        file_index: want_file_index,
                                        piece_index,
                                        piece_offset: 0,
                                        block_index,
                                        block_length,
                                    });
                                    piece_position += block_length;
                                }
                            } else {
                                let mut piece_position = 0;
                                for block_index in 0..want_piece.block_count() {
                                    let block_length =
                                        min(want_piece.length() - piece_position, BLOCK_SIZE_BYTES);
                                    have_file_blocks.push(FileBlockRef {
                                        file_index: want_file_index,
                                        piece_index,
                                        piece_offset: 0,
                                        block_index,
                                        block_length,
                                    });
                                    piece_position += block_length;
                                }
                            }
                        } else {
                            let mut piece_position = 0;
                            for block_index in 0..want_piece.block_count() {
                                let block_length =
                                    min(want_piece.length() - piece_position, BLOCK_SIZE_BYTES);
                                want_file_blocks.push(FileBlockRef {
                                    file_index: want_file_index,
                                    piece_index,
                                    piece_offset: 0,
                                    block_index,
                                    block_length,
                                });
                                piece_position += block_length;
                            }
                        }
                    }
                }
            } else {
                // File is missing, add all blocks
                for (piece_index, piece) in self.payload.pieces().iter().enumerate() {
                    let mut piece_position = 0;
                    let piece_length = piece.length();
                    for block_index in 0..piece.block_count() {
                        let block_length = min(piece_length - piece_position, BLOCK_SIZE_BYTES);
                        want_file_blocks.push(FileBlockRef {
                            file_index: want_file_index,
                            piece_index,
                            piece_offset: 0,
                            block_index,
                            block_length,
                        });
                        piece_position += block_length;
                    }
                }
            }
        }
        IndexDiff {
            want: want_file_blocks,
            have: have_file_blocks,
        }
    }
}

/// Represent the difference between two indexes.
pub struct IndexDiff {
    pub want: Vec<FileBlockRef>,
    pub have: Vec<FileBlockRef>,
}

/// Reference a block in a piece in a file.
pub struct FileBlockRef {
    pub file_index: usize,
    pub piece_index: usize,
    pub piece_offset: usize,
    pub block_index: usize,
    pub block_length: usize,
}

/// Progress of an indexing process.
#[derive(Clone, Copy, Debug, Default)]
pub struct Progress {
    pub length: u64,
    pub position: u64,
}

/// Indexer represents a process which indexes filesystem contents.
pub struct Indexer {
    root_dir: PathBuf,
    files: Vec<PathBuf>,

    digest_progress_tx: watch::Sender<Progress>,
    index_progress_tx: watch::Sender<Progress>,
    /// Wanted-side alignment: the global starting piece for each file,
    /// taken from the want index. None on the seed side, where bases
    /// accumulate naturally in payload order.
    bases: Option<Vec<usize>>,
}

impl Default for Indexer {
    fn default() -> Self {
        let (digest_progress_tx, _) = watch::channel(Progress::default());
        let (index_progress_tx, _) = watch::channel(Progress::default());
        Self {
            root_dir: Default::default(),
            files: Default::default(),
            bases: None,
            digest_progress_tx,
            index_progress_tx,
        }
    }
}

impl Indexer {
    /// Index a complete local file on disk.
    pub async fn from_file(file: &Path) -> Result<Indexer> {
        match file.try_exists() {
            Ok(true) => {}
            Ok(false) => return Err(io::Error::from(io::ErrorKind::NotFound).into()),
            Err(e) => return Err(e.into()),
        }

        // root is directory containing file
        let resolved_file = file.canonicalize()?;
        let root_dir = &resolved_file
            .parent()
            .ok_or(io::Error::from(io::ErrorKind::NotFound))?;

        let (digest_progress_tx, _) = watch::channel(Progress::default());
        let (index_progress_tx, _) = watch::channel(Progress::default());

        Ok(Indexer {
            root_dir: root_dir.to_path_buf(),
            files: vec![resolved_file],
            bases: None,
            digest_progress_tx,
            index_progress_tx,
        })
    }

    /// Index a local path: a file, or a whole directory tree.
    ///
    /// A directory becomes a multi-file index: every regular file under it,
    /// sorted by relative path, each starting on a fresh piece boundary.
    pub async fn from_path(path: &Path) -> Result<Indexer> {
        let resolved = path.canonicalize()?;
        let meta = tokio::fs::metadata(&resolved).await?;
        if !meta.is_dir() {
            return Indexer::from_file(&resolved).await;
        }
        let mut files: Vec<PathBuf> = vec![];
        let mut stack = vec![resolved.clone()];
        while let Some(dir) = stack.pop() {
            let mut rd = tokio::fs::read_dir(&dir).await?;
            while let Some(entry) = rd.next_entry().await? {
                let p = entry.path();
                let ft = entry.file_type().await?;
                if ft.is_dir() {
                    stack.push(p);
                } else if ft.is_file() {
                    files.push(p);
                }
            }
        }
        if files.is_empty() {
            return Err(io::Error::from(io::ErrorKind::NotFound).into());
        }
        files.sort();
        let (digest_progress_tx, _) = watch::channel(Progress::default());
        let (index_progress_tx, _) = watch::channel(Progress::default());
        Ok(Indexer {
            root_dir: resolved,
            files,
            bases: None,
            digest_progress_tx,
            index_progress_tx,
        })
    }

    /// Index a wanted local file set on disk.
    ///
    /// The wanted files may be incomplete, corrupt, or may not exist yet.
    /// Every wanted file is created if absent (so zero-length files exist
    /// even though no block will ever be fetched for them), oversize ones
    /// are shrunk to the wanted length, and each carries the want index's
    /// own starting piece so the have/want diff compares aligned pieces
    /// even when an earlier file is missing entirely.
    pub async fn from_wanted(want: &Index) -> Result<Indexer> {
        if want.files.is_empty() {
            return Ok(Indexer::default());
        }
        // The root must be canonical before any file under it is: files are
        // canonicalized below, and `index()` strips this root off each one.
        // A literal root over a symlinked path (Android's /data/user/0 is a
        // symlink to /data/data) never prefixes its own canonicalized files,
        // and the whole index fails on the mismatch.
        let _ = tokio::fs::create_dir_all(&want.root).await;
        let root = match want.root.canonicalize() {
            Ok(r) => r,
            Err(_) => return Ok(Indexer::default()),
        };
        let mut files: Vec<PathBuf> = vec![];
        let mut bases: Vec<usize> = vec![];
        for f in want.files.iter() {
            // Belt and braces with the wire decoder's check: nothing in a
            // want index may reach outside its root, whoever built it.
            if !f.is_contained() {
                return Err(anyhow::anyhow!(
                    "refusing to fetch outside the share root: {:?}",
                    f.path()
                ));
            }
            let file_path = root.join(f.path());
            let file_len = TryInto::<u64>::try_into(f.contents().length()).unwrap();
            let ready = async {
                if let Some(parent) = file_path.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                let fh = OpenOptions::new()
                    .write(true)
                    .create(true)
                    .append(true)
                    .open(&file_path)
                    .await?;
                if fh.metadata().await?.len() > file_len {
                    fh.set_len(file_len).await?;
                }
                Ok::<(), Error>(())
            }
            .await
            .is_ok();
            if !ready {
                return Ok(Indexer::default());
            }
            files.push(file_path.canonicalize()?);
            bases.push(f.contents().starting_piece());
        }
        let (digest_progress_tx, _) = watch::channel(Progress::default());
        let (index_progress_tx, _) = watch::channel(Progress::default());
        Ok(Indexer {
            root_dir: root,
            files,
            bases: Some(bases),
            digest_progress_tx,
            index_progress_tx,
        })
    }

    /// Subscribe to progress updates on calculating digests of pieces and the
    /// overall content.
    pub fn subscribe_digest_progress(&self) -> watch::Receiver<Progress> {
        self.digest_progress_tx.subscribe()
    }

    /// Subscribe to progress updates on indexing the filesystem content.
    pub fn subscribe_index_progress(&self) -> watch::Receiver<Progress> {
        self.index_progress_tx.subscribe()
    }

    /// Index the configured content.
    pub async fn index(&self) -> Result<Index> {
        if self.files.is_empty() {
            self.index_progress_tx.send_modify(|p| {
                p.length = 0;
                p.position = 0;
            });
            self.digest_progress_tx.send_modify(|p| {
                p.length = 0;
                p.position = 0;
            });
            return Ok(Index::empty_root(&self.root_dir));
        }
        if self.files.len() == 1 && self.bases.is_none() {
            // The single-file path, byte-for-byte as it always was: the
            // payload digest stays the digest of the file's raw bytes, so
            // existing single-file shares keep their identity.
            let resolved_file = self.files[0].to_owned();
            let payload = self.index_spec(&resolved_file).await?;
            let length = payload.length;
            return Ok(Index {
                root: self.root_dir.to_owned(),
                payload,
                files: vec![FileSpec {
                    path: resolved_file.strip_prefix(&self.root_dir)?.to_owned(),
                    contents: PayloadSlice {
                        starting_piece: 0,
                        piece_offset: 0,
                        length,
                    },
                }],
            });
        }
        // Multi-file: each file scanned independently, its pieces appended
        // at a global base so every file starts on a piece boundary. The
        // payload digest is the BLAKE3 chain of the per-file digests in
        // path order - deterministic on both sides without ever needing to
        // stream one hash across file boundaries.
        let mut pieces: Vec<PayloadPiece> = vec![];
        let mut files: Vec<FileSpec> = vec![];
        let mut chain = Blake3::new();
        let mut total: usize = 0;
        for (i, path) in self.files.iter().enumerate() {
            let local = self.index_spec(path).await?;
            let base = match &self.bases {
                Some(b) => b[i],
                None => pieces.len(),
            };
            match &self.bases {
                Some(_) => {
                    // Wanted side: land each file's pieces at the want
                    // index's own global positions, leaving gaps where
                    // files are missing or short - the diff reads absent
                    // pieces as wanted, which is exactly the truth.
                    if pieces.len() < base + local.pieces.len() {
                        pieces.resize(
                            base + local.pieces.len(),
                            PayloadPiece {
                                digest: [0u8; 32],
                                length: 0,
                            },
                        );
                    }
                    for (j, piece) in local.pieces.iter().enumerate() {
                        pieces[base + j] = piece.clone();
                    }
                }
                None => pieces.extend(local.pieces.iter().cloned()),
            }
            chain.update(&local.digest);
            total += local.length;
            files.push(FileSpec {
                path: path.strip_prefix(&self.root_dir)?.to_owned(),
                contents: PayloadSlice {
                    starting_piece: base,
                    piece_offset: 0,
                    length: local.length,
                },
            });
        }
        Ok(Index {
            root: self.root_dir.to_owned(),
            payload: PayloadSpec {
                digest: chain.finalize().into(),
                length: total,
                pieces,
            },
            files,
        })
    }

    /// Calculate the payload layout for a file's contents.
    async fn index_spec(&self, file: impl AsRef<Path>) -> Result<PayloadSpec> {
        let mut fh = File::open(file.as_ref()).await?;
        let file_meta = fh.metadata().await?;
        let mut payload = PayloadSpec::default();

        let (task_tx, task_rx) = unbounded::<Option<ScanTask>>();
        let (result_tx, result_rx) = unbounded::<ScanResult>();
        let mut scanners = JoinSet::new();

        let file_len = TryInto::<usize>::try_into(file_meta.len()).unwrap();
        let n_tasks = file_len / INDEX_BUFFER_SIZE
            + if !file_len.is_multiple_of(INDEX_BUFFER_SIZE) {
                1
            } else {
                0
            };

        for scan_index in 0..n_tasks {
            let scan_task = ScanTask {
                offset: scan_index * INDEX_BUFFER_SIZE,
                fh: File::open(file.as_ref()).await?,
            };
            task_tx.send_async(Some(scan_task)).await?;
        }
        task_tx.send_async(None).await?;

        for _ in 0..num_cpus::get() {
            scanners.spawn(Self::scan(
                task_tx.clone(),
                task_rx.clone(),
                result_tx.clone(),
            ));
        }

        let digest_progress_tx = self.digest_progress_tx.clone();
        let file_len = file_meta.len();
        let digest_task = spawn(async move {
            let mut buf = vec![0; INDEX_BUFFER_SIZE];
            let mut payload_digest = Blake3::new();
            loop {
                let mut total_rd = 0;
                while total_rd < INDEX_BUFFER_SIZE {
                    let rd = fh.read(&mut buf[total_rd..INDEX_BUFFER_SIZE]).await?;
                    if rd == 0 {
                        break;
                    }
                    total_rd += rd;

                    digest_progress_tx.send_modify(|p| {
                        p.length = file_len;
                        p.position += TryInto::<u64>::try_into(rd).unwrap();
                    });
                }
                if total_rd == 0 {
                    break;
                }
                payload_digest.update(&buf[..total_rd]);
            }
            Ok::<[u8; 32], Error>(payload_digest.finalize().into())
        });

        let mut scan_results: Vec<ScanResult> = vec![];
        loop {
            select! {
                recv_result = result_rx.recv_async() => {
                    let scan_result = recv_result?;
                    self.index_progress_tx.send_modify(|p| {
                        p.length = TryInto::<u64>::try_into(file_meta.len()).unwrap();
                        p.position += TryInto::<u64>::try_into(scan_result.piece.length).unwrap();
                    });
                    scan_results.push(scan_result);
                }
                joined = scanners.join_next() => {
                    match joined {
                        None => {
                            if result_rx.is_empty() {
                                break;
                            }
                        }
                        Some(Err(e)) => return Err(e.into()),
                        Some(Ok(Err(e))) => return Err(e),
                        _ => continue,
                    }
                }
            }
        }

        // Task and result channels should be empty. Dropping them defensively
        // to force any unexpected receive still running to error noisily.
        drop(task_tx);
        drop(result_tx);

        scan_results.sort_by_key(|r| r.piece_index);
        payload.pieces = scan_results.drain(..).map(|r| r.piece).collect();
        if !payload.pieces.is_empty() {
            payload.length = (payload.pieces.len() - 1) * PIECE_SIZE_BYTES;
            payload.length += payload.pieces[payload.pieces.len() - 1].length;
        }

        let payload_digest = digest_task.await??;
        payload.digest = payload_digest;
        Ok(payload)
    }

    /// Process filesystem scanning tasks from a channel.
    async fn scan(
        task_tx: Sender<Option<ScanTask>>,
        task_rx: Receiver<Option<ScanTask>>,
        result_tx: Sender<ScanResult>,
    ) -> Result<()> {
        loop {
            match task_rx.recv_async().await {
                Ok(None) => {
                    // Create tombstones on channels to wake the next receiver.
                    task_tx.send_async(None).await?;
                    return Ok(());
                }
                Ok(Some(mut scan_task)) => {
                    scan_task
                        .fh
                        .seek(std::io::SeekFrom::Start(scan_task.offset.try_into()?))
                        .await?;
                    let mut total_rd = 0;
                    let mut buf = vec![0u8; INDEX_BUFFER_SIZE];
                    while total_rd < INDEX_BUFFER_SIZE {
                        let rd = scan_task
                            .fh
                            .read(&mut buf[total_rd..INDEX_BUFFER_SIZE])
                            .await?;
                        if rd == 0 {
                            break;
                        }
                        total_rd += rd;
                    }
                    if total_rd == 0 {
                        return Ok(());
                    }

                    let mut offset = 0;
                    while offset < total_rd {
                        let piece_index = (scan_task.offset + offset) / PIECE_SIZE_BYTES;
                        let piece_length = min(PIECE_SIZE_BYTES, total_rd - offset);
                        let mut piece_digest = Blake3::new();
                        piece_digest.update(&buf[offset..offset + piece_length]);
                        let mut piece = PayloadPiece {
                            digest: [0u8; 32],
                            length: piece_length,
                        };
                        piece.digest = piece_digest.finalize().into();
                        let scan_result = ScanResult { piece_index, piece };
                        result_tx.send_async(scan_result).await?;
                        offset += PIECE_SIZE_BYTES;
                    }
                }
                Err(e) => return Err(e.into()),
            };
        }
    }
}

/// Map file paths in the indexed content onto the payload layout.
#[derive(Debug, PartialEq, Clone)]
pub struct FileSpec {
    /// File name.
    path: PathBuf,

    /// File contents in payload.
    contents: PayloadSlice,
}

impl FileSpec {
    pub fn new(path: PathBuf, contents: PayloadSlice) -> FileSpec {
        FileSpec { path, contents }
    }

    pub fn path(&self) -> &Path {
        self.path.as_ref()
    }

    /// Whether this path can only ever land *under* a fetch root.
    ///
    /// DUCAT modification (see ../../STIGMERGE-NOTICE.md): the path comes
    /// off the wire, from whoever published the share. `root.join(path)`
    /// discards the root for an absolute path and walks out of it for a
    /// `..` — and the fetcher creates, truncates and writes that file
    /// before any hash is checked. Only plain relative components, and at
    /// least one of them.
    pub fn is_contained(&self) -> bool {
        use std::path::Component;
        let mut any = false;
        for c in self.path.components() {
            match c {
                Component::Normal(_) => any = true,
                Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => return false,
            }
        }
        any
    }

    pub fn contents(&self) -> PayloadSlice {
        self.contents
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct PayloadSlice {
    /// Starting piece where the slice begins.
    starting_piece: usize,

    /// Offset from the beginning of the starting piece where the slice starts.
    piece_offset: usize,

    /// Length of the slice. This can span multiple slices.
    length: usize,
}

impl PayloadSlice {
    pub fn new(starting_piece: usize, piece_offset: usize, length: usize) -> PayloadSlice {
        PayloadSlice {
            starting_piece,
            piece_offset,
            length,
        }
    }

    pub fn starting_piece(&self) -> usize {
        self.starting_piece
    }

    pub fn piece_offset(&self) -> usize {
        self.piece_offset
    }

    pub fn length(&self) -> usize {
        self.length
    }
}

/// Repreent the layout of the indexed content payload.
#[derive(Debug, PartialEq, Clone, Default)]
pub struct PayloadSpec {
    /// BLAKE3 digest of the complete payload.
    digest: [u8; 32],

    /// Length of the complete payload.
    length: usize,

    /// Pieces in the file.
    pieces: Vec<PayloadPiece>,
}

impl PayloadSpec {
    pub fn new(digest: [u8; 32], length: usize, pieces: Vec<PayloadPiece>) -> PayloadSpec {
        PayloadSpec {
            digest,
            length,
            pieces,
        }
    }

    pub fn digest(&self) -> &[u8] {
        &self.digest[..]
    }

    pub fn length(&self) -> usize {
        self.length
    }

    pub fn pieces(&self) -> &Vec<PayloadPiece> {
        &self.pieces
    }
}

#[derive(Debug)]
struct ScanTask {
    offset: usize,
    fh: File,
}

#[derive(Debug)]
struct ScanResult {
    piece_index: usize,
    piece: PayloadPiece,
}

/// A piece of the content payload which is verifiable with a content digest.
#[derive(Debug, PartialEq, Clone)]
pub struct PayloadPiece {
    /// BLAKE3 digest of the complete piece.
    digest: [u8; 32],

    /// Length of the piece.
    /// May be < BLOCK_SIZE_BYTES * PIECE_SIZE_BLOCKS if the last piece.
    length: usize,
}

impl PayloadPiece {
    pub fn new(digest: [u8; 32], length: usize) -> PayloadPiece {
        PayloadPiece { digest, length }
    }

    pub fn digest(&self) -> &[u8] {
        &self.digest[..]
    }

    pub fn length(&self) -> usize {
        self.length
    }

    pub fn block_count(&self) -> usize {
        self.length / BLOCK_SIZE_BYTES
            + if !self.length.is_multiple_of(BLOCK_SIZE_BYTES) {
                1
            } else {
                0
            }
    }
}

#[cfg(test)]
mod from_file_tests;

#[cfg(test)]
mod multi_file_tests;

#[cfg(test)]
mod want_have_tests;

#[cfg(test)]
mod tests;
