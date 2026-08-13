package org.ducatproject.ducat

import android.content.Context
import uniffi.ducat_mobile.*

private const val TAG = "DucatMailbox"

/** A head plus seven message slots. Small deliberately: §16.11 wants a message
 *  to stop being readable rather than accumulate, and a ring that wraps in
 *  ordinary use is a ring whose wrap gets exercised. */
const val LOG_SUBKEYS: UInt = 8u
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

    /**
     * Mint a card: an inbox for the handshake, an outbox for what we will say.
     *
     * Both records are created before the card is signed, because the card
     * names the inbox and the inbox's first subkey names the outbox — a card
     * pointing at a record that does not exist yet is a card that fails after
     * someone has already accepted it.
     */
    fun issueCard(context: Context, displayName: String?, validSecs: ULong): IssuedCard {
        val store = ContactStore(context)
        val persona = PersonaStore(context).secret()

        val writer = generateWriterKeys()
        val inbox = nodeDhtCreateShared(writer.public)
        val outbox = createLog()

        val prekeys = generatePrekeys(ONE_TIME_KEYS, 60uL * 60uL * 24uL * 30uL)
        store.savePrekeys(
            prekeys.bundle,
            prekeys.signedSecret,
            prekeys.oneTimeIds.mapIndexed { i, id -> id.toInt() to prekeys.oneTimeSecrets[i] }.toMap(),
        )

        // Subkey 0: who we are and where to leave things. Written now, so a
        // claimant reading it later needs nothing from us but the record.
        nodeDhtSet(
            inbox.key, 0u,
            buildContactDetails(persona, outbox.key, prekeys.bundle, displayName),
        )

        val card = createContactCard(
            persona, inbox.key, writer.public, displayName, writer.secret, validSecs,
        )
        store.saveIssuedCard(
            inbox.key, writer.public, writer.secret,
            outbox.key, outbox.ownerPublic, outbox.ownerSecret,
        )
        DucatLog.i(TAG, "issued card: inbox=${inbox.key.take(24)}… outbox=${outbox.key.take(24)}…")
        return card
    }

    /** A fresh append-only log with its head initialised. */
    private fun createLog(): DhtRecord {
        val rec = nodeDhtCreate(LOG_SUBKEYS)
        nodeDhtSet(rec.key, 0u, buildLogHead(0uL, null))
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

        val outbox = createLog()
        val prekeys = generatePrekeys(ONE_TIME_KEYS, 60uL * 60uL * 24uL * 30uL)
        store.savePrekeys(
            prekeys.bundle,
            prekeys.signedSecret,
            prekeys.oneTimeIds.mapIndexed { i, id -> id.toInt() to prekeys.oneTimeSecrets[i] }.toMap(),
        )
        nodeDhtSet(
            scanned.inboxKey, 1u,
            buildContactDetails(persona, outbox.key, prekeys.bundle, petname),
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
        val issued = store.issuedCard() ?: return 0
        if (store.issuedCardAnswered()) return 0

        return try {
            nodeDhtOpen(issued.inboxKey, null, null)
            val raw = nodeDhtGet(issued.inboxKey, 1u, true) ?: return 0
            if (raw.isEmpty()) return 0
            val theirs = parseContactDetails(raw)
            store.add(
                Contact(
                    personaHex = theirs.persona.toHexString(),
                    petname = null,
                    assertedName = theirs.assertedName,
                    myOutbox = issued.outboxKey,
                    myOutboxOwnerPublic = issued.outboxOwnerPublic,
                    myOutboxOwnerSecret = issued.outboxOwnerSecret,
                    theirOutbox = theirs.outboxKey,
                    theirBundle = theirs.prekeyBundle,
                )
            )
            store.markIssuedCardAnswered()
            DucatLog.i(TAG, "card answered by ${theirs.assertedName}")
            1
        } catch (e: Exception) {
            DucatLog.w(TAG, "collectClaims: ${e.message}")
            0
        }
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
    ): Contact {
        val store = ContactStore(context)
        val bundle = c.theirBundle
            ?: throw IllegalStateException("No keys for this contact yet.")
        val sealed = sealMessage(
            bundle, c.outSeq.toULong(), c.outPrevLink ?: ByteArray(32), body,
            threadAad(minePersonaHex, c.personaHex),
            kind.toUByte(), amountPxmr?.toULong(), null,
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
        nodeDhtSet(c.myOutbox, logSubkey(c.outSeq.toULong(), LOG_SUBKEYS), sealed.bytes)
        // Republish our keys with every head write. Cheap — the head is read on
        // every poll anyway — and it is the only route back from an exhausted
        // supply, since the handshake inbox is a one-time artifact.
        nodeDhtSet(
            c.myOutbox, 0u,
            buildLogHead((c.outSeq + 1).toULong(), topUpIfLow(store)),
        )

        store.append(
            c.personaHex,
            StoredMessage(
                outgoing = true, seq = c.outSeq, body = body,
                timestamp = System.currentTimeMillis() / 1000,
                forwardSecret = sealed.forwardSecret,
                kind = kind, amountPxmr = amountPxmr ?: 0L,
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
        return store.all().first { it.personaHex == c.personaHex }
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
        val m = generatePrekeys(ONE_TIME_KEYS, 60uL * 60uL * 24uL * 30uL)
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
        val mine = PersonaStore(context).personaHex()
        var got = 0
        for (c in store.all()) {
            got += try {
                pollOne(store, c, mine)
            } catch (e: Exception) {
                DucatLog.w(TAG, "poll ${c.displayName()}: ${e.message}")
                0
            }
        }
        return got
    }

    private fun pollOne(store: ContactStore, c: Contact, minePersonaHex: String): Int {
        nodeDhtOpen(c.theirOutbox, null, null)
        val headRaw = nodeDhtGet(c.theirOutbox, 0u, true) ?: return 0
        val head = parseLogHead(headRaw)
        val next = head.nextSeq
        // Take their refreshed keys if they published any. A stale cached bundle
        // means sealing to keys they consumed long ago, which fails on their
        // side and looks like the network.
        head.prekeyBundle?.let { store.setTheirBundle(c.personaHex, it) }
        var seq = c.inSeq.toULong()
        var prev = c.inPrevLink
        var count = 0

        while (seq < next) {
            if (!logStillReadable(seq, next, LOG_SUBKEYS)) {
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
                continue
            }
            val raw = nodeDhtGet(c.theirOutbox, logSubkey(seq, LOG_SUBKEYS), true) ?: break
            val id = sealedPrekeyId(raw).toInt()
            val isOneTime = id != 0
            val secret = if (isOneTime) store.oneTimeSecret(id) else store.signedPrekeySecret()
            if (secret == null) {
                // Stop rather than skip: the chain links each message to the
                // one before, so stepping over an unreadable message makes
                // every later one fail to verify too.
                DucatLog.w(TAG, "prekey $id is gone; cannot read $seq")
                break
            }

            val opened = openMessage(
                raw, secret, isOneTime, seq, prev, threadAad(minePersonaHex, c.personaHex),
            )
            store.append(
                c.personaHex,
                StoredMessage(
                    outgoing = false, seq = opened.seq.toLong(),
                    body = opened.body, timestamp = opened.timestamp.toLong(),
                    kind = opened.kind.toInt(),
                    amountPxmr = opened.amountPxmr?.toLong() ?: 0L,
                ),
            )
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
