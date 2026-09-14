use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use stigmerge_fileindex::{BLOCK_SIZE_BYTES, PIECE_SIZE_BLOCKS};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt},
    select,
    sync::{Mutex, Semaphore},
    task::JoinSet,
    time::interval,
};
use tokio_util::sync::CancellationToken;
use tracing::{error, instrument, trace, warn};
use veilid_core::{OperationId, RouteId, VeilidAppCall};
use veilnet::{
    connection::{RoutingContext, UpdateHandler, API},
    Connection,
};

// DUCAT modification (see ../STIGMERGE-NOTICE.md): seeder back-pressure.
//
// Serving is the one thing a DUCAT node does for strangers, and upstream did
// it with no ceiling anywhere: an unbounded queue took every block request
// that arrived, the loop spawned a task per request, and each task held a
// 32 KiB buffer while queuing behind one mutex that was held across the
// network reply. A peer that asked fast enough — or many peers, or one peer
// with a script — grew the queue, the task set and the memory without limit,
// on somebody's phone. Nothing about it needed the piece to exist.
//
// Three bounds, cheapest first: a rate per route, a bounded queue, and a
// ceiling on replies in flight. Each drops what it cannot take rather than
// buffering it — a dropped block request costs the asker one retry, which is
// the fetcher's ordinary weather, and costs us nothing.

/// Block requests waiting to be served. Beyond this the queue is full and
/// new requests are dropped.
const MAX_QUEUED_BLOCK_REQUESTS: usize = 256;

/// Replies in flight at once. Each holds one block (32 KiB) while it is read
/// and sent, so this is also the seeder's memory ceiling: 256 KiB.
const MAX_INFLIGHT_REPLIES: usize = 8;

/// Requests one route may spend in [`ROUTE_BUCKET_WINDOW`].
///
/// Sixty-four 32 KiB blocks in ten seconds is about 205 KiB/s per inbound
/// route: a rate a phone can serve all day rather than the best a desk on a
/// fast line could manage. It is deliberately a *pace*, not a cliff — a
/// request that arrives with no token waits for one (up to
/// [`MAX_SHAPE_WAIT`]) instead of being dropped, so an honest fetcher asking
/// faster than this is slowed rather than starved of replies it will sit and
/// wait for. A flood outruns the wait and is dropped at the queue.
///
/// Worth knowing when tuning it: the live proof runs moved 25 MiB in 97.5 s
/// (~80 requests per 10 s) and 100 MiB in 279.9 s (~114), both above this,
/// so this rate does cap a single fast transfer. That is the trade the
/// review asked for — a seeder is a background service on somebody else's
/// device, and without a rate the peer that asks hardest decides how much of
/// it they get.
const ROUTE_BUCKET_CAPACITY: f64 = 64.0;

/// The window [`ROUTE_BUCKET_CAPACITY`] is spent over.
const ROUTE_BUCKET_WINDOW: Duration = Duration::from_secs(10);

/// The longest a request will wait for a token before it is dropped.
///
/// Past this the asker has given up on the reply anyway, so serving it is
/// work spent on nobody.
const MAX_SHAPE_WAIT: Duration = Duration::from_secs(5);

/// Routes remembered by the bucket. Our own private routes rotate, so the
/// map would otherwise grow for the life of the process.
const MAX_TRACKED_ROUTES: usize = 64;

/// How long a route is remembered after its last request.
const ROUTE_BUCKET_FORGET: Duration = Duration::from_secs(300);

/// A token bucket per inbound private route.
///
/// The route the call arrived on is all a seeder can key on: a request over
/// a private route carries no sender, by design. It is therefore a rate per
/// swarm rather than per asker — which is the honest shape of the limit, and
/// the reason it is set well above what one healthy fetcher uses.
#[derive(Default)]
struct RouteBuckets {
    inner: std::sync::Mutex<HashMap<Option<RouteId>, Bucket>>,
}

struct Bucket {
    tokens: f64,
    last: Instant,
}

