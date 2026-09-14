// Vendored from the upstream CLI crate (stigmerge/src/share.rs) so an
// embedding application drives seeds and fetches with the same
// orchestration the CLI uses — resolver, announcer, gossip, verifier,
// seeder and fetcher wired identically. See ../STIGMERGE-NOTICE.md for
// origin, credit and the modifications.

use std::{ops::Deref, path::PathBuf, sync::Arc};

use anyhow::{Error, Result};
use path_absolutize::Absolutize;
use stigmerge_fileindex::{Indexer, Progress};
use crate::{
    content_addressable::ContentAddressable,
    fetcher,
    peer_gossip::PeerGossip,
    piece_verifier,
    proto::Digest,
    record::{StableHaveMap, StableShareRecord},
    seeder, share_announcer, share_resolver,
    types::{LocalShareInfo, RemoteShareInfo},
    CancelError, Retry,
};
use tokio::{
    select,
    sync::{broadcast, watch, RwLock},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, warn};
use veilid_core::RecordKey;
use veilnet::{
    connection::{RoutingContext, API},
    Connection,
};

#[derive(Debug)]
pub enum Mode {
    Seed {
        /// Local file to seed.
        path: PathBuf,
    },
    Fetch {
        /// Local directory where the fetched file(s) will be placed.
        root: PathBuf,

        /// Content digest of the share index (not the payload itself).
        ///
        /// The index digest is used to authenticate other peers advertising
        /// that they offer the same payload. If their share DHT has an index
        /// that doesn't match, they're not.
        want_index_digest: Option<Digest>,

        /// Share key(s) used to bootstrap into the swarm of peers sharing this
        /// content. At least one is required.
        share_keys: Vec<RecordKey>,

        /// DUCAT modification (see ../STIGMERGE-NOTICE.md): the most this
        /// fetch may be, in bytes.
        ///
        /// The index says how big the payload is and the fetcher believed
        /// it — it created and sized every file named before a byte was
        /// verified, so a hearted home fetched unattended could be any size
        /// its publisher liked. The embedding application knows what kind
        /// of thing it asked for and therefore what a sane ceiling is (a
        /// gallery is not a release); it passes that here, and an index
        /// that declares more is refused before anything is created.
        ///
        /// `None` keeps the old behaviour and is for tests and harnesses;
        /// the absolute ceilings in `stigmerge_fileindex` still apply.
        max_bytes: Option<u64>,
    },
}

/// DUCAT modification (see ../STIGMERGE-NOTICE.md): an index that declares
/// more than the caller will take.
///
/// Its own type, and not a plain message, so the embedding application can
/// say it to the person in their own words and units — "this bundle says it
/// is 3.2 GB; the cap for a home is 256 MB" — rather than showing them a
/// sentence from the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooLarge {
    /// What the share's index and header say the payload is.
    pub declared: u64,
    /// The ceiling this fetch was given.
    pub max: u64,
}

impl std::fmt::Display for TooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the share declares {} bytes; this fetch will take at most {}",
            self.declared, self.max
        )
    }
}

impl std::error::Error for TooLarge {}

/// The [`TooLarge`] in an error chain, if that is why a fetch was refused.
pub fn too_large(e: &Error) -> Option<TooLarge> {
    for cause in e.chain() {
        if let Some(t) = cause.downcast_ref::<TooLarge>() {
            return Some(*t);
        }
    }
    None
}

/// Is this index within the caller's budget?
///
/// DUCAT modification (see ../STIGMERGE-NOTICE.md). Its own function so the
/// rule can be tested without a swarm: it is the whole of the byte ceiling,
/// and it is asked exactly once, in `start`, before the fetcher creates a
/// single file. `None` is "no budget given" — the absolute ceilings in
/// `stigmerge_fileindex` still apply, and `read_index` has already made the
/// header, the pieces and the files agree on the number weighed here.
pub fn check_budget(
    index: &stigmerge_fileindex::Index,
    max_bytes: Option<u64>,
) -> std::result::Result<(), TooLarge> {
    match max_bytes {
        Some(max) if index.declared_length() > max => Err(TooLarge {
            declared: index.declared_length(),
            max,
        }),
        _ => Ok(()),
    }
}

