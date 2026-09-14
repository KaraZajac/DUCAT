use super::*;
use std::path::PathBuf;

#[test]
fn test_canonicalize() {
    let mut index = Index {
        root: PathBuf::from("/test"),
        payload: PayloadSpec::default(),
        files: vec![
            FileSpec::new(PathBuf::from("z.txt"), PayloadSlice::new(0, 0, 0)),
            FileSpec::new(PathBuf::from("a.txt"), PayloadSlice::new(0, 0, 0)),
            FileSpec::new(PathBuf::from("m/file.txt"), PayloadSlice::new(0, 0, 0)),
        ],
    };

    // Store original files for comparison
    let original_files: Vec<PathBuf> = index.files.iter().map(|f| f.path().to_owned()).collect();

    index.canonicalize();

    // Verify files are sorted by path
    let sorted_files: Vec<PathBuf> = index.files.iter().map(|f| f.path().to_owned()).collect();

    assert_eq!(
        sorted_files,
        vec![
            PathBuf::from("a.txt"),
            PathBuf::from("m/file.txt"),
            PathBuf::from("z.txt"),
        ]
    );

    // Verify all original files are still present
    assert_eq!(
        original_files
            .iter()
            .collect::<std::collections::HashSet<_>>(),
        sorted_files
            .iter()
            .collect::<std::collections::HashSet<_>>()
    );
}

// DUCAT modification (see ../../STIGMERGE-NOTICE.md): what an index off the
// wire is allowed to declare. Every honest index this crate builds passes
// these; each refusal is one way a publisher's numbers could otherwise make
// a fetcher create, size or seek something it chose.

/// One honest piece of `len` bytes, with a digest nobody checks here.
fn piece(len: usize) -> PayloadPiece {
    PayloadPiece::new([7u8; 32], len)
}

fn file(path: &str, starting_piece: usize, len: usize) -> FileSpec {
    FileSpec::new(
        PathBuf::from(path),
        PayloadSlice::new(starting_piece, 0, len),
    )
}

#[test]
fn an_honest_shape_passes() {
    // One file of two whole pieces and a short tail.
    let pieces = vec![
        piece(PIECE_SIZE_BYTES),
        piece(PIECE_SIZE_BYTES),
        piece(1234),
    ];
    let files = vec![file("a.bin", 0, PIECE_SIZE_BYTES * 2 + 1234)];
    check_index_shape(&pieces, &files, None).expect("one file");
    check_index_shape(&pieces, &files, Some(PIECE_SIZE_BYTES * 2 + 1234)).expect("with a header");

    // Two files, piece-aligned, each starting on a fresh piece — and a
    // zero-length one, which occupies no piece at all.
    let pieces = vec![piece(10), piece(PIECE_SIZE_BYTES), piece(5)];
    let files = vec![
        file("a", 0, 10),
        file("b", 1, PIECE_SIZE_BYTES + 5),
        file("empty", 3, 0),
    ];
    check_index_shape(&pieces, &files, Some(PIECE_SIZE_BYTES + 15)).expect("three files");

    // Nothing at all is a shape too.
    check_index_shape(&[], &[], Some(0)).expect("empty");
}

#[test]
fn a_piece_longer_than_a_piece_is_refused() {
    let pieces = vec![piece(PIECE_SIZE_BYTES + 1)];
    let files = vec![file("a", 0, PIECE_SIZE_BYTES + 1)];
    assert_eq!(
        check_index_shape(&pieces, &files, None),
        Err(ShapeError::PieceLength {
            piece_index: 0,
            length: PIECE_SIZE_BYTES + 1
        })
    );
    // And an empty one: no indexer makes one, and it would let a hostile
    // index pad its piece list for free.
    assert_eq!(
        check_index_shape(&[piece(0)], &[file("a", 0, 0)], None),
        Err(ShapeError::PieceLength {
            piece_index: 0,
            length: 0
        })
    );
}