impl RouteBuckets {
    /// Spend a token for this route, or say how long until there is one.
    fn take(&self, route: &Option<RouteId>) -> std::result::Result<(), Duration> {
        let now = Instant::now();
        let per_second = ROUTE_BUCKET_CAPACITY / ROUTE_BUCKET_WINDOW.as_secs_f64();
        let mut map = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if map.len() > MAX_TRACKED_ROUTES {
            map.retain(|_, b| now.duration_since(b.last) < ROUTE_BUCKET_FORGET);
        }
        let bucket = map.entry(route.clone()).or_insert(Bucket {
            tokens: ROUTE_BUCKET_CAPACITY,
            last: now,
        });
        let refill = now.duration_since(bucket.last).as_secs_f64() * per_second;
        bucket.tokens = (bucket.tokens + refill).min(ROUTE_BUCKET_CAPACITY);
        bucket.last = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            Err(Duration::from_secs_f64((1.0 - bucket.tokens) / per_second))
        }
    }

    /// Wait for this route's turn, or give up on the request.
    ///
    /// True when a token was spent and the block may be served.
    async fn pace(&self, route: &Option<RouteId>) -> bool {
        let mut waited = Duration::ZERO;
        loop {
            match self.take(route) {
                Ok(()) => return true,
                Err(wait) => {
                    if waited + wait > MAX_SHAPE_WAIT {
                        return false;
                    }
                    tokio::time::sleep(wait).await;
                    waited += wait;
                }
            }
        }
    }
}

use crate::{
    error::CancelError,
    piece_verifier::{PieceStatus, PieceStatusNotifier, PieceVerifier},
    proto::{self, BlockRequest, Decoder},
    record::StableHaveMap,
    types::LocalShareInfo,
    Result, Retry,
};

pub struct Seeder<C: Connection> {
    verifier: PieceVerifier,
    verified_rx: flume::Receiver<PieceStatus>,
    have_map: StableHaveMap,

    inner: Arc<Mutex<SeederInner<C>>>,

    /// DUCAT modification (see ../STIGMERGE-NOTICE.md): a connection handle
    /// for replies, so a reply is not sent while holding the lock that the
    /// next read needs. The clone shares the node; it is a handle, not a
    /// second connection.
    reply_conn: C,

    /// DUCAT modification (see ../STIGMERGE-NOTICE.md): the per-route pace.
    buckets: Arc<RouteBuckets>,
}

