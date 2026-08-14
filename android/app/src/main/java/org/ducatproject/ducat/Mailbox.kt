package org.ducatproject.ducat

import android.content.Context
import uniffi.ducat_mobile.*

private const val TAG = "DucatMailbox"

/** A card just issued: what to show, and which claim is its answer. */
data class IssuedHandle(val uri: String, val inboxKey: String)

/** A head plus seven message slots. Small deliberately: §16.11 wants a message
 *  to stop being readable rather than accumulate, and a ring that wraps in
 *  ordinary use is a ring whose wrap gets exercised. */
const val LOG_SUBKEYS: UInt = 8u

/** New logs get a bigger ring: reactions and receipts multiply message count,
 *  and eight slots of history was sized for text alone (§16.12). */
const val NEW_RING: UInt = 32u
private const val ONE_TIME_KEYS: UInt = 32u

/**
 * Everything that touches DHT records (§16.12).
 *
 * The transport used to be `app_call` against a live route, which is a remote
 * procedure call being used as a mailbox. A record key is permanent and a
 * record outlives the process that wrote it, so a card no longer dies when the
 * app restarts and neither side has to be present for the other.
 *
 * Every call here blocks on the network. Callers must be off the main thread.
 */
object Mailbox {

    /** How long an unreadable sequence gets before it is declared lost —
     *  covering slow ring-slot propagation, which force-refresh does not. */
    private const val STUCK_PATIENCE_MS = 10L * 60 * 1000
    private val stuckSince = java.util.concurrent.ConcurrentHashMap<String, Long>()

    /**
     * Mint a card: an inbox for the handshake, an outbox for what we will say.
     *
     * Both records are created before the card is signed, because the card
     * names the inbox and the inbox's first subkey names the outbox — a card
     * pointing at a record that does not exist yet is a card that fails after
     * someone has already accepted it.
     */
    fun issueCard(
        context: Context,
        displayName: String?,
        validSecs: ULong,
        /** "profile" for the standing code, "sale" for a till/tab/ride handshake. */
        purpose: String = "profile",
    ): IssuedHandle {
        val store = ContactStore(context)
        val persona = PersonaStore(context).secret()

        val writer = generateWriterKeys()
        val inbox = nodeDhtCreateShared(writer.public)
        val outbox = createLog(context)

        // Fresh one-time ids from the device-wide counter, and the signed
        // prekey *reused*: rotating it as a side effect of making a card was
        // how messages sealed to the old one — cached in every peer's copy of
        // our bundle — arrived unreadable.
        val prekeys = generatePrekeys(
            ONE_TIME_KEYS, 60uL * 60uL * 24uL * 30uL,
            store.nextPrekeyStart(ONE_TIME_KEYS.toInt()).toUInt(),
            store.signedPrekeySecret(),
        )
        store.savePrekeys(
            prekeys.bundle,
            prekeys.signedSecret,
            prekeys.oneTimeIds.mapIndexed { i, id -> id.toInt() to prekeys.oneTimeSecrets[i] }.toMap(),
        )

        // Subkey 0: who we are and where to leave things. Written now, so a
        // claimant reading it later needs nothing from us but the record.
        nodeDhtSet(
            inbox.key, 0u,
            buildContactDetails(
                persona, outbox.key, prekeys.bundle, displayName,
                // Only if the user has opted in. §16.12 makes this a choice,
                // and defaulting it on would be choosing for them.
                if (store.publishAddress()) WalletStore(context).address() else null,
                // §16.9: the profile rides the record, never the card. A card
                // carrying a picture is a QR code nobody can scan.
                MyProfile(context).toWire(),
            ),
        )

        val card = createContactCard(
            persona, inbox.key, writer.public, displayName, writer.secret, validSecs,
        )
        store.saveIssuedCard(
            inbox.key, writer.public, writer.secret,
            outbox.key, outbox.ownerPublic, outbox.ownerSecret,
            card.uri, purpose,
        )
        DucatLog.i(TAG, "issued card: inbox=${inbox.key.take(24)}… outbox=${outbox.key.take(24)}…")
        // The inbox key rides along because it is the card's identity: a flow
        // that shows this code must wait for *this card's* claimant, not for
        // whichever contact appears next.
        return IssuedHandle(card.uri, inbox.key)
    }

