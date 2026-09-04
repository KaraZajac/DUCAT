package org.ducatproject.ducat

/**
 * The swarm's two verbs, for clients (post-1.0 1.3; engine and proof in
 * mobile/vendor — vendored from cmars's stigmerge, BLAKE3 pieces, riding
 * the same node the mailbox runs).
 *
 * The contract mirrors §16.20's manifest rule: a share is named by its
 * key AND its index digest, and the two travel together on the thread —
 * a key without its digest bootstraps into whatever answers, which is
 * not a fetch, it is an ask.
 *
 * [fetch] blocks for the duration — minutes for a heavy month — so it is
 * called on IO like the attachment chunk reads, with [fetchProgress]
 * polled from the screen the way wallet sync is.
 */
object Swarm {
    data class Share(val shareKey: String, val indexDigestHex: String)
    data class Progress(
        val position: Long,
        val length: Long,
        val done: Boolean,
        /** Pieces verified, and how many the index says there are. Known
         *  before the first byte lands, which is what lets a screen show
         *  the shape of a transfer rather than a byte count that may never
         *  move. */
        val piecesDone: Long = 0,
        val piecesTotal: Long = 0,
    )

    /** Index and announce; returns once every local piece is verified and
     *  the share is on the DHT. Serving continues until [stop]. */
    fun seed(path: String): Share {
        val s = uniffi.ducat_mobile.swarmSeed(path)
        return Share(s.shareKey, s.indexDigestHex)
    }

    /** Stop serving everything. A fetcher mid-download keeps any other
     *  peer it met — every peer is a seeder, which is the shape's whole
     *  point. */
    fun stop() = uniffi.ducat_mobile.swarmStop()

    /** Stop serving one share, leaving the rest up. */
    fun stopShare(shareKey: String) = uniffi.ducat_mobile.swarmStopShare(shareKey)

    /** Fetch into [rootDir], blocking until every piece verified against
     *  the promised digest. Returns the byte count. With [staySeeding] the
     *  share keeps serving afterwards — the reader becomes a mirror — and
     *  a fetch over already-complete files verifies, downloads nothing,
     *  and stays: that is how a restart re-seeds. */
    fun fetch(
        shareKey: String,
        indexDigestHex: String,
        rootDir: String,
        staySeeding: Boolean = false,
    ): Long =
        uniffi.ducat_mobile.swarmFetch(shareKey, indexDigestHex, rootDir, staySeeding).toLong()

    /** This share's fetch progress. Keyed: fetches run concurrently now. */
    fun fetchProgress(shareKey: String): Progress {
        val p = uniffi.ducat_mobile.swarmFetchProgress(shareKey)
        return Progress(
            p.position, p.length.toLong(), p.done,
            p.piecesDone.toLong(), p.piecesTotal.toLong(),
        )
    }
}