impl<C: Connection + Clone + Send + Sync + 'static> Seeder<C> {
    pub async fn new(mut conn: C, share: LocalShareInfo, verifier: PieceVerifier) -> Result<Self> {
        let (status_handler, verified_rx) = PieceStatusNotifier::new();
        verifier.subscribe(Box::new(status_handler)).await;

        let have_map = StableHaveMap::new_local(&mut conn, &share.want_index).await?;

        let reply_conn = conn.clone();
        Ok(Seeder {
            verifier,
            verified_rx,
            have_map,
            inner: Arc::new(Mutex::new(SeederInner::new(conn, share))),
            reply_conn,
            buckets: Arc::new(RouteBuckets::default()),
        })
    }

    #[instrument(skip_all, err)]
    // DUCAT modification (see ../STIGMERGE-NOTICE.md): `retry` is no longer
    // used — the block path does not retry a reply behind a lock any more,
    // it drops what it cannot serve — but the parameter stays so the
    // orchestration in `share.rs` reads the same for every task.
    pub async fn run(mut self, cancel: CancellationToken, _retry: Retry) -> Result<()> {
        let block_request_rx = {
            let inner = self.inner.lock().await;
            let (block_req_handler, block_request_rx) = BlockRequestHandler::new(cancel.clone());
            inner.conn.add_update_handler(Box::new(block_req_handler));
            block_request_rx
        };
        let mut tasks: JoinSet<Result<()>> = JoinSet::new();
        // DUCAT modification (see ../STIGMERGE-NOTICE.md): the ceiling on
        // replies in flight, and therefore on the seeder's memory.
        let inflight = Arc::new(Semaphore::new(MAX_INFLIGHT_REPLIES));
        let mut have_map_sync_interval = interval(Duration::from_secs(30));
        loop {
            select! {
                _ = cancel.cancelled() => {
                    tasks.abort_all();
                    return Err(CancelError.into());
                }
                res = self.verified_rx.recv_async() => {
                    match res {
                        Err(err) => {
                            // TODO: this shouldn't happen
                            warn!(?err, "receive verified piece update");
                            let (status_handler, verified_rx) = PieceStatusNotifier::new();
                            self.verifier.subscribe(Box::new(status_handler)).await;
                            self.verified_rx = verified_rx;
                            warn!("resubscribed to piece verifier");
                            continue;

                        }
                        Ok(piece_state) => {
                            let piece_index = piece_state.piece_index().try_into().unwrap();
                            self.have_map.update_piece(piece_index, true).await?;
                            if piece_state.index_complete() {
                                self.have_map.sync(&mut self.inner.lock().await.conn).await?;
                            }
                        }
                    };
                }
                // DUCAT modification (see ../STIGMERGE-NOTICE.md): finished
                // reply tasks are reaped here. Upstream never joined them,
                // so a long-lived seeder's JoinSet grew with every block it
                // ever served, and a failed read was never noticed.
                Some(res) = tasks.join_next(), if !tasks.is_empty() => {
                    match res {
                        Ok(Ok(())) => {}
                        Ok(Err(err)) if crate::error::is_cancelled(&err) => {}
                        Ok(Err(err)) => warn!(?err, "serving a block"),
                        Err(err) if err.is_cancelled() => {}
                        Err(err) => warn!(?err, "block reply task"),
                    }
                }
                res = block_request_rx.recv_async() => {
                    let (app_call_id, block_req, route) = res?;
                    trace!("app_call: {:?}", block_req);
                    // DUCAT modification (see ../STIGMERGE-NOTICE.md): one
                    // permit per reply in flight, and an excess request is
                    // dropped rather than queued behind a lock. The asker
                    // retries; that is the fetcher's ordinary weather.
                    let Ok(permit) = inflight.clone().try_acquire_owned() else {
                        trace!(piece = block_req.piece, "seeder at capacity, dropping a request");
                        continue;
                    };
                    let have = self.have_map.has_piece(block_req.piece);
                    let inner = self.inner.clone();
                    let mut reply_conn = self.reply_conn.clone();
                    let buckets = self.buckets.clone();
                    let task_cancel = cancel.child_token();
                    tasks.spawn(async move {
                        let _permit = permit;
                        let work = async {
                            // DUCAT modification (see ../STIGMERGE-NOTICE.md):
                            // the route's turn. A fetcher asking faster than
                            // this seeder serves is paced, not starved; one
                            // asking absurdly fast outruns the wait and is
                            // dropped, which costs it a retry and costs this
                            // device nothing.
                            if !buckets.pace(&route).await {
                                trace!(piece = block_req.piece, "over the route's rate, dropping");
                                return Ok(());
                            }
                            // The read happens under the lock — one file
                            // handle cache, one set of seeks. The reply does
                            // not: upstream held the lock across the network
                            // send, so every other request on this share
                            // waited on one peer's round trip.
                            let contents = if have {
                                let mut inner_guard = inner.lock().await;
                                match inner_guard.read_block(&block_req).await {
                                    Ok(buf) => Some(buf),
                                    Err(err) => {
                                        warn!(?err, piece = block_req.piece, "reading a block");
                                        inner_guard.flush_file_cache();
                                        None
                                    }
                                }
                            } else {
                                None
                            };
                            SeederInner::<C>::reply_on(
                                &mut reply_conn,
                                app_call_id,
                                contents.as_deref(),
                            )
                            .await
                        };
                        select! {
                            _ = task_cancel.cancelled() => Err(CancelError.into()),
                            res = work => res,
                        }
                    });
                }
                _ = have_map_sync_interval.tick() => {
                    self.have_map.sync(&mut self.inner.lock().await.conn).await?;
                }
            }
        }
    }
}

struct BlockRequestHandler {
    cancel: CancellationToken,
    block_request_tx: flume::Sender<(OperationId, BlockRequest, Option<RouteId>)>,
}