    /** A fresh append-only log with its head initialised. */
    private fun createLog(context: Context): DhtRecord {
        // The record must be as big as the ring its head advertises. It was
        // minted with 8 subkeys while the head claimed 32 — and sequences 0–6
        // map identically under both, so every thread worked flawlessly until
        // its eighth message computed slot 8 in an 8-subkey record and the
        // send died. Found by the first conversation to reach it.
        val rec = nodeDhtCreate(NEW_RING)
        // The bundle rides the head from birth, not from the first message.
        // A head without keys strands a counterparty who claimed the card and
        // wants to speak first — observed live: a hail's driver claimed,
        // tried to quote, and found "they have not written since claiming".
        val bundle = topUpIfLow(ContactStore(context))
        nodeDhtSet(rec.key, 0u, buildLogHead(0uL, bundle, null, NEW_RING))
        return rec
    }

    /**
     * Accept someone's card: read their details, publish ours in the reply
     * subkey, and keep both.
     */
    fun claimCard(context: Context, scanned: ScannedCard, petname: String?): Contact {
        val store = ContactStore(context)
        val persona = PersonaStore(context).secret()

        nodeDhtOpen(scanned.inboxKey, scanned.writerPublic, scanned.writerSecret)

        // Single use, checked by reading rather than trusting a local flag. The
        // inbox has exactly one reply subkey, so a card already answered has
        // nowhere left to write and this is the only way to find out.
        val already = nodeDhtGet(scanned.inboxKey, 1u, true)
        if (already != null && already.isNotEmpty()) {
            throw IllegalStateException("That card has already been used. Ask them for a new one.")
        }

        val raw = nodeDhtGet(scanned.inboxKey, 0u, true)
            ?: throw IllegalStateException("Their details are not published yet — ask them to open DUCAT.")
        val theirs = parseContactDetails(raw)

        val outbox = createLog(context)
        val prekeys = generatePrekeys(
            ONE_TIME_KEYS, 60uL * 60uL * 24uL * 30uL,
            store.nextPrekeyStart(ONE_TIME_KEYS.toInt()).toUInt(),
            store.signedPrekeySecret(),
        )
        store.savePrekeys(
            prekeys.bundle,
            prekeys.signedSecret,
            prekeys.oneTimeIds.mapIndexed { i, id -> id.toInt() to prekeys.oneTimeSecrets[i] }.toMap(),
        )
        nodeDhtSet(
            scanned.inboxKey, 1u,
            buildContactDetails(
                persona, outbox.key, prekeys.bundle, petname,
                if (store.publishAddress()) WalletStore(context).address() else null,
                MyProfile(context).toWire(),
            ),
        )

        val c = Contact(
            personaHex = theirs.persona.toHexString(),
            petname = petname,
            assertedName = theirs.assertedName,
            myOutbox = outbox.key,
            myOutboxOwnerPublic = outbox.ownerPublic,
            myOutboxOwnerSecret = outbox.ownerSecret,
            theirOutbox = theirs.outboxKey,
            theirBundle = theirs.prekeyBundle,
            theirAddress = theirs.payto,
            avatar = theirs.profile.avatar,
            email = theirs.profile.email,
            phone = theirs.profile.phone,
            signal = theirs.profile.signal,
            pronouns = theirs.profile.pronouns?.toInt(),
            carModel = theirs.profile.carModel,
            carColor = theirs.profile.carColor,
            plate = theirs.profile.plate,
            myRing = NEW_RING.toInt(),
        )
        store.add(c)
        DucatLog.i(TAG, "claimed: their outbox=${theirs.outboxKey.take(24)}…")
        return c
    }