#[test]
fn a_slice_outside_the_pieces_list_is_refused() {
    // The panic at piece_verifier.rs's empty_pieces, as an index: a file
    // whose pieces run off the end of the list.
    let pieces = vec![piece(PIECE_SIZE_BYTES)];
    let files = vec![file("a", 0, PIECE_SIZE_BYTES * 4)];
    assert_eq!(
        check_index_shape(&pieces, &files, None),
        Err(ShapeError::SliceOutsidePieces {
            file_index: 0,
            starting_piece: 0,
            piece_count: 4,
            have_pieces: 1
        })
    );
    // And one that starts past the end.
    let files = vec![file("a", 9, PIECE_SIZE_BYTES)];
    assert!(matches!(
        check_index_shape(&pieces, &files, None),
        Err(ShapeError::SliceOutsidePieces { .. })
    ));
}

#[test]
fn a_slice_that_starts_inside_a_piece_is_refused() {
    let pieces = vec![piece(100)];
    let files = vec![FileSpec::new(
        PathBuf::from("a"),
        PayloadSlice::new(0, 1, 100),
    )];
    assert_eq!(
        check_index_shape(&pieces, &files, None),
        Err(ShapeError::UnalignedSlice { file_index: 0 })
    );
}

#[test]
fn pieces_that_do_not_add_up_are_refused() {
    // The file says one thing, its own pieces say another.
    let pieces = vec![piece(100)];
    let files = vec![file("a", 0, 900)];
    assert_eq!(
        check_index_shape(&pieces, &files, None),
        Err(ShapeError::SliceLengthMismatch {
            file_index: 0,
            declared: 900,
            pieces: 100
        })
    );

    // The header says a third thing. This is the one that matters most: the
    // byte ceiling is weighed against the header's number, so the header
    // must be the pieces' number.
    let pieces = vec![piece(100)];
    let files = vec![file("a", 0, 100)];
    assert_eq!(
        check_index_shape(&pieces, &files, Some(9_000_000_000)),
        Err(ShapeError::LengthMismatch {
            declared: 9_000_000_000,
            pieces: 100,
            files: 100
        })
    );
}

#[test]
fn a_piece_claimed_twice_or_not_at_all_is_refused() {
    // Two files over one piece: the sums still balance, the shape does not.
    let pieces = vec![piece(100), piece(100)];
    let files = vec![file("a", 0, 100), file("b", 0, 100)];
    assert_eq!(
        check_index_shape(&pieces, &files, Some(200)),
        Err(ShapeError::PieceNotClaimedOnce {
            piece_index: 0,
            claims: 2
        })
    );

    // A piece nobody claims — padding, to make an index look bigger than
    // the files it names.
    let pieces = vec![piece(100), piece(PIECE_SIZE_BYTES)];
    let files = vec![file("a", 0, 100)];
    assert_eq!(
        check_index_shape(&pieces, &files, Some(100)),
        Err(ShapeError::PieceNotClaimedOnce {
            piece_index: 1,
            claims: 0
        })
    );
}

#[test]
fn absurd_counts_are_refused() {
    let pieces: Vec<PayloadPiece> = vec![];
    let files: Vec<FileSpec> = (0..MAX_FILES + 1).map(|i| file(&format!("f{i}"), 0, 0)).collect();
    assert_eq!(
        check_index_shape(&pieces, &files, None),
        Err(ShapeError::TooManyFiles(MAX_FILES + 1))
    );

    let pieces: Vec<PayloadPiece> = (0..MAX_PIECES + 1).map(|_| piece(1)).collect();
    assert_eq!(
        check_index_shape(&pieces, &[], None),
        Err(ShapeError::TooManyPieces(MAX_PIECES + 1))
    );
}

#[test]
fn a_payload_over_the_absolute_ceiling_is_refused() {
    // 16 GiB + one piece, declared with whole pieces: refused on the
    // running total, before the files are even looked at.
    let n = (MAX_PAYLOAD_BYTES / PIECE_SIZE_BYTES as u64) as usize + 1;
    let pieces: Vec<PayloadPiece> = (0..n).map(|_| piece(PIECE_SIZE_BYTES)).collect();
    assert!(matches!(
        check_index_shape(&pieces, &[], None),
        Err(ShapeError::PayloadTooLarge(_))
    ));
}

#[test]
fn an_index_built_here_always_passes_its_own_check() {
    // The rule that matters for honest peers: whatever this indexer
    // produces, the checker accepts.
    let index = Index::new(
        PathBuf::from("/test"),
        PayloadSpec::new([0u8; 32], 0, vec![]),
        vec![],
    );
    index.check_shape().expect("an empty index");
}