impl BlockRequestHandler {
    fn new(cancel: CancellationToken) -> (Self, flume::Receiver<(OperationId, BlockRequest, Option<RouteId>)>) {
        // DUCAT modification (see ../STIGMERGE-NOTICE.md): bounded. The
        // queue used to be unbounded, so a peer that asked faster than the
        // disk could answer grew it without limit — on a phone.
        let (block_request_tx, block_request_rx) = flume::bounded(MAX_QUEUED_BLOCK_REQUESTS);
        (
            Self {
                cancel,
                block_request_tx,
            },
            block_request_rx,
        )
    }
}

impl UpdateHandler for BlockRequestHandler {
    fn is_done(&self) -> bool {
        self.block_request_tx.is_disconnected()
    }
    fn app_call(&self, app_call: &VeilidAppCall) {
        match proto::Request::decode(app_call.message()) {
            Ok(proto::Request::BlockRequest(block_req)) => {
                // DUCAT modification (see ../STIGMERGE-NOTICE.md): a full
                // queue is not a broken seeder — the request is dropped and
                // the asker retries. Only a channel with no receiver means
                // this task is gone. The route the call arrived on rides
                // along, because that is all a seeder can meter on: a
                // request over a private route carries no sender, by design.
                match self
                    .block_request_tx
                    .try_send((app_call.id(), block_req, app_call.route_id().cloned()))
                {
                    Ok(()) => {}
                    Err(flume::TrySendError::Full(_)) => {
                        trace!("block request queue full, dropping");
                    }
                    Err(err @ flume::TrySendError::Disconnected(_)) => {
                        error!(?err, "send block request to seeder");
                        self.cancel.cancel();
                    }
                }
            }
            Ok(_) => {}
            Err(err) => {
                warn!(?err, "invalid app_call");
            }
        }
    }
    fn shutdown(&self) {
        trace!("shutdown");
        self.cancel.cancel();
    }
}

struct SeederInner<C: Connection> {
    conn: C,
    share: LocalShareInfo,
    files: HashMap<usize, File>,
}

impl<C: Connection> SeederInner<C> {
    fn new(conn: C, share: LocalShareInfo) -> Self {
        Self {
            conn,
            share,
            files: HashMap::new(),
        }
    }

    fn flush_file_cache(&mut self) {
        self.files.clear();
    }

    /// DUCAT modification (see ../STIGMERGE-NOTICE.md): replying takes a
    /// connection handle rather than `&mut self`, so the reply happens
    /// outside the lock the reads share.
    async fn reply_on(conn: &mut C, call_id: OperationId, contents: Option<&[u8]>) -> Result<()> {
        conn.require_attachment().await?;
        conn.routing_context()
            .api()
            .app_call_reply(call_id, contents.unwrap_or(&[]).to_vec())
            .await?;
        Ok(())
    }

    /// Read one block of one piece, exactly as long as the index says it is.
    ///
    /// DUCAT modification (see ../STIGMERGE-NOTICE.md): bounded, and the
    /// buffer is sized to the block rather than always a whole one. A block
    /// index at or past the piece's block count used to seek merrily past
    /// the end of the file and reply with whatever a short read returned —
    /// the request came off the wire, and the only thing checked about it
    /// was whether we held the piece.
    async fn read_block(&mut self, block_req: &BlockRequest) -> Result<Vec<u8>> {
        // Piece-aligned multi-file (DUCAT): the index says whose piece this
        // is, and the seek is relative to that file's own slice.
        let piece: usize = TryInto::<usize>::try_into(block_req.piece)?;
        let piece_len = self
            .share
            .want_index
            .payload()
            .pieces()
            .get(piece)
            .ok_or_else(|| crate::Error::msg(format!("no piece {piece} in this share")))?
            .length();
        let block_index: usize = block_req.block.into();
        let want = crate::types::expected_block_len(piece_len, block_index).ok_or_else(|| {
            crate::Error::msg(format!(
                "block {block_index} is past the end of piece {piece}"
            ))
        })?;
        let consumed = block_index * BLOCK_SIZE_BYTES;
        let file_index = self.share.want_index.file_index_for_piece(piece);
        let starting_piece = self
            .share
            .want_index
            .files()
            .get(file_index)
            .ok_or_else(|| crate::Error::msg(format!("no file {file_index} in this share")))?
            .contents()
            .starting_piece();
        let offset = piece
            .checked_sub(starting_piece)
            .and_then(|rel| (rel as u64).checked_mul((PIECE_SIZE_BLOCKS * BLOCK_SIZE_BYTES) as u64))
            .and_then(|at| at.checked_add(consumed as u64))
            .ok_or_else(|| crate::Error::msg("block offset outside the file"))?;
        let fh = self.get_file_for_block(file_index).await?;
        fh.seek(std::io::SeekFrom::Start(offset)).await?;
        // Exactly `want` bytes: a short read here would be a short reply,
        // and the fetcher now refuses one — rightly, since we said we held
        // this piece.
        let mut buf = vec![0u8; want];
        fh.read_exact(&mut buf).await?;
        Ok(buf)
    }