    /**
     * The issuer's other half: has anyone answered our card?
     *
     * Called on a poll rather than pushed, because the answer may have been
     * written while this device was off.
     */
    fun collectClaims(context: Context): Int {
        val store = ContactStore(context)
        var collected = 0
        // Every outstanding card, not "the" card. A claim answers a specific
        // card, and the registry is what lets a till's handshake and the
        // profile code be outstanding at once without either stealing the
        // other's claimant.
        for (issued in store.issuedCards().filter { it.answeredBy == null }) {
            try {
                nodeDhtOpen(issued.inboxKey, null, null)
                val raw = nodeDhtGet(issued.inboxKey, 1u, true) ?: continue
                if (raw.isEmpty()) continue
                val theirs = parseContactDetails(raw)
                val personaHex = theirs.persona.toHexString()
                store.add(
                    Contact(
                        personaHex = personaHex,
                        petname = null,
                        assertedName = theirs.assertedName,
                        myOutbox = issued.outboxKey,
                        myOutboxOwnerPublic = issued.outboxOwnerPublic,
                        myOutboxOwnerSecret = issued.outboxOwnerSecret,
                        theirOutbox = theirs.outboxKey,
                        theirBundle = theirs.prekeyBundle,
                        theirAddress = theirs.payto,
                        avatar = theirs.profile.avatar,
                        email = theirs.profile.email,
                        phone = theirs.profile.phone,
                        signal = theirs.profile.signal,
                        pronouns = theirs.profile.pronouns?.toInt(),
                        carModel = theirs.profile.carModel,
                        carColor = theirs.profile.carColor,
                        plate = theirs.profile.plate,
                        myRing = NEW_RING.toInt(),
                    )
                )
                store.markCardAnswered(issued.inboxKey, personaHex)
                collected++
                DucatLog.i(TAG, "card (${issued.purpose}) answered by ${theirs.assertedName}")
                // Only the standing profile code replaces itself — a sale's
                // handshake was for that sale, and pre-issuing another would
                // mint records nobody will ever scan.
                if (issued.purpose == "profile") {
                    runCatching { issueCard(context, NameStore(context).get(), 60uL * 60uL * 24uL) }
                        .onSuccess { DucatLog.i(TAG, "a fresh profile code is ready") }
                        .onFailure { DucatLog.w(TAG, "could not pre-issue: ${it.message}") }
                }
            } catch (e: Exception) {
                if (isOffline(e)) {
                    // One line, not one per card: offline fails them all alike.
                    DucatLog.i(TAG, "offline — claims wait for the network")
                    break
                }
                DucatLog.w(TAG, "collectClaims(${issued.inboxKey.take(16)}…): ${e.message}")
            }
        }
        return collected
    }

