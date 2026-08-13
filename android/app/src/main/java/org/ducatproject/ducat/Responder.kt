package org.ducatproject.ducat

import android.content.Context
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import uniffi.ducat_mobile.checkInboundClaim
import uniffi.ducat_mobile.generatePrekeys
import uniffi.ducat_mobile.nodePollCall
import uniffi.ducat_mobile.nodeReply
import uniffi.ducat_mobile.openMessage
import uniffi.ducat_mobile.sealedPrekeyId

private const val TAG = "DucatResponder"

private const val MSG_CLAIM: Byte = 0x40
private const val MSG_TEXT: Byte = 0x41
private const val MSG_PREKEYS: Byte = 0x42

/**
 * The half of a conversation that answers.
 *
 * Sending is easy; being reachable is the part that makes a peer-to-peer app
 * actually peer-to-peer. This polls the node's inbox and replies, which is what
 * turns "I can send a message" into "two people can talk".
 *
 * Every branch replies. A Veilid `app_call` blocks the caller until it is
 * answered or times out, so silently dropping a request we dislike spends
 * someone else's thirty seconds — §8.7.2 measured what that costs. A refusal is
 * a reply.
 */
class Responder(private val context: Context) {

    fun start(scope: CoroutineScope) {
        scope.launch(Dispatchers.IO) {
            while (isActive) {
                val call = runCatching { nodePollCall() }.getOrNull()
                if (call == null) {
                    // Polling rather than a callback because UniFFI callbacks
                    // into Kotlin from a Veilid worker thread are a sharper
                    // edge than a 250 ms tick is a cost.
                    delay(250)
                    continue
                }
                val reply = runCatching { handle(call.message) }
                    .getOrElse { refusal(it.message ?: "failed") }
                runCatching { nodeReply(call.id, reply) }
                    .onFailure { Log.w(TAG, "reply failed: ${it.message}") }
            }
        }
    }

    private fun handle(msg: ByteArray): ByteArray {
        if (msg.isEmpty()) return refusal("empty")
        val body = msg.copyOfRange(1, msg.size)
        return when (msg[0]) {
            MSG_PREKEYS -> ourBundle()
            MSG_CLAIM -> handleClaim(body)
            MSG_TEXT -> handleText(body)
            else -> refusal("unknown message type")
        }
    }

    /**
     * Publish our prekeys, generating them the first time anyone asks.
     *
     * Generated lazily so a user who never chats never creates key material
     * they would then have to protect.
     */
    private fun ourBundle(): ByteArray {
        val store = ContactStore(context)
        store.prekeyBundle()?.let { if (store.oneTimeRemaining() > 0) return it }

        // Refilled when the supply runs out. §16.11's fallback exists so this
        // is never fatal, but running on the fallback is the weaker state and
        // topping up is how a device leaves it.
        val m = generatePrekeys(32u, 60uL * 60uL * 24uL * 30uL)
        val oneTime = m.oneTimeIds.mapIndexed { i, id -> id.toInt() to m.oneTimeSecrets[i] }.toMap()
        store.savePrekeys(m.bundle, m.signedSecret, oneTime)
        return m.bundle
    }

    private fun handleClaim(body: ByteArray): ByteArray {
        val cards = CardStore(context)
        val card = cards.cardBytes() ?: return refusal("no card outstanding")
        val scanned = checkInboundClaim(card, body, cards.claimed())
        cards.markClaimed()
        ContactStore(context).add(
            Contact(
                personaHex = scanned.persona.joinToString("") { "%02x".format(it) },
                petname = null,
                assertedName = scanned.assertedName,
                rendezvous = scanned.rendezvous,
                claimSecret = ByteArray(0),
            )
        )
        return "ok".toByteArray()
    }

    private fun handleText(body: ByteArray): ByteArray {
        val store = ContactStore(context)
        val mine = PersonaStore(context).personaHex()
        val id = sealedPrekeyId(body).toInt()
        val isOneTime = id != 0
        val secret = (if (isOneTime) store.oneTimeSecret(id) else store.signedPrekeySecret())
            ?: return refusal("that key is gone")

        // Sender identity comes from which thread the ciphertext authenticates
        // under: the AAD binds it to a persona (§16.11), so a message that opens
        // under a contact's AAD is from that contact.
        for (c in store.all()) {
            val opened = runCatching {
                openMessage(
                    body, secret, isOneTime,
                    c.inSeq.toULong(),
                    c.inPrevLink,
                    threadAad(mine, c.personaHex),
                )
            }.getOrNull() ?: continue

            store.append(
                c.personaHex,
                StoredMessage(
                    outgoing = false,
                    seq = opened.seq.toLong(),
                    body = opened.body,
                    timestamp = opened.timestamp.toLong(),
                ),
            )
            store.update(c.copy(inSeq = c.inSeq + 1, inPrevLink = opened.link))
            // The deletion §16.11 is made of. After this the message we just
            // read cannot be read again by anyone, ourselves included.
            if (opened.consumedOneTime) store.burnOneTime(opened.prekeyId.toInt())
            return "ok".toByteArray()
        }
        return refusal("not from a known contact, or out of order")
    }

    /** §18.5's shape, minus the encoder: this path has no signed context. */
    private fun refusal(why: String) = ("!" + why).toByteArray()
}