    async fn get_file_for_block(&mut self, file_index: usize) -> Result<&mut File> {
        if !self.files.contains_key(&file_index) {
            let file_path = self.share.root.join(
                self.share
                    .want_index
                    .files()
                    .get(file_index)
                    .ok_or_else(|| crate::Error::msg(format!("no file {file_index} in this share")))?
                    .path(),
            );
            let fh = File::open(file_path).await?;
            self.files.insert(file_index, fh);
        }
        self.files
            .get_mut(&file_index)
            .ok_or_else(|| crate::Error::msg("file handle vanished"))
    }
}

// DUCAT modification (see ../STIGMERGE-NOTICE.md): the pace, on its own.
#[cfg(test)]
mod bucket_tests {
    use super::*;

    #[test]
    fn a_route_gets_its_budget_and_then_waits() {
        let buckets = RouteBuckets::default();
        let route = None;

        // The burst is there to be spent.
        for i in 0..(ROUTE_BUCKET_CAPACITY as usize) {
            buckets.take(&route).unwrap_or_else(|_| panic!("token {i}"));
        }

        // And then the asker waits for the next one, rather than being told
        // no: a fetcher that asks faster than this seeder serves is paced.
        let wait = buckets.take(&route).expect_err("the budget is spent");
        assert!(wait > Duration::ZERO, "a wait of {wait:?}");
        // One token at 64 per 10 s is about 156 ms; never more than the
        // whole window.
        assert!(wait <= ROUTE_BUCKET_WINDOW, "a wait of {wait:?}");
    }

    #[test]
    fn the_budget_is_per_route() {
        let buckets = RouteBuckets::default();
        let route = |b: u8| {
            Some(RouteId::new(
                veilid_core::CRYPTO_KIND_VLD0,
                veilid_core::BareRouteId::new(&[b; 32]),
            ))
        };
        let (a, b) = (route(1), route(2));
        for _ in 0..(ROUTE_BUCKET_CAPACITY as usize) {
            buckets.take(&a).expect("a's budget");
        }
        buckets.take(&a).expect_err("a is spent");
        // b has not asked for anything, and does not pay for a.
        buckets.take(&b).expect("b's own budget");
    }
}