    /**
     * Append one message to our outbox for this contact.
     *
     * The slot is written **before** the head. A reader that saw `next_seq`
     * move and then found an unwritten slot would have been told a message
     * exists that does not; this order only ever makes one briefly late.
     */
    fun send(
        context: Context,
        c: Contact,
        body: String,
        minePersonaHex: String,
        kind: Int = 0,
        amountPxmr: Long? = null,
        payto: String? = null,
        /** The transaction a payment notice points at (§16.13). */
        txidHex: String? = null,
        /** What the money is for. Must add up to [amountPxmr] plus [taxPxmr];
         *  core refuses the message if it does not, so this cannot go out
         *  disagreeing with the total it sits beside. */
        items: List<BillItem> = emptyList(),
        taxPxmr: Long? = null,
        /** §16.14: the message this reaction is about. */
        reSeq: Long? = null,
        reOwn: Boolean = false,
        /** §16.15: a sealed blob parked in its own record. */
        attachment: uniffi.ducat_mobile.AttachmentRef? = null,
    ): Contact {
        val store = ContactStore(context)
        val bundle = c.theirBundle
            ?: throw IllegalStateException("No keys for this contact yet.")
        DucatLog.i(TAG, "sending ${if (kind == 0) "message" else "payment note"} " +
            "seq ${c.outSeq} to ${c.displayName()}")
        val sealed = sealMessage(
            bundle, c.outSeq.toULong(), c.outPrevLink ?: ByteArray(32), body,
            threadAad(minePersonaHex, c.personaHex),
            kind.toUByte(), amountPxmr?.toULong(),
            txidHex?.let { hexToBytes(it) }, payto,
            items.map { uniffi.ducat_mobile.BillLine(it.description, it.amountPxmr.toULong()) },
            taxPxmr?.toULong(),
            reSeq?.toULong(), reOwn, attachment,
        )
        // Re-opened **as the owner**. Creating a record leaves it writable only
        // for that process; a plain re-open is read-only and the write comes
        // back "value is not writable", which sounds like the network refusing
        // and is us having discarded the key.
        if (c.myOutboxOwnerSecret.isEmpty()) {
            throw IllegalStateException(
                "This conversation predates the current format. Ask them for a new card."
            )
        }
        nodeDhtOpen(c.myOutbox, c.myOutboxOwnerPublic, c.myOutboxOwnerSecret)
        // Threads minted before the 8-subkey/32-ring mismatch was fixed hold
        // records smaller than the ring their head claims. Slots 0–6 map
        // identically under ring 8 and ring 32, so clamping the ring back to
        // the record's real size heals the thread with no history rewritten —
        // the head republishes the honest ring below and readers follow it.
        var ring = c.myRing.toUInt()
        try {
            nodeDhtSet(c.myOutbox, logSubkey(c.outSeq.toULong(), ring), sealed.bytes)
        } catch (e: Exception) {
            if (e.message?.contains("out of range") == true && ring > LOG_SUBKEYS) {
                DucatLog.w(
                    TAG,
                    "legacy log smaller than its ring — clamping ${c.displayName()} to $LOG_SUBKEYS",
                )
                ring = LOG_SUBKEYS
                store.setMyRing(c.personaHex, LOG_SUBKEYS.toInt())
                nodeDhtSet(c.myOutbox, logSubkey(c.outSeq.toULong(), ring), sealed.bytes)
            } else {
                throw e
            }
        }
        // Republish our keys with every head write. Cheap — the head is read on
        // every poll anyway — and it is the only route back from an exhausted
        // supply, since the handshake inbox is a one-time artifact.
        nodeDhtSet(
            c.myOutbox, 0u,
            buildLogHead(
                (c.outSeq + 1).toULong(),
                topUpIfLow(store),
                // §16.16: the watermark rides every head write for free, and
                // only when the user opted in. c.inSeq is "I have accepted
                // your messages below this", which is exactly the claim.
                if (store.readReceipts()) c.inSeq.toULong() else null,
                ring.takeIf { it != 8u },
            ),
        )

        store.append(
            c.personaHex,
            StoredMessage(
                outgoing = true, seq = c.outSeq, body = body,
                timestamp = System.currentTimeMillis() / 1000,
                forwardSecret = sealed.forwardSecret,
                kind = kind, amountPxmr = amountPxmr ?: 0L, payto = payto,
                txidHex = txidHex, items = items, taxPxmr = taxPxmr,
                reSeq = reSeq, reOwn = reOwn,
                attRecord = attachment?.recordKey, attKey = attachment?.key,
                attNonce = attachment?.nonce, attLen = attachment?.len?.toLong() ?: 0L,
                attHash = attachment?.ctHash?.toHexString(),
                attMime = attachment?.mime, attName = attachment?.name,
            ),
        )
        store.advanceOutbound(c.personaHex, c.outSeq + 1, sealed.nextLink)

        // Withdraw the key we just used from our *cached copy* of their bundle.
        // select() takes the first one-time entry, so without this every message
        // seals to the same key — the first is accepted, the receiver burns it,
        // and every later one comes back as an unknown prekey. Exactly the bug
        // that hit the published bundle earlier, on the other side of the wire.
        if (sealed.prekeyId != 0u) {
            runCatching { prunePrekey(bundle, sealed.prekeyId) }
                .onSuccess { store.setTheirBundle(c.personaHex, it) }
        }
        DucatLog.i(TAG, "delivered seq ${c.outSeq} to ${c.displayName()}" +
            if (sealed.forwardSecret) "" else " (no forward secrecy — their one-time keys ran out)")
        return store.all().first { it.personaHex == c.personaHex }
    }

