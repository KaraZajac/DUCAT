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

    /** Persisted, not in-memory: the patience clock reset on every app
     *  restart, and a phone that restarts every few minutes (this one does)
     *  made a dead letter immortal — sixteen observed minutes on a ten-minute
     *  window, with live receipts queued behind it forever. */
    private fun waitPrefs(context: Context) =
        securePrefs(context, "ducat_contacts")

    private fun stuckSince(context: Context, key: String): Long {
        val p = waitPrefs(context)
        val existing = p.getLong("stuck_$key", 0L)
        if (existing > 0L) return existing
        val now = System.currentTimeMillis()
        p.edit().putLong("stuck_$key", now).apply()
        return now
    }

    private fun clearStuck(context: Context, key: String) {
        waitPrefs(context).edit().remove("stuck_$key").apply()
    }

    /**
     * §16.13's registry, for the log line.
     *
     * It used to be "message" for kind 0 and "payment note" for everything
     * else, so a withdrawn bill, a declined bill, an emoji and every round of
     * an escrow ceremony all went out as a payment note — in the log somebody
     * reads to work out what happened to somebody's money. Never shown to a
     * user; deliberately not translated.
     */
    private fun kindName(kind: Int): String = when (kind) {
        0 -> "message"
        1 -> "bill"
        2 -> "payment note"
        3 -> "receipt"
        4 -> "reaction"
        5 -> "retraction"
        6 -> "ride offer"
        7 -> "ride accept"
        8, 9 -> "ceremony round"
        10 -> "ceremony abort"
        else -> "kind $kind"
    }

    /**
     * When to say a dead letter happened.
     *
     * Not now: the send time is inside the bytes that would not open, and the
     * clock is only ever read at the moment we give up — which can be an hour
     * after the message, and *after* the messages that follow it. A restored
     * phone showed four placeholders stamped 01:20 and 01:37 sitting above a
     * real message stamped 01:17, which reads as arriving out of order, in a
     * thread already asking the reader to trust it about what was lost.
     *
     * The message before it is the closest honest answer — the gap is
     * somewhere after that — and it keeps the thread monotonic.
     */
    private fun deadLetterTime(store: ContactStore, personaHex: String): Long =
        store.thread(personaHex).lastOrNull()?.timestamp ?: (System.currentTimeMillis() / 1000)

    /** Hash of the raw bytes last *processed* per (contact, slot) — persisted
     *  for the same reason: the stale-tenant/dead-letter distinction is only
     *  as durable as its memory. */
    private fun slotSeen(context: Context, key: String): Int? =
        waitPrefs(context).let {
            if (it.contains("slotseen_$key")) it.getInt("slotseen_$key", 0) else null
        }

    private fun recordSlotSeen(context: Context, key: String, hash: Int) {
        waitPrefs(context).edit().putInt("slotseen_$key", hash).apply()
    }

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
                // The claimant is not known yet, so the minor allocates to the
                // card and is adopted by whoever answers it (§15.10) — the
                // primary never travels in a handshake.
                if (store.publishAddress()) {
                    WalletStore(context).addressFor("card_${inbox.key}")
                } else null,
                // §16.9: the profile rides the record, never the card. A card
                // carrying a picture is a QR code nobody can scan. Scoped to
                // the purpose — a "sale" card does not carry the till owner's
                // phone number to every customer who claims it.
                MyProfile(context).toWire(purpose = purpose),
                // Stamped so the claimant can scope their reply to match.
                purpose,
            ),
        )

        val card = createContactCard(
            persona, inbox.key, writer.public, displayName, writer.secret, validSecs,
        )
        store.saveIssuedCard(
            inbox.key, writer.public, writer.secret,
            outbox.key, outbox.ownerPublic, outbox.ownerSecret,
            card.uri, purpose, validSecs.toLong(),
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
        val bundle = topUpIfLow(ContactStore(context), rec.key)
        nodeDhtSet(rec.key, 0u, buildLogHead(0uL, bundle, null, NEW_RING))
        return rec
    }

    /**
     * Accept someone's card: read their details, publish ours in the reply
     * subkey, and keep both.
     */
    fun claimCard(
        context: Context,
        scanned: ScannedCard,
        petname: String?,
        /** True only when accepting a hail: the one claim where the car —
         *  model, colour, plate — belongs in the details we publish. */
        asDriver: Boolean = false,
    ): Contact {
        val store = ContactStore(context)
        val persona = PersonaStore(context).secret()

        nodeDhtOpen(scanned.inboxKey, scanned.writerPublic, scanned.writerSecret)

        // Single use, checked by reading rather than trusting a local flag. The
        // inbox has exactly one reply subkey, so a card already answered has
        // nowhere left to write and this is the only way to find out.
        val already = nodeDhtGet(scanned.inboxKey, 1u, true)
        if (already != null && already.isNotEmpty()) {
            // Typed, because callers have to tell this apart from "the network
            // is not up yet" and from a genuinely malformed card, and matching
            // on English prose to do it would break in every other language.
            throw CardAlreadyUsed()
        }

        val raw = nodeDhtGet(scanned.inboxKey, 0u, true)
            ?: throw DetailsNotPublished()
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
                persona, outbox.key, prekeys.bundle,
                // **Our** name, not the one we just chose for them. This
                // argument is `display_name` — what the reply asserts about
                // its sender — and it was being handed `petname`, the private
                // label this device picked for the other party. Wrong twice:
                // with a petname it published "what I call you" *as* my name,
                // and without one (every hail, every scan, every ducat: link —
                // they all pass null) nothing travelled, so the other side
                // stored no name at all. A rider watching the curb was shown
                // "Unnamed contact" beside the plate they were looking for.
                NameStore(context).get(),
                // Their address, for them alone (§15.10): the counterparty is
                // known here, so the published address is their subaddress.
                if (store.publishAddress()) {
                    WalletStore(context).addressFor(theirs.persona.toHexString())
                } else null,
                // Scope our reply to what the issuer said this handshake is for
                // (§16.9): answering a "sale" card sends no reach-me identifiers,
                // and a null purpose — an older card — is read as not a contact
                // exchange, the private default. A driver claiming a hail still
                // sends the car, which is what a rider is scanning the curb for.
                MyProfile(context).toWire(purpose = theirs.purpose, driving = asDriver),
                theirs.purpose,
            ),
        )

        // What this thread already had, if we have met before.
        //
        // `add` replaces the whole record, and a Contact built here carries
        // the *defaults* for the per-direction chain counters — zero and no
        // previous link. Claiming a second card from somebody already known
        // therefore rewound both counters while their log kept its history,
        // and every later message was refused with "this message does not
        // follow the one before it". The thread simply stopped, in both
        // directions, with no way back. §16.12's counters are not metadata;
        // they are whether the thread works.
        //
        // Our own outbox is genuinely new — it was created three lines up —
        // so zero is right for the sending side. Their log is only new if the
        // card names a different one, which is exactly the test below.
        val prior = store.all().firstOrNull { it.personaHex == theirs.persona.toHexString() }
        val sameLog = prior != null && prior.theirOutbox == theirs.outboxKey
        // A card carries a persona and nothing signed over it, so it may not
        // move an address that is already working. See foldCardAddress.
        val (payto, heldPayto) = foldCardAddress(prior, theirs.payto)

        val c = Contact(
            personaHex = theirs.persona.toHexString(),
            // A name the reader chose survives a re-claim: this argument is
            // null on every path that is not somebody typing one.
            petname = petname ?: prior?.petname,
            assertedName = theirs.assertedName,
            inSeq = if (sameLog) prior!!.inSeq else 0,
            inPrevLink = if (sameLog) prior!!.inPrevLink else null,
            myOutbox = outbox.key,
            myOutboxOwnerPublic = outbox.ownerPublic,
            myOutboxOwnerSecret = outbox.ownerSecret,
            theirOutbox = theirs.outboxKey,
            theirBundle = theirs.prekeyBundle,
            theirAddress = payto,
            pendingAddress = heldPayto,
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
        if (heldPayto != null && heldPayto != prior?.pendingAddress) {
            warnAddressHeld(context, c)
        }
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
                // The same rule as claimCard, from the issuer's side: keep the
                // counters for whichever log has not changed underneath them.
                val prior = store.all().firstOrNull { it.personaHex == personaHex }
                val sameOurs = prior != null && prior.myOutbox == issued.outboxKey
                val sameTheirs = prior != null && prior.theirOutbox == theirs.outboxKey
                val (payto, heldPayto) = foldCardAddress(prior, theirs.payto)
                store.add(
                    Contact(
                        personaHex = personaHex,
                        petname = prior?.petname,
                        assertedName = theirs.assertedName,
                        outSeq = if (sameOurs) prior!!.outSeq else 0,
                        outPrevLink = if (sameOurs) prior!!.outPrevLink else null,
                        inSeq = if (sameTheirs) prior!!.inSeq else 0,
                        inPrevLink = if (sameTheirs) prior!!.inPrevLink else null,
                        myOutbox = issued.outboxKey,
                        myOutboxOwnerPublic = issued.outboxOwnerPublic,
                        myOutboxOwnerSecret = issued.outboxOwnerSecret,
                        theirOutbox = theirs.outboxKey,
                        theirBundle = theirs.prekeyBundle,
                        theirAddress = payto,
                        pendingAddress = heldPayto,
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
                if (heldPayto != null && heldPayto != prior?.pendingAddress) {
                    // firstOrNull, not first: all() drops contacts with no
                    // outbox on either side, and this record was written from
                    // whatever the card carried. Losing the notice beats
                    // throwing out of a card that was otherwise collected.
                    store.all().firstOrNull { it.personaHex == personaHex }
                        ?.let { warnAddressHeld(context, it) }
                }
                store.markCardAnswered(issued.inboxKey, personaHex)
                WalletStore(context).adoptMinor("card_${issued.inboxKey}", personaHex)
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
     * The local echo, the counters and the sealed bytes are persisted before
     * the DHT sees anything, and the slot is written **before** the head. A
     * reader that saw `next_seq` move and then found an unwritten slot would
     * have been told a message exists that does not; this order only ever
     * makes one briefly late — and a persisted counter with the slot still
     * owed is filled in by the next send, with the same seq and same bytes.
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
        /** The bill was settled outside DUCAT: this receipt deliberately names
         *  no transaction, and the record it leaves must say so (§15.11). */
        oob: Boolean = false,
        /** §15.12: a ride offer's distance-in-time; refused on other kinds. */
        etaSecs: Long? = null,
        /** §17.9 ceremony: opaque threshold bytes, round tag, escrow id. */
        payload: ByteArray? = null,
        round: Long? = null,
        ceremonyId: ByteArray? = null,
    ): Contact {
        val store = ContactStore(context)
        val bundle = c.theirBundle
            ?: throw IllegalStateException("No keys for this contact yet.")
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
        var ring = c.myRing.toUInt()
        // A previous send persisted its message and counters but died before
        // the DHT took the slot. Those bytes go out first — the same seq and
        // the same bytes, never a re-seal, which would put different content
        // under a sequence number.
        store.pendingSlot(c.personaHex)?.let { (pseq, pbytes) ->
            if (pseq < c.outSeq) {
                DucatLog.i(TAG, "delivering seq $pseq to ${c.displayName()} " +
                    "left over from an interrupted send")
                ring = writeSlotClamped(store, c, pseq.toULong(), pbytes, ring)
            }
            store.clearPendingSlot(c.personaHex)
        }
        DucatLog.i(TAG, "sending ${kindName(kind)} seq ${c.outSeq} to ${c.displayName()}")
        val sealed = sealMessage(
            bundle, c.outSeq.toULong(), c.outPrevLink ?: ByteArray(32), body,
            threadAad(minePersonaHex, c.personaHex),
            kind.toUByte(), amountPxmr?.toULong(),
            txidHex?.let { hexToBytes(it) }, payto,
            items.map { uniffi.ducat_mobile.BillLine(it.description, it.amountPxmr.toULong()) },
            taxPxmr?.toULong(),
            reSeq?.toULong(), reOwn, attachment,
            etaSecs?.toULong(),
            payload, round?.toULong(), ceremonyId,
        )
        // Everything local lands before anything remote. The failure orders
        // are not symmetric: a published slot and head with the counter lost
        // to a process death reuses this seq with different content next time
        // — a fork every reader keeps — while a persisted counter with the
        // slot unwritten is only a late slot, retried above on the next send,
        // and a head one behind the counter self-heals on its next publish.
        store.appendAndAdvanceOutbound(
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
                oob = oob,
                etaSecs = etaSecs,
            ),
            c.outSeq + 1, sealed.nextLink, sealed.bytes,
        )

        // Withdraw the key we just used from our *cached copy* of their bundle
        // — also before the network, since the sealed bytes are committed for
        // delivery from here whether or not tonight's write lands.
        // select() takes the first one-time entry, so without this every message
        // seals to the same key — the first is accepted, the receiver burns it,
        // and every later one comes back as an unknown prekey. Exactly the bug
        // that hit the published bundle earlier, on the other side of the wire.
        if (sealed.prekeyId != 0u) {
            if (sealed.forwardSecret) {
                store.recordUsedTheirId(c.personaHex, sealed.prekeyId.toInt())
            }
            runCatching { prunePrekey(bundle, sealed.prekeyId) }
                .onSuccess { store.setTheirBundle(c.personaHex, it) }
        }
        ring = writeSlotClamped(store, c, c.outSeq.toULong(), sealed.bytes, ring)
        // Republish our keys with every head write. Cheap — the head is read on
        // every poll anyway — and it is the only route back from an exhausted
        // supply, since the handshake inbox is a one-time artifact.
        nodeDhtSet(
            c.myOutbox, 0u,
            buildLogHead(
                (c.outSeq + 1).toULong(),
                topUpIfLow(store, c.myOutbox),
                // §16.16: the watermark rides every head write for free, and
                // only when the user opted in. c.inSeq is "I have accepted
                // your messages below this", which is exactly the claim.
                if (store.readReceipts()) c.inSeq.toULong() else null,
                ring.takeIf { it != 8u },
            ),
        )
        store.clearPendingSlot(c.personaHex)
        DucatLog.i(TAG, "delivered seq ${c.outSeq} to ${c.displayName()}" +
            if (sealed.forwardSecret) "" else " (no forward secrecy — their one-time keys ran out)")
        return store.all().first { it.personaHex == c.personaHex }
    }

    /**
     * One slot write, healing legacy logs on the way. Threads minted before
     * the 8-subkey/32-ring mismatch was fixed hold records smaller than the
     * ring their head claims. Slots 0–6 map identically under ring 8 and
     * ring 32, so clamping the ring back to the record's real size heals the
     * thread with no history rewritten — the head republishes the honest ring
     * and readers follow it. Returns the ring actually in effect.
     */
    private fun writeSlotClamped(
        store: ContactStore,
        c: Contact,
        seq: ULong,
        bytes: ByteArray,
        ring0: UInt,
    ): UInt {
        var ring = ring0
        try {
            nodeDhtSet(c.myOutbox, logSubkey(seq, ring), bytes)
        } catch (e: Exception) {
            if (e.message?.contains("out of range") == true && ring > LOG_SUBKEYS) {
                DucatLog.w(
                    TAG,
                    "legacy log smaller than its ring — clamping ${c.displayName()} to $LOG_SUBKEYS",
                )
                ring = LOG_SUBKEYS
                store.setMyRing(c.personaHex, LOG_SUBKEYS.toInt())
                nodeDhtSet(c.myOutbox, logSubkey(seq, ring), bytes)
            } else {
                throw e
            }
        }
        return ring
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
                    store.threadBundle(c.myOutbox) ?: store.prekeyBundle(),
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
    private fun topUpIfLow(store: ContactStore, outbox: String): ByteArray? {
        // §16.11: each thread's head offers its own disjoint batch of ids —
        // the secrets are global, the offering is partitioned, so two
        // contacts can never seal to the same key. A fresh batch replaces
        // the thread's offer wholesale; unconsumed ids from the old offer
        // keep their secrets, so a sender on a stale head still opens.
        if (store.threadOneTimeRemaining(outbox) > 6) return store.threadBundle(outbox)
        val m = generatePrekeys(
            ONE_TIME_KEYS, 60uL * 60uL * 24uL * 30uL,
            store.nextPrekeyStart(ONE_TIME_KEYS.toInt()).toUInt(),
            store.signedPrekeySecret(),
        )
        store.savePrekeys(
            // The global bundle field is legacy — heads now carry per-thread
            // offers — and empty material never overwrites it (§4.3's restore
            // rule, reused): only the secrets and the signed key land here.
            ByteArray(0), m.signedSecret,
            m.oneTimeIds.mapIndexed { i, id -> id.toInt() to m.oneTimeSecrets[i] }.toMap(),
        )
        store.setThreadBundle(outbox, m.bundle)
        DucatLog.i(TAG, "cut a fresh one-time batch for this thread")
        return m.bundle
    }

    /** Read everything new from every contact's outbox. Returns how many landed.
     *
     *  @Synchronized: the global poller, a screen's pump, and a hail's wait
     *  all call this on their own clocks. Two polls running at once processed
     *  the same sealed message twice — a ceremony round-0 handled in parallel
     *  joined a bond twice and double-committed (found live, 2026-08-16). One
     *  poll at a time; a second caller waits and then finds the log already
     *  drained, which costs nothing but the wait. */
    /**
     * Re-cut and re-publish every thread's one-time offer (§16.11).
     *
     * Only after a restore, and only once. The secrets come back from the
     * bundle as they were when it was written; the ids burned between then and
     * the export do not, since the burn pen is not exported. Meanwhile the
     * peers hold the offer from before those burns and no record of which ids
     * they already spent — that ledger is not in a backup either — so they
     * re-offer dead keys, seal to them, and every message lands unreadable and
     * is tombstoned ten minutes later.
     *
     * Recutting fixes it from this side alone: a fresh batch replaces the
     * thread's offer wholesale, the peer picks it up on its next head read, and
     * the channel is clean without either person doing anything. That already
     * happened on the first outgoing message — this just stops it waiting for
     * one, because a phone restored after a loss is on the receiving end first.
     *
     * Best-effort per contact, and the flag only clears when every one landed:
     * a partial pass over a network still coming up must run again, not report
     * itself done.
     */
    private fun republishBundles(context: Context, store: ContactStore) {
        var allLanded = true
        for (c0 in store.all()) {
            if (c0.myOutboxOwnerSecret.isEmpty()) continue
            var c = c0
            runCatching {
                nodeDhtOpen(c.myOutbox, c.myOutboxOwnerPublic, c.myOutboxOwnerSecret)
                // Catch the send counter up to what we actually published.
                //
                // A bundle restores the counter to where it stood when the
                // bundle was written, and the messages sent after that are
                // still out there — read, and counted, by the person we sent
                // them to. Resuming underneath them means every message is
                // written at a sequence they have already passed, so they skip
                // it as one they have seen: nothing arrives, nothing errors,
                // and both sides go on believing the thread is fine. Watched it
                // happen — Sam's poller waking on the right record, at the right
                // second, and reading nothing, twice.
                //
                // Our own head is the record of how far we got, and it survived
                // on the network while the counter did not. Take the further of
                // the two. Written before the head below, so the republish does
                // not go on to overwrite that number with the stale one.
                runCatching {
                    val mine = nodeDhtGet(c.myOutbox, 0u, true)?.let { parseLogHead(it) }
                    val published = mine?.nextSeq?.toLong() ?: 0L
                    if (published > c.outSeq) {
                        DucatLog.i(
                            TAG,
                            "our log for ${c.displayName()} reached $published, " +
                                "the bundle said ${c.outSeq} — resuming from theirs",
                        )
                        store.advanceOutbound(c.personaHex, published, c.outPrevLink ?: ByteArray(32))
                        c = store.all().first { it.personaHex == c.personaHex }
                    }
                }.onFailure { DucatLog.w(TAG, "own head for ${c.displayName()}: ${it.message}") }
                nodeDhtSet(
                    c.myOutbox, 0u,
                    buildLogHead(
                        c.outSeq.toULong(),
                        // Forced, not topUpIfLow: the supply reads as healthy —
                        // the secrets are there — and it is the *offer* that is
                        // stale. Asking whether we are low would decline to fix
                        // exactly the case this exists for.
                        recutThreadBundle(store, c.myOutbox),
                        if (store.readReceipts()) c.inSeq.toULong() else null,
                        c.myRing.toUInt().takeIf { it != 8u },
                    ),
                )
            }.onFailure {
                // A record the network no longer has is not a slow network:
                // there is nothing at that key to republish and there never
                // will be again, so holding the whole pass open for it means
                // running on every poll for ever — cutting a fresh batch of
                // thirty-two one-time keys for every other contact each time
                // round. Seen on a restored phone carrying a thread whose
                // record had expired: "republish Unnamed contact: Key not
                // found", which no amount of retrying improves.
                val gone = it.message?.contains("Key not found", ignoreCase = true) == true
                if (!gone) allLanded = false
                DucatLog.w(
                    TAG,
                    "republish ${c.displayName()}: ${it.message}" +
                        if (gone) " — that record is gone, nothing to republish" else "",
                )
            }
        }
        if (allLanded) {
            store.setBundlesNeedRepublish(false)
            DucatLog.i(TAG, "republished every thread's one-time offer after a restore")
        }
    }

    /** A fresh batch for this thread, whatever the current supply looks like. */
    private fun recutThreadBundle(store: ContactStore, outbox: String): ByteArray {
        val m = generatePrekeys(
            ONE_TIME_KEYS, 60uL * 60uL * 24uL * 30uL,
            store.nextPrekeyStart(ONE_TIME_KEYS.toInt()).toUInt(),
            store.signedPrekeySecret(),
        )
        store.savePrekeys(
            ByteArray(0), m.signedSecret,
            m.oneTimeIds.mapIndexed { i, id -> id.toInt() to m.oneTimeSecrets[i] }.toMap(),
        )
        store.setThreadBundle(outbox, m.bundle)
        return m.bundle
    }

    @Synchronized
    fun poll(context: Context): Int {
        val store = ContactStore(context)
        // Before reading anyone: a restored device is advertising keys it does
        // not hold, and every message written against them is lost on arrival.
        if (store.bundlesNeedRepublish()) republishBundles(context, store)
        // Each poll is also the clock for the forward-secrecy delete: burned
        // one-time secrets past their grace window leave for good here.
        store.sweepBurnedPrekeys()
        // Withdrawn hails whose board clears could not land yet (offline
        // take-downs); each poll is a retry until the slot is verifiably
        // not ours or the notice has expired out of everyone's sweeps.
        runCatching { org.ducatproject.ducat.ui.sweepHailTombstones(context) }
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

    /**
     * A card whose one reply slot is already written (§7.5's claim-once).
     *
     * The English sentence it used to carry is a string resource
     * (`contacts_reply_replay`), so the screen can say it in the reader's own
     * language instead of repeating an exception message.
     */
    class CardAlreadyUsed : IllegalStateException("card already claimed")

    /**
     * The card exists but the details behind it have not arrived yet.
     *
     * Ordinary at a counter: the till mints a fresh card per sale, and a
     * customer scanning the moment it appears can outrun the DHT write. Timed
     * at about forty seconds on stagenet, after which the same card claims
     * fine — so this is "wait and scan again", not a broken code, and it must
     * not be reported as one.
     */
    class DetailsNotPublished : IllegalStateException("card details not published yet")

    /**
     * Veilid's TryAgain surfacing through the bridge as message text.
     *
     * Not private: a screen that tells someone their contact card is "broken,
     * already claimed, or no longer valid" when the node simply has not
     * finished connecting sends them back to ask for a replacement card and
     * burn a perfectly good one.
     */
    fun isOffline(e: Throwable) =
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
                // Placeholder and cursor in one commit, or a process death
                // between them re-appends the same loss on the next poll.
                DucatLog.w(TAG, "lost message $seq from ${c.displayName()} — ring wrapped")
                store.appendAndAdvance(
                    c.personaHex,
                    StoredMessage(
                        outgoing = false, seq = seq.toLong(),
                        body = "[a message was lost — this device was away too long]",
                        timestamp = deadLetterTime(store, c.personaHex),
                        deadLetter = true,
                    ),
                    (seq + 1uL).toLong(), null,
                )
                // A patience clock started for this seq before the ring caught
                // it would otherwise sit in prefs forever.
                clearStuck(context, "${c.personaHex}:$seq")
                seq += 1uL
                prev = null
                continue
            }
            val raw = nodeDhtGet(c.theirOutbox, logSubkey(seq, ring), true) ?: break
            val id = sealedPrekeyId(raw).toInt()
            val isOneTime = id != 0
            val secret = if (isOneTime) store.oneTimeSecret(id) else store.signedPrekeySecret()
            if (secret == null) {
                val slotKey = "${c.personaHex}:${logSubkey(seq, ring)}"
                val rawHash = raw.contentHashCode()
                if (slotSeen(context, slotKey) == rawHash) {
                    // The slot's previous tenant — bytes this reader already
                    // processed as an earlier sequence. The real write is still
                    // propagating; wait as long as it takes, no clock.
                    DucatLog.i(
                        TAG,
                        "slot for message $seq from ${c.displayName()} still " +
                            "holds its previous tenant — waiting",
                    )
                    break
                }
                // Bytes this reader has no memory of processing. Either this
                // seq's real write, sealed to a key this device no longer
                // holds, or an older tenant still propagating that never got
                // processed here — the seen-hash cannot tell them apart, so
                // both get the patience window. Declaring loss on sight
                // turned every unprocessed old tenant into a false loss.
                val key = "${c.personaHex}:$seq"
                val since = stuckSince(context, key)
                if (System.currentTimeMillis() - since < STUCK_PATIENCE_MS) {
                    // Everything behind this one is stuck too — the log is read
                    // in order, so none of them can be reached until this
                    // resolves. Start their clocks now rather than each in turn
                    // when it reaches the front, or the windows run end to end
                    // and a thread with n unreadable messages takes n times ten
                    // minutes to drain. A restored phone is exactly that case:
                    // its cursor rewinds to before a run of slots sealed to keys
                    // it no longer holds, and until they clear it cannot see
                    // anything newer. Nothing is declared lost early — this only
                    // decides when the patience *started*, and it started when
                    // the message was first sitting there unreadable.
                    var behind = seq + 1uL
                    while (behind < next) {
                        stuckSince(context, "${c.personaHex}:$behind")
                        behind += 1uL
                    }
                    DucatLog.i(
                        TAG,
                        "message $seq from ${c.displayName()} not readable yet " +
                            "(prekey $id) — waiting for the slot to settle",
                    )
                    break
                }
                clearStuck(context, key)
                DucatLog.w(
                    TAG,
                    "prekey $id is gone; message $seq from ${c.displayName()} is lost",
                )
                // The bytes being given up on become the slot's last-processed
                // tenant: when the next sequence lands here and these bytes
                // are still what the network serves, that is propagation lag,
                // not another loss.
                recordSlotSeen(context, slotKey, rawHash)
                store.appendAndAdvance(
                    c.personaHex,
                    StoredMessage(
                        outgoing = false, seq = seq.toLong(),
                        body = "[a message could not be opened — it was sealed " +
                            "to a key this device no longer holds]",
                        timestamp = deadLetterTime(store, c.personaHex),
                        deadLetter = true,
                    ),
                    (seq + 1uL).toLong(), null,
                )
                seq += 1uL
                prev = null
                continue
            }

            val opened = try {
                openMessage(
                    raw, secret, isOneTime, seq, prev, threadAad(minePersonaHex, c.personaHex),
                )
            } catch (e: uniffi.ducat_mobile.ContactException) {
                // Decrypted but refused: the bytes are final and will never
                // parse differently, so this is a dead letter, not weather —
                // §16.11's must-not-block rule, one layer up. (Seen live: a
                // sealed empty text field wedged this loop for good.)
                if (e.message?.contains("Malformed") == true) {
                    DucatLog.w(TAG, "message $seq from ${c.displayName()} is malformed — recorded and skipped")
                    store.appendAndAdvance(
                        c.personaHex,
                        StoredMessage(
                            outgoing = false, seq = seq.toLong(),
                            body = "[a message could not be understood — " +
                                "the sender's client encoded it wrongly]",
                            timestamp = deadLetterTime(store, c.personaHex),
                        deadLetter = true,
                        ),
                        (seq + 1uL).toLong(), null,
                    )
                    clearStuck(context, "${c.personaHex}:$seq")
                    recordSlotSeen(context, "${c.personaHex}:${logSubkey(seq, ring)}", raw.contentHashCode())
                    seq += 1uL
                    prev = null
                    continue
                }
                // Two refusals that are final for these bytes and were both
                // rethrown, which aborts the contact's whole poll — every poll,
                // forever, with no patience window and no dead letter. One such
                // message walls off every message behind it for good, and both
                // of these are things a restore produces.
                //
                //  * **It does not authenticate.** The key fits and the
                //    ciphertext does not, because a one-time id was re-minted
                //    and the secret under it is not the one this was sealed to.
                //  * **It does not follow the one before it.** The sender lost
                //    messages it had already sent — restored from a bundle
                //    older than its last send — so the chain has a hole in it
                //    that neither side can fill. §16.11 already treats a gap as
                //    unverifiable rather than fatal on the *reading* side; this
                //    is the same hole arriving from the writing side.
                //
                // Both take the window rather than an instant tombstone,
                // because the bytes on a slot may be a previous tenant that is
                // slow to be replaced, and those open fine once the real write
                // lands.
                val unopenable = e.message?.contains("BadSig") == true ||
                    e.message?.contains("did not authenticate") == true
                val outOfChain = e.message?.contains("does not follow") == true
                if (unopenable || outOfChain) {
                    val badKey = "${c.personaHex}:$seq"
                    if (System.currentTimeMillis() - stuckSince(context, badKey) < STUCK_PATIENCE_MS) {
                        var behind = seq + 1uL
                        while (behind < next) {
                            stuckSince(context, "${c.personaHex}:$behind")
                            behind += 1uL
                        }
                        DucatLog.i(
                            TAG,
                            "message $seq from ${c.displayName()} " +
                                (if (outOfChain) "does not follow the one before it"
                                else "does not open with the key it names") +
                                " — waiting for the slot to settle",
                        )
                        break
                    }
                    clearStuck(context, badKey)
                    DucatLog.w(
                        TAG,
                        "message $seq from ${c.displayName()} " +
                            (if (outOfChain) "broke the chain" else "never authenticated") +
                            " — recorded and skipped",
                    )
                    store.appendAndAdvance(
                        c.personaHex,
                        StoredMessage(
                            outgoing = false, seq = seq.toLong(),
                            body = if (outOfChain) {
                                "[a message is missing here — the sender lost it " +
                                    "before this device could read it]"
                            } else {
                                "[a message could not be opened — it was sealed " +
                                    "to a key this device no longer holds]"
                            },
                            timestamp = deadLetterTime(store, c.personaHex),
                        deadLetter = true,
                        ),
                        (seq + 1uL).toLong(), null,
                    )
                    recordSlotSeen(context, "${c.personaHex}:${logSubkey(seq, ring)}", raw.contentHashCode())
                    seq += 1uL
                    prev = null
                    continue
                }
                throw e
            }
            // If this seq had been waiting out the patience window, it made
            // it after all — the tracker must not keep growing.
            clearStuck(context, "${c.personaHex}:$seq")
            recordSlotSeen(context, "${c.personaHex}:${logSubkey(seq, ring)}", raw.contentHashCode())
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
                etaSecs = opened.etaSecs?.toLong(),
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
            // announced. Message and cursor land in one commit: a process
            // death between them re-delivered the message on the next poll —
            // a duplicate thread row, and its receipt captured twice.
            Notify.message(context, c.displayName(), c.personaHex, arrived)
            store.appendAndAdvance(c.personaHex, arrived, (seq + 1uL).toLong(), opened.link)
            // §17.9: a ceremony round drives the threshold engine, not the
            // chat. The message is recorded above like any other so the
            // thread stays honest; the orchestrator acts on it here.
            if (arrived.kind == 8) {
                runCatching {
                    Ceremony.onDkgRound(context, c, opened.ceremonyId, opened.round?.toLong(), opened.payload)
                }.onFailure { DucatLog.w(TAG, "ceremony round: ${it.message}") }
            }
            if (arrived.kind == 9) {
                runCatching {
                    // The amount rides along: a release proposal names what
                    // the funder gets back, and the consent screen states it
                    // (§15.12 — the claimed split, on the screen that signs).
                    Ceremony.onFrostRound(
                        context, c, opened.ceremonyId, opened.round?.toLong(), opened.payload,
                        arrived.amountPxmr.takeIf { it > 0 },
                    )
                }.onFailure { DucatLog.w(TAG, "frost round: ${it.message}") }
            }
            // §17.9: the far side withdrew. Core has validated that this
            // names a ceremony and carries no round payload, so all that is
            // left is to believe it — and Ceremony.onAbort decides whether to,
            // because an escrow with money in it is not endable by message.
            if (arrived.kind == 10) {
                runCatching {
                    opened.ceremonyId?.let {
                        Ceremony.onAbort(context, it.toHexString())
                    }
                }.onFailure { DucatLog.w(TAG, "ceremony abort: ${it.message}") }
            }
            // A request carries a fresher address than anything stored (§16.12).
            opened.payto?.let { store.setTheirAddress(c.personaHex, it) }
            if (opened.consumedOneTime) store.burnOneTime(opened.prekeyId.toInt())
            prev = opened.link
            seq += 1uL
            count++
        }
        return count
    }
}

/**
 * Somebody handed us a card that wants to be paid somewhere new.
 *
 * Worth waking a phone for. Every other consequence of a card is additive — a
 * new contact, a fresher avatar — and this one is the single field where being
 * wrong costs money. The notification does not say who is at fault, because
 * the honest case is real: a contact who lost their phone comes back on a new
 * card with a new wallet, and their old thread is dead so §16.12's rotation
 * has nowhere to ride.
 */
private fun warnAddressHeld(context: Context, c: Contact) {
    DucatLog.w(TAG, "${c.personaHex.take(12)}… card wants a different payment address — holding")
    Notify.post(
        context,
        context.getString(R.string.notify_payto_changed_title),
        context.getString(R.string.notify_payto_changed_body, c.displayName()),
    )
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