#[cfg(test)]
#[cfg(feature = "refactor")]
mod tests {
    use std::{
        path::PathBuf,
        str::FromStr,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use stigmerge_fileindex::Index;
    use tokio::{
        runtime::{Builder, RngSeed},
        sync::mpsc,
        time,
    };
    use tokio_util::sync::CancellationToken;
    use veilid_core::{OperationId, RecordKey, VeilidAppCall};

    use crate::{
        actor::{OneShot, Operator, ResponseChannel},
        proto::{BlockRequest, Encoder, Header},
        seeder,
        tests::{temp_file, StubNode},
    };

    use super::*;

    fn test_header() -> Header {
        Header::new([0xab; 32], 42, 1, [0xcd; 99].as_slice(), None, None)
    }

    #[test]
    fn test_seeder_block_request_for_verified_piece() {
        let seed = RngSeed::from_bytes(b"test");

        let rt = Builder::new_current_thread()
            .enable_time()
            .rng_seed(seed) // Apply the seed for deterministic polling order
            .build_local(Default::default())
            .unwrap();

        rt.block_on(async {
            // Create a test file with known content
            const BLOCK_DATA: u8 = 0xa5;
            let tf = temp_file(BLOCK_DATA, BLOCK_SIZE_BYTES * PIECE_SIZE_BLOCKS * 2); // 2 pieces
            let tf_path = tf.path().to_path_buf();
            let root_dir = tf_path.parent().unwrap().to_path_buf();

            // Create a mock index
            let index = create_test_index(&tf_path).await;

            // Set up channels
            let (verified_tx, verified_rx) = broadcast::channel(16);

            // Create a stub peer with mock reply_block_contents
            let mut node = StubNode::new();
            let update_tx = node.update_tx.clone();
            let reply_contents_called = Arc::new(Mutex::new(false));
            let reply_contents_data = Arc::new(Mutex::new(Vec::new()));
            let reply_contents_called_clone = reply_contents_called.clone();
            let reply_contents_data_clone = reply_contents_data.clone();

            let (replied_tx, mut replied_rx) = mpsc::channel(1);

            node.reply_block_contents_result = Arc::new(Mutex::new(
                move |_call_id: OperationId, contents: Option<&[u8]>| {
                    *reply_contents_called_clone.lock().unwrap() = true;
                    *reply_contents_data_clone.lock().unwrap() = match contents {
                        Some(contents) => contents.to_vec(),
                        None => vec![],
                    };
                    replied_tx.try_send(()).expect("replied");
                    Ok(())
                },
            ));
            node.known_peers_result = Arc::new(Mutex::new(move |_index_digest: &[u8]| Ok(vec![])));

            let fake_key = RecordKey::from_str("VLD0:cCHB85pEaV4bvRfywxnd2fRNBScR64UaJC8hoKzyr3M")
                .expect("key");

            // Create share info
            let share_info = LocalShareInfo {
                key: fake_key,
                header: test_header(),
                want_index: index,
                want_index_digest: [0u8; 32],
                root: root_dir,
                have_map: PieceMap::new(),
            };

            // Create cancellation token
            let cancel = CancellationToken::new();
            let mut operator = Operator::new(
                cancel.clone(),
                Seeder::new(node, share_info, verified_rx),
                OneShot,
            );

            // First, send a verified piece notification with confirmation it's applied
            let piece_state = PieceState::new(0, 0, 0, PIECE_SIZE_BLOCKS, PIECE_SIZE_BLOCKS - 1);
            verified_tx.send(piece_state).expect("send verified piece");

            // Have map should be updated. Retry with a backoff delay to allow
            // select! to pick up on the request.
            let mut confirmed_have_map = false;
            let req = Request::HaveMap {
                response_tx: ResponseChannel::default(),
            };
            let resp = operator.call(req).await.expect("call havemap");
            match resp {
                Response::HaveMap(have_map) => {
                    if !have_map.is_empty() {
                        confirmed_have_map = true;
                    }
                }
            }
            assert!(confirmed_have_map, "confirm verified block");

            // Send a block request for the verified piece
            let block_req = BlockRequest { piece: 0, block: 0 };
            let req = proto::Request::BlockRequest(block_req);
            let encoded_req = req.encode().expect("encode request");
            let app_call = VeilidAppCall::new(None, None, encoded_req, 42u64.into());

            update_tx
                .send(VeilidUpdate::AppCall(Box::new(app_call.clone())))
                .expect("send app call");
            time::timeout(Duration::from_secs(10), replied_rx.recv())
                .await
                .expect("confirm app_call");

            // Cancel the seeder
            cancel.cancel();
            operator.join().await.expect_err("cancelled");

            // Verify reply_block_contents was called
            assert!(
                *reply_contents_called.lock().unwrap(),
                "reply_block_contents should be called"
            );

            // Verify that the data returned matches what we expect
            let reply_data = reply_contents_data.lock().unwrap();
            assert_eq!(
                reply_data.len(),
                BLOCK_SIZE_BYTES,
                "should return full block"
            );
            assert!(
                reply_data.iter().all(|&b| b == BLOCK_DATA),
                "all bytes should match the pattern"
            );
        });
    }

    #[tokio::test]
    async fn test_seeder_block_request_for_unverified_piece() {
        // Create a test file with known content
        const BLOCK_DATA: u8 = 0xa5;
        let tf = temp_file(BLOCK_DATA, BLOCK_SIZE_BYTES * PIECE_SIZE_BLOCKS * 2); // 2 pieces
        let tf_path = tf.path().to_path_buf();
        let root_dir = tf_path.parent().unwrap().to_path_buf();

        // Create a mock index
        let index = create_test_index(&tf_path).await;

        // Set up channels
        let (_verified_tx, verified_rx) = broadcast::channel(16);

        // Create a stub peer with mock reply_block_contents
        let mut node = StubNode::new();
        let update_tx = node.update_tx.clone();
        let _update_rx = update_tx.subscribe();

        let reply_contents_called = Arc::new(Mutex::new(false));
        let reply_contents_data = Arc::new(Mutex::new(Vec::new()));
        let reply_contents_called_clone = reply_contents_called.clone();
        let reply_contents_data_clone = reply_contents_data.clone();

        let (replied_tx, mut replied_rx) = mpsc::channel(1);

        node.reply_block_contents_result = Arc::new(Mutex::new(
            move |_call_id: OperationId, contents: Option<&[u8]>| {
                *reply_contents_called_clone.lock().unwrap() = true;
                *reply_contents_data_clone.lock().unwrap() = match contents {
                    Some(contents) => contents.to_vec(),
                    None => vec![],
                };
                replied_tx.try_send(()).expect("replied");
                Ok(())
            },
        ));
        node.known_peers_result = Arc::new(Mutex::new(move |_index_digest: &[u8]| Ok(vec![])));

        let fake_key =
            RecordKey::from_str("VLD0:cCHB85pEaV4bvRfywxnd2fRNBScR64UaJC8hoKzyr3M").expect("key");

        // Create share info
        let share_info = LocalShareInfo {
            key: fake_key,
            header: test_header(),
            want_index: index,
            want_index_digest: [0u8; 32],
            root: root_dir,
            have_map: PieceMap::new(),
        };

        // Create clients
        let cancel = CancellationToken::new();

        // Create seeder
        let mut operator = Operator::new(
            cancel.clone(),
            Seeder::new(node, share_info, verified_rx),
            OneShot,
        );

        // Send a block request for an unverified piece
        let block_req = BlockRequest { piece: 0, block: 0 };
        let req = proto::Request::BlockRequest(block_req);
        let encoded_req = req.encode().expect("encode request");

        let app_call = VeilidAppCall::new(None, None, encoded_req, 42u64.into());

        // Making a synchronous call to the seeder ensures we're in the event
        // loop and reacting to app_calls.
        operator
            .call(seeder::Request::HaveMap {
                response_tx: ResponseChannel::default(),
            })
            .await
            .expect("call");

        update_tx
            .send(VeilidUpdate::AppCall(Box::new(app_call.clone())))
            .expect("send app call");
        time::timeout(Duration::from_secs(10), replied_rx.recv())
            .await
            .expect("confirm app_call");

        // Cancel the seeder
        cancel.cancel();

        // Verify seeder exits cleanly
        operator.join().await.expect_err("cancelled");

        // Verify reply_block_contents was called
        assert!(
            *reply_contents_called.lock().unwrap(),
            "reply_block_contents should be called"
        );

        // Verify empty data was returned for unverified piece
        let reply_data = reply_contents_data.lock().unwrap();
        assert_eq!(
            reply_data.len(),
            0,
            "should return empty data for unverified piece"
        );
    }

    // Helper function to create a test index
    async fn create_test_index(file_path: &PathBuf) -> Index {
        // Create index from the file using Indexer
        let indexer = stigmerge_fileindex::Indexer::from_file(file_path.as_path())
            .await
            .expect("create indexer");
        indexer.index().await.expect("create index")
    }
}