    /** Where a fetched attachment lives, named by its ciphertext hash. */
    fun attachmentFile(context: Context, ctHashHex: String): java.io.File =
        java.io.File(java.io.File(context.filesDir, "att").apply { mkdirs() }, ctHashHex)

    /**
     * Fetch one missing attachment, if any (§16.15).
     *
     * Bounded to one per call because each chunk is a DHT round trip and this
     * runs on the poll loop. Hash first, decrypt second: bytes from the
     * network never reach the AEAD without matching the hash the sealed
     * message promised — and the file is cached under that hash, so a
     * re-delivered message finds its picture already on disk.
     */
    fun fetchOneAttachment(context: Context): Boolean {
        val store = ContactStore(context)
        for (c in store.all()) {
            for (m in store.thread(c.personaHex)) {
                val hash = m.attHash ?: continue
                val rec = m.attRecord ?: continue
                val key = m.attKey ?: continue
                val nonce = m.attNonce ?: continue
                val out = attachmentFile(context, hash)
                if (out.exists()) continue
                return runCatching {
                    nodeDhtOpen(rec, null, null)
                    val ctLen = m.attLen + 16 // AEAD tag
                    val chunks = ((ctLen + 32_767) / 32_768).toInt()
                    val buf = java.io.ByteArrayOutputStream()
                    for (i in 0 until chunks) {
                        val part = nodeDhtGet(rec, i.toUInt(), true)
                            ?: throw IllegalStateException("chunk $i missing")
                        buf.write(part)
                    }
                    val ct = buf.toByteArray()
                    val digest = java.security.MessageDigest.getInstance("SHA-256").digest(ct)
                    if (digest.toHexString() != hash) {
                        throw IllegalStateException("ciphertext hash mismatch")
                    }
                    val plain = attachmentOpen(key, nonce, ct)
                    out.writeBytes(plain)
                    // Stewardship (§18.7): the bytes are ours now; stop being
                    // an origin for the record that carried them.
                    runCatching { nodeDhtDelete(rec) }
                    DucatLog.i(TAG, "fetched attachment ${hash.take(12)}… (${plain.size} bytes)")
                    ContactStore.bump()
                    true
                }.getOrElse {
                    DucatLog.w(TAG, "attachment ${hash.take(12)}…: ${it.message}")
                    false
                }
            }
        }
        return false
    }

    /**
     * Publish the read watermark without sending anything (§16.16).
     *
     * A head write with the sequence unchanged: no slot, no prekey, no chain
     * entry — the cheapest possible "I have seen it", and only when the user
     * turned receipts on.
     */
    fun markRead(context: Context, c: Contact) {
        val store = ContactStore(context)
        if (!store.readReceipts()) return
        if (c.myOutboxOwnerSecret.isEmpty()) return
        runCatching {
            nodeDhtOpen(c.myOutbox, c.myOutboxOwnerPublic, c.myOutboxOwnerSecret)
            nodeDhtSet(
                c.myOutbox, 0u,
                buildLogHead(
                    c.outSeq.toULong(),
                    store.prekeyBundle(),
                    c.inSeq.toULong(),
                    c.myRing.toUInt().takeIf { it != 8u },
                ),
            )
        }.onFailure { DucatLog.w(TAG, "markRead: ${it.message}") }
    }

