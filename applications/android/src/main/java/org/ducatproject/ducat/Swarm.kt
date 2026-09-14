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
        /** One bit per piece, little-endian within each byte: which have
         *  verified. Empty until the fetcher has an index to count. */
        val pieces: ByteArray = ByteArray(0),
    ) {
        /** Is piece [i] on this device yet? */
        fun has(i: Int): Boolean {
            val byte = i / 8
            if (byte < 0 || byte >= pieces.size) return false
            return (pieces[byte].toInt() shr (i % 8)) and 1 == 1
        }

        // ByteArray in a data class: equals/hashCode by identity would make
        // every poll look like a change and redraw the whole bar.
        override fun equals(other: Any?): Boolean =
            other is Progress && position == other.position && length == other.length &&
                done == other.done && piecesDone == other.piecesDone &&
                piecesTotal == other.piecesTotal && pieces.contentEquals(other.pieces)

        override fun hashCode(): Int =
            ((position * 31 + length) * 31 + piecesDone).toInt() * 31 + pieces.contentHashCode()
    }

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

    /**
     * Per-kind fetch ceilings, in bytes (research/security phase 1: N4/D2).
     *
     * A share's index says how big it is and the fetcher used to believe
     * it: every file it named was created and sized before a byte was
     * verified, so a hearted home fetched on the lap — with nobody watching
     * — could be any size its publisher liked. The engine now refuses an
     * index that declares more than the caller will take, before anything
     * is created, so every caller says what kind of thing it asked for.
     *
     * The same table lives in `mobile/src/swarm.rs` (`swarm::caps`) for the
     * desk; keep them together.
     */
    object Caps {
        /** A listing's pictures (§16.18.3). Photographs, thumbnailed. */
        const val GALLERY = 64L * 1024 * 1024
        /** Somebody's home page and feed, fetched unattended when hearted. */
        const val HOME = 256L * 1024 * 1024
        /** A kept site's bundle, also fetched unattended. */
        const val SITE = 256L * 1024 * 1024
        /** One issue of a publication. */
        const val ISSUE = 256L * 1024 * 1024
        /** A release whose entry does not name its own size. A release is
         *  the one thing here that is legitimately huge, and its address
         *  carries no length until somebody has fetched it once. */
        const val RELEASE = 4L * 1024 * 1024 * 1024
        /** Slack over a sealed attachment's declared length: the AEAD tag
         *  and whatever the blob is wrapped in. The room check in Mailbox
         *  does the real work. */
        const val ATTACHMENT_SLACK = 1L * 1024 * 1024
        /** What a caller that names no kind gets. Not a budget — a ceiling,
         *  so the "any size at all" shape cannot come back through a caller
         *  nobody updated. */
        const val DEFAULT = 4L * 1024 * 1024 * 1024
    }

    /** Fetch into [rootDir], blocking until every piece verified against
     *  the promised digest. Returns the byte count. With [staySeeding] the
     *  share keeps serving afterwards — the reader becomes a mirror — and
     *  a fetch over already-complete files verifies, downloads nothing,
     *  and stays: that is how a restart re-seeds.
     *
     *  [maxBytes] is the most this fetch will take: a share whose index
     *  declares more is refused before a file is created, with
     *  `SwarmException.TooLarge` naming both figures. Pass the [Caps] entry
     *  for the kind of thing being fetched. */
    fun fetch(
        shareKey: String,
        indexDigestHex: String,
        rootDir: String,
        staySeeding: Boolean = false,
        maxBytes: Long = Caps.DEFAULT,
    ): Long =
        uniffi.ducat_mobile.swarmFetchCapped(
            shareKey, indexDigestHex, rootDir, staySeeding, maxBytes.toULong(),
        ).toLong()

    /** This share's fetch progress. Keyed: fetches run concurrently now. */
    fun fetchProgress(shareKey: String): Progress {
        val p = uniffi.ducat_mobile.swarmFetchProgress(shareKey)
        return Progress(
            p.position, p.length.toLong(), p.done,
            p.piecesDone.toLong(), p.piecesTotal.toLong(),
            p.pieces,
        )
    }
}
