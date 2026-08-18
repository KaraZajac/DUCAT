package org.ducatproject.desk

import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.Mailbox
import org.ducatproject.ducat.PersonaStore
import org.ducatproject.ducat.toHexString
import uniffi.ducat_mobile.nodeStart
import uniffi.ducat_mobile.nodeStatus
import uniffi.ducat_mobile.nodeStop

/**
 * The desk, driven blind: issue a card, wait for a stranger, converse.
 *
 * What the window does with clicks, this does with stdout — so a shell (or
 * CI) can hold a real conversation with the desk over the live network and
 * assert on every line. The counterpart is the Rust harness claiming the
 * printed card; between them they exercise the shared brain end to end
 * without a phone in the room.
 */
fun main() {
    // DUCAT_DESK_STATE keeps the desk's memory across runs — the shape of a
    // process-death test is exactly "same state, new process".
    val dir = System.getenv("DUCAT_DESK_STATE")?.let { java.io.File(it).apply { mkdirs() } }
        ?: kotlin.io.path.createTempDirectory("ducat-desk-e2e").toFile()
    Unlock.orExit(dir)
    val context = DeskContext(dir)
    println("e2e: state in ${dir.absolutePath}")
    nodeStart("${dir.absolutePath}/veilid", true)
    while (!nodeStatus().publicInternetReady) Thread.sleep(2_000)
    println("e2e: node ready")

    // A card in the environment is always claimed (unless its persona is
    // already a contact) — an arbiter desk has to befriend BOTH principals,
    // which takes one claim per run against the same state. With no card
    // and no contacts, the desk issues one and waits, as before.
    val toClaim = System.getenv("DUCAT_DESK_CLAIM")
    if (!toClaim.isNullOrEmpty()) {
        val scanned = uniffi.ducat_mobile.readContactCard(toClaim)
        val known = ContactStore(context).all()
            .any { it.personaHex == scanned.persona.toHexString() }
        if (known) {
            println("E2E_ALREADY_KNOWN")
        } else {
            val c = Mailbox.claimCard(context, scanned, null)
            println("E2E_CLAIMED ${c.displayName()} ${c.personaHex}")
            Mailbox.send(context, c, "hello from the desk",
                PersonaStore(context).personaHex())
            println("E2E_GREETED")
        }
    } else if (ContactStore(context).all().isEmpty()) {
        val card = Mailbox.issueCard(context, "desk-e2e", 60uL * 60uL)
        // One line, greppable, complete: the whole handshake is this string.
        println("E2E_CARD ${card.uri}")
    } else {
        println("E2E_RESUMED")
    }

    val store = ContactStore(context)
    val mine = PersonaStore(context).personaHex()
    val replied = mutableSetOf<Long>()
    val deadline = System.currentTimeMillis() + 15 * 60_000
    var known = 0

    while (System.currentTimeMillis() < deadline) {
        runCatching {
            Mailbox.collectClaims(context)
            Mailbox.poll(context)
        }.onFailure { println("e2e: poll error ${it.message}") }

        val contacts = store.all()
        if (contacts.size != known) {
            known = contacts.size
            contacts.forEach { println("E2E_CONTACT ${it.displayName()} ${it.personaHex}") }
        }
        for (c in contacts) {
            for (m in store.thread(c.personaHex)) {
                if (m.outgoing || m.seq in replied) continue
                replied += m.seq
                println("E2E_MSG kind=${m.kind} seq=${m.seq} amt=${m.amountPxmr} " +
                    "eta=${m.etaSecs ?: 0} re=${m.reSeq ?: -1} body=${m.body}")
                // Answer text so the far side can assert the desk speaks; leave
                // money and ceremony kinds as received facts.
                // Never answer an answer: two desks running this same loop
                // would otherwise volley "desk heard" at each other forever.
                if (m.kind == 0 && !m.body.startsWith("[") && !m.body.startsWith("desk heard")) {
                    runCatching {
                        Mailbox.send(context, c, "desk heard: ${m.body}", mine)
                        println("E2E_REPLIED ${m.seq}")
                    }.onFailure { println("e2e: send error ${it.message}") }
                }
            }
        }
        Thread.sleep(4_000)
    }
    nodeStop()
}