    /**
     * Our current bundle, regenerated first if the one-time supply has run low.
     *
     * Six is a threshold rather than zero: hitting zero means the *next* sender
     * is already on the signed prekey, and this only refreshes when we happen
     * to write a head. Leaving headroom is what keeps forward secrecy the
     * normal case rather than the lucky one.
     */
    private fun topUpIfLow(store: ContactStore): ByteArray? {
        if (store.oneTimeRemaining() > 6) return store.prekeyBundle()
        val m = generatePrekeys(
            ONE_TIME_KEYS, 60uL * 60uL * 24uL * 30uL,
            store.nextPrekeyStart(ONE_TIME_KEYS.toInt()).toUInt(),
            store.signedPrekeySecret(),
        )
        store.savePrekeys(
            m.bundle, m.signedSecret,
            m.oneTimeIds.mapIndexed { i, id -> id.toInt() to m.oneTimeSecrets[i] }.toMap(),
        )
        DucatLog.i(TAG, "topped up one-time keys")
        return m.bundle
    }

    /** Read everything new from every contact's outbox. Returns how many landed. */
    fun poll(context: Context): Int {
        val store = ContactStore(context)
        // Each poll is also the clock for the forward-secrecy delete: burned
        // one-time secrets past their grace window leave for good here.
        store.sweepBurnedPrekeys()
        val mine = PersonaStore(context).personaHex()
        var got = 0
        for (c in store.all()) {
            got += try {
                pollOne(context, store, c, mine)
            } catch (e: Exception) {
                if (isOffline(e)) {
                    // Offline fails every contact identically; one line says it.
                    DucatLog.i(TAG, "offline — messages wait for the network")
                    break
                }
                DucatLog.w(TAG, "poll ${c.displayName()}: ${e.message}")
                0
            }
        }
        return got
    }

    /** Veilid's TryAgain surfacing through the bridge as message text. */
    private fun isOffline(e: Exception) =
        e.message?.contains("TryAgain", ignoreCase = true) == true