impl Mode {
    pub fn share_keys(&self) -> &[RecordKey] {
        match self {
            Mode::Seed { .. } => &[],
            Mode::Fetch { share_keys, .. } => share_keys,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Event {
    /// Share info has been published to the Veilid DHT.
    ShareInfo(Box<LocalShareInfo>),

    /// Fetch status has changed.
    FetcherStatus(fetcher::Status),

    /// Seeder is indexing and verify the local copy of the share. For a large
    /// local share, this can take a little while, so progress is reported.
    SeederLoading {
        index_progress: stigmerge_fileindex::Progress,
        verify_progress: stigmerge_fileindex::Progress,
    },

    /// Seeder is available to service block requests. The share may still be
    /// incomplete. FetcherStatus(fetcher::Status::Done) indicates the share is
    /// complete.
    SeederAvailable,
}

pub struct Share<C: Connection> {
    conn: C,
    mode: Mode,
    retry: Retry,

    events_tx: broadcast::Sender<Event>,
    events_rx: broadcast::Receiver<Event>,

    /// DUCAT modification: kept so an embedding host can ask which pieces
    /// are verified and draw them. Cheap — PieceVerifier is an Arc handle.
    piece_verifier: Option<piece_verifier::PieceVerifier>,

    pub(crate) tasks: JoinSet<Result<()>>,
}

const SHARE_EVENTS_CAPACITY: usize = 131072;

impl<C: Connection + Clone + Send + Sync + 'static> Share<C> {
    pub fn new(conn: C, mode: Mode) -> Result<Self> {
        let (events_tx, events_rx) = broadcast::channel(SHARE_EVENTS_CAPACITY);
        let share = Self {
            conn,
            mode,
            retry: Retry::default(),
            events_tx,
            events_rx,
            piece_verifier: None,
            tasks: JoinSet::new(),
        };
        Ok(share)
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<Event> {
        self.events_rx.resubscribe()
    }

    /// DUCAT modification: verified pieces as a bitmap, and how many there
    /// are. None until `start` has built the verifier.
    pub async fn verified_bitmap(&self) -> Option<(Vec<u8>, usize)> {
        match &self.piece_verifier {
            Some(v) => Some(v.verified_bitmap().await),
            None => None,
        }
    }

    pub async fn start(&mut self, cancel: CancellationToken) -> Result<()> {
        let root = match self.mode {
            // DUCAT multi-file: a directory seed is rooted at the directory
            // itself; a file seed keeps the old parent-directory root.
            Mode::Seed { ref path } => {
                let abs = path.absolutize()?.to_path_buf();
                if abs.is_dir() {
                    abs
                } else {
                    abs.parent()
                        .ok_or(Error::msg("cannot determine parent directory"))?
                        .to_path_buf()
                }
            }
            Mode::Fetch { ref root, .. } => root.to_owned(),
        };

        // Set up share resolver
        let (share_resolver, resolver_task) = share_resolver::ShareResolver::new_task(
            cancel.clone(),
            self.retry.clone(),
            self.conn.clone(),
            &root,
        );
        self.tasks.spawn(async move {
            resolver_task.await??;
            Ok(())
        });

        // Resolve or create index based on mode
        let mut want_index = None;
        let mut remote_shares = vec![];
        match &self.mode {
            Mode::Fetch {
                share_keys,
                want_index_digest,
                max_bytes,
                ..
            } => {
                for share_key in share_keys.iter() {
                    // DUCAT modification: the caller hands a Digest, and a
                    // Digest is 32 bytes, not a hex string to decode.
                    let want_index_digest: Option<[u8; 32]> = *want_index_digest;

                    // Resolve the index from the bootstrap peer
                    let (_, header) =
                        StableShareRecord::new_remote(&mut self.conn, share_key).await?;
                    let mut index =
                        StableShareRecord::read_index(&mut self.conn, share_key, &header, &root)
                            .await?;
                    let remote_index_digest = index.digest()?;

                    // DUCAT modification (see ../STIGMERGE-NOTICE.md): the
                    // byte ceiling, here and nowhere later. Below this line
                    // the indexer creates and sizes every file the index
                    // names; above it, nothing of the publisher's has
                    // touched the disk. `read_index` has already made the
                    // header, the pieces and the files agree on this
                    // number, so refusing on it refuses the whole fetch.
                    check_budget(&index, *max_bytes)?;

                    // Verify the index matches what we want
                    if let Some(want_digest) = want_index_digest {
                        if remote_index_digest != want_digest {
                            warn!(
                                "remote share does not match wanted index digest: expected {}, got {}",
                                hex::encode(&want_digest[..]),
                                hex::encode(&remote_index_digest[..])
                            );
                            continue;
                        }
                    }

                    if let Err(err) = async {
                        let route_id = self
                            .conn
                            .routing_context()
                            .api()
                            .import_remote_private_route(header.route_data().to_vec())?;
                        // DUCAT modification (see ../STIGMERGE-NOTICE.md):
                        // a header off the wire need not carry a have-map
                        // reference, and `unwrap()` on a field a stranger
                        // fills in is a remote panic. A share without one
                        // is simply not one we can fetch from.
                        let have_map_ref = header
                            .have_map()
                            .ok_or_else(|| Error::msg("share header names no have-map"))?;
                        let have_map =
                            StableHaveMap::read_remote(&mut self.conn, have_map_ref.key(), &index)
                                .await?;

                        // Store the remote share
                        remote_shares.push(RemoteShareInfo {
                            key: share_key.clone(),
                            header,
                            index: index.clone(),
                            index_digest: remote_index_digest,
                            route_id,
                            have_map,
                        });
                        Ok::<(), anyhow::Error>(())
                    }
                    .await
                    {
                        warn!(?err, ?share_key, "failed to resolve share");
                        continue;
                    }

                    // Store the index for piece verification below
                    want_index.get_or_insert(index);
                }
            }
            Mode::Seed { path } => {
                let indexer = Indexer::from_path(path).await?;
                self.tasks.spawn(Self::send_indexer_progress(
                    cancel.clone(),
                    indexer.subscribe_index_progress(),
                    indexer.subscribe_digest_progress(),
                    self.events_tx.clone(),
                ));
                let index = indexer.index().await?;
                want_index.get_or_insert(index);
            }
        };
        let index = want_index.ok_or(Error::msg("failed to resolve index"))?;

        // For seeding, announce our own share
        let share = {
            let share_announcer = share_announcer::ShareAnnouncer::new(
                cancel.clone(),
                self.retry.clone(),
                self.conn.clone(),
                index.clone(),
            )
            .await?;
            let share_info = share_announcer.share_info().await;
            debug!(key = ?share_info.key, index_digest = hex::encode(share_info.want_index_digest));
            {
                let cancel = cancel.clone();
                self.tasks.spawn(async move {
                    let res = share_announcer.run().await;
                    cancel.cancel();
                    res
                });
            }

            self.events_tx
                .send(Event::ShareInfo(Box::new(share_info.clone())))?;
            share_info
        };

        // Set up peer gossip
        let peer_gossip =
            PeerGossip::new(self.conn.clone(), share.clone(), share_resolver.clone()).await?;
        {
            let retry = self.retry.clone();
            let cancel = cancel.clone();
            self.tasks.spawn(async move {
                peer_gossip.run(cancel, retry).await.map_err(|err| {
                    error!(?err, "peer gossip task");
                    err
                })
            });
        }

        // Set up piece verifier
        let shared_index = Arc::new(RwLock::new(index.clone()));
        let piece_verifier = piece_verifier::PieceVerifier::new(shared_index.clone()).await;
        self.piece_verifier = Some(piece_verifier.clone());

        // All peers are seeders
        let seeder =
            seeder::Seeder::new(self.conn.clone(), share.clone(), piece_verifier.clone()).await?;
        self.tasks
            .spawn(seeder.run(cancel.clone(), self.retry.clone()));

        match &self.mode {
            Mode::Seed { .. } => {
                // Verify all pieces to mark them as available for seeding
                for (piece_index, piece) in index.payload().pieces().iter().enumerate() {
                    for block_index in 0..piece.block_count() {
                        let piece_state = crate::types::PieceState::new(
                            0,
                            piece_index,
                            0,
                            piece.block_count(),
                            block_index,
                        );
                        piece_verifier.update_piece(piece_state).await?;
                    }
                }

                self.events_tx.send(Event::SeederAvailable)?;
            }
            Mode::Fetch { .. } => {
                if remote_shares.is_empty() {
                    anyhow::bail!("failed to resolve initial shares");
                }

                // Set up fetcher for fetching mode
                let fetcher = fetcher::Fetcher::new(
                    self.conn.clone(),
                    share.clone(),
                    piece_verifier,
                    share_resolver.clone(),
                    remote_shares,
                )
                .await;

                // Add any remote shares we resolved
                for share_key in self.mode.share_keys() {
                    share_resolver.add_share(share_key).await?;
                }

                self.tasks.spawn(Self::send_fetch_progress(
                    cancel.clone(),
                    fetcher.subscribe_fetcher_status(),
                    self.events_tx.clone(),
                ));
                self.tasks
                    .spawn(fetcher.run(cancel.clone(), self.retry.clone()));
            }
        }

        Ok(())
    }

    pub async fn join(mut self) -> Result<()> {
        crate::fetcher::join_drain(&mut self.tasks).await
    }

    async fn send_indexer_progress(
        cancel: CancellationToken,
        mut subscribe_index_progress: watch::Receiver<stigmerge_fileindex::Progress>,
        mut subscribe_digest_progress: watch::Receiver<stigmerge_fileindex::Progress>,
        events_tx: broadcast::Sender<Event>,
    ) -> Result<()> {
        let mut index_progress = Progress::default();
        let mut verify_progress = Progress::default();
        loop {
            select! {
                _ = cancel.cancelled() => {
                    return Err(CancelError.into());
                }
                res = subscribe_index_progress.changed() => {
                    res?;
                    let progress = subscribe_index_progress.borrow_and_update();
                    progress.clone_into(&mut index_progress);
                    events_tx.send(Event::SeederLoading{
                        index_progress,
                        verify_progress,
                    })?;
                }
                res = subscribe_digest_progress.changed() => {
                    res?;
                    let progress = subscribe_digest_progress.borrow_and_update();
                    progress.clone_into(&mut verify_progress);
                    events_tx.send(Event::SeederLoading{
                        index_progress,
                        verify_progress,
                    })?;
                    if progress.length == progress.position {
                        return Ok(());
                    }
                }
            }
        }
    }

    async fn send_fetch_progress(
        cancel: CancellationToken,
        mut subscribe_fetcher_status: watch::Receiver<fetcher::Status>,
        events_tx: broadcast::Sender<Event>,
    ) -> Result<()> {
        loop {
            select! {
                _ = cancel.cancelled() => {
                    return Err(CancelError.into());
                }
                res = subscribe_fetcher_status.changed() => {
                    res?;
                    let progress = subscribe_fetcher_status.borrow_and_update();
                    events_tx.send(Event::FetcherStatus(progress.clone()))?;
                    if let &fetcher::Status::Done = progress.deref() {
                        return Ok(());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use stigmerge_fileindex::{Indexer, PIECE_SIZE_BYTES};
    use tempfile::NamedTempFile;

    use super::*;

    async fn index_of(bytes: usize) -> stigmerge_fileindex::Index {
        let mut tempf = NamedTempFile::new().expect("temp file");
        tempf.write_all(&vec![b'z'; bytes]).expect("write");
        let indexer = Indexer::from_file(tempf.path()).await.expect("indexer");
        indexer.index().await.expect("index")
    }

    /// DUCAT modification (see ../STIGMERGE-NOTICE.md): the byte ceiling.
    /// An index declaring more than the caller will take is refused, and
    /// both numbers come back so the caller can say them to a person.
    #[tokio::test]
    async fn a_share_bigger_than_the_budget_is_refused() {
        let index = index_of(PIECE_SIZE_BYTES * 3).await;
        let declared = index.declared_length();
        assert_eq!(declared, (PIECE_SIZE_BYTES * 3) as u64);

        // Under the ceiling: fetched.
        check_budget(&index, Some(declared)).expect("exactly the budget is inside it");
        check_budget(&index, Some(declared + 1)).expect("under the budget");
        check_budget(&index, None).expect("no budget given");

        // Over it: refused, by name, with both figures.
        let err = check_budget(&index, Some(declared - 1)).expect_err("over the budget");
        assert_eq!(
            err,
            TooLarge {
                declared,
                max: declared - 1
            }
        );
        let err = check_budget(&index, Some(1)).expect_err("a gallery-sized budget");
        assert_eq!(err.declared, declared);
        assert_eq!(err.max, 1);

        // And it survives being carried as an anyhow error, which is how
        // `start` hands it back to the embedding application.
        let wrapped: Error = err.into();
        assert_eq!(too_large(&wrapped), Some(err));
        assert_eq!(too_large(&Error::msg("something else")), None);
    }
}