    private fun pollOne(context: Context, store: ContactStore, c: Contact, minePersonaHex: String): Int {
        nodeDhtOpen(c.theirOutbox, null, null)
        val headRaw = nodeDhtGet(c.theirOutbox, 0u, true) ?: return 0
        val head = parseLogHead(headRaw)
        val next = head.nextSeq
        // Take their refreshed keys if they published any. A stale cached bundle
        // means sealing to keys they consumed long ago, which fails on their
        // side and looks like the network.
        head.prekeyBundle?.let { store.setTheirBundle(c.personaHex, it) }
        // §16.12: their ring is whatever their head says it is.
        val ring = head.ring ?: LOG_SUBKEYS
        // §16.16: their claim about how far they have read our log.
        head.readUpTo?.let { store.setTheirReadUpTo(c.personaHex, it.toLong()) }
        var seq = c.inSeq.toULong()
        var prev = c.inPrevLink
        var count = 0

        while (seq < next) {
            if (!logStillReadable(seq, next, ring)) {
                // The ring passed us. Saying so beats rendering a thread with a
                // hole in it (§16.10's conversation that did not happen).
                DucatLog.w(TAG, "lost message $seq from ${c.displayName()} — ring wrapped")
                store.append(
                    c.personaHex,
                    StoredMessage(
                        outgoing = false, seq = seq.toLong(),
                        body = "[a message was lost — this device was away too long]",
                        timestamp = System.currentTimeMillis() / 1000,
                    ),
                )
                seq += 1uL
                prev = null
                // Advance the stored cursor too, or the loop exiting here
                // re-appends the same loss placeholder on every poll.
                store.advanceInbound(c.personaHex, seq.toLong(), null)
                continue
            }
            val raw = nodeDhtGet(c.theirOutbox, logSubkey(seq, ring), true) ?: break
            val id = sealedPrekeyId(raw).toInt()
            val isOneTime = id != 0
            val secret = if (isOneTime) store.oneTimeSecret(id) else store.signedPrekeySecret()
            if (secret == null) {
                // Not lost yet. A ring slot's write propagates slowly, so this
                // read may be the slot's *previous tenant* — an old message,
                // sealed to an old key — while the real one is still in
                // flight. Declaring loss on first sight converted exactly that
                // into a permanent hole (observed: a receipt marked lost while
                // its bytes were minutes from arriving). Wait the slot out;
                // only a message still unreadable after the patience window is
                // genuinely gone.
                val key = "${c.personaHex}:$seq"
                val since = stuckSince.getOrPut(key) { System.currentTimeMillis() }
                if (System.currentTimeMillis() - since < STUCK_PATIENCE_MS) {
                    DucatLog.i(
                        TAG,
                        "message $seq from ${c.displayName()} not readable yet " +
                            "(prekey $id) — waiting for the slot to settle",
                    )
                    break
                }
                stuckSince.remove(key)
                DucatLog.w(
                    TAG,
                    "prekey $id is gone; message $seq from ${c.displayName()} is lost",
                )
                store.append(
                    c.personaHex,
                    StoredMessage(
                        outgoing = false, seq = seq.toLong(),
                        body = "[a message could not be opened — it was sealed " +
                            "to a key this device no longer holds]",
                        timestamp = System.currentTimeMillis() / 1000,
                    ),
                )
                seq += 1uL
                prev = null
                store.advanceInbound(c.personaHex, seq.toLong(), null)
                continue
            }

            val opened = openMessage(
                raw, secret, isOneTime, seq, prev, threadAad(minePersonaHex, c.personaHex),
            )
            DucatLog.i(TAG, "received seq ${opened.seq} from ${c.displayName()}")
            val arrived = StoredMessage(
                outgoing = false, seq = opened.seq.toLong(),
                body = opened.body, timestamp = opened.timestamp.toLong(),
                kind = opened.kind.toInt(),
                amountPxmr = opened.amountPxmr?.toLong() ?: 0L,
                payto = opened.payto,
                txidHex = opened.txid?.toHexString(),
                items = opened.items.map { BillItem(it.description, it.amountPxmr.toLong()) },
                taxPxmr = opened.taxPxmr?.toLong(),
                reSeq = opened.reSeq?.toLong(),
                reOwn = opened.reOwn,
                attRecord = opened.attachment?.recordKey,
                attKey = opened.attachment?.key,
                attNonce = opened.attachment?.nonce,
                attLen = opened.attachment?.len?.toLong() ?: 0L,
                attHash = opened.attachment?.ctHash?.toHexString(),
                attMime = opened.attachment?.mime,
                attName = opened.attachment?.name,
            )
            // The one funnel every arrival passes through, so the notification
            // cannot be forgotten by a new screen: if it was stored, it was
            // announced.
            Notify.message(context, c.displayName(), c.personaHex, arrived)
            store.append(c.personaHex, arrived)
            // A request carries a fresher address than anything stored (§16.12).
            opened.payto?.let { store.setTheirAddress(c.personaHex, it) }
            if (opened.consumedOneTime) store.burnOneTime(opened.prekeyId.toInt())
            prev = opened.link
            seq += 1uL
            count++
            store.advanceInbound(c.personaHex, seq.toLong(), opened.link)
        }
        return count
    }
}

fun ByteArray.toHexString(): String = joinToString("") { "%02x".format(it) }

/** Null rather than a partial array: a half-parsed transaction id is worse
 *  than an absent one, because it points at nothing and looks like it points. */
fun hexToBytes(s: String): ByteArray? {
    val t = s.trim()
    if (t.length % 2 != 0 || t.isEmpty()) return null
    return runCatching {
        ByteArray(t.length / 2) { t.substring(it * 2, it * 2 + 2).toInt(16).toByte() }
    }.getOrNull()
}
