package org.ducatproject.ducat.ui

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import org.ducatproject.ducat.ContactStore
import org.ducatproject.ducat.withoutDisplayHazards
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ConcurrentLinkedQueue

/**
 * A thread's sends in flight, held by the process rather than by the screen.
 *
 * Every send from the conversation screen ran on the screen's own coroutine
 * scope, so the screen going away — Back, a rotation, a call arriving on
 * top — cancelled the coroutine at its next suspension point. The write
 * itself is a blocking call under [org.ducatproject.ducat.Mailbox]'s lock
 * and always ran to the end; what was cancelled was everything after it.
 * For a text send that meant the draft was never cleared: the words that
 * had just gone out were saved as the draft on dispose, restored on the
 * way back in, and offered up again under a Send button — two copies of
 * "on my way", numbered one after the other. For a photo it meant the bar
 * and the busy button were gone while the upload went on, so the picture
 * could be picked a second time.
 *
 * Here a send outlives the screen that started it. What a screen needs
 * back — whether anything is still going, how far along, what landed and
 * what failed — it reads from this object whenever [ticks] moves, and the
 * screen instance that is up when the answer arrives is the one that shows
 * it, whether or not it is the one that asked.
 *
 * The saved draft is the other half. A screen disposed mid-send saves its
 * composer, which still holds the words on their way; [owns] lets it save
 * an empty draft instead, and a send that turns out never to have left the
 * phone puts the words back ([Outcome.Failed.body]) rather than lose them.
 */
internal object ThreadSends {
    sealed interface Outcome {
        /**
         * A text send whose words are in the thread now. Delivered — or
         * persisted as "not sent yet", which the poll delivers later: a
         * send commits its row before the network write, so a failure
         * after that point has still put the words in a bubble. Either
         * way the composer is done with them; kept there, they went out a
         * second time under a second number.
         */
        data class Landed(val body: String) : Outcome

        /**
         * Nothing left the phone. [what] is the thing that did not go, in
         * the user's words, or null for the actions that name themselves;
         * [body] is a text send's words, for the composer to take back.
         */
        data class Failed(val error: Throwable, val what: String?, val body: String?) : Outcome
    }

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val counts = ConcurrentHashMap<String, Int>()
    private val progress = ConcurrentHashMap<String, Pair<Int, Int>>()
    /** The text on its way, per contact. The composer refuses a second
     *  while the first is going, so one is the most there is. */
    private val bodies = ConcurrentHashMap<String, String>()
    private val outcomes = ConcurrentHashMap<String, ConcurrentLinkedQueue<Outcome>>()
    private val _ticks = MutableStateFlow(0L)

    /** Moves whenever anything below changes. */
    val ticks: StateFlow<Long> = _ticks

    private fun tick() = _ticks.update { it + 1 }

    /** Whether a send to this contact is still under way. */
    fun inFlight(personaHex: String): Boolean = (counts[personaHex] ?: 0) > 0

    /** (done, total) chunks of an attachment on its way, or null. */
    fun progress(personaHex: String): Pair<Int, Int>? = progress[personaHex]

    /**
     * Whether these words are this object's rather than the composer's:
     * on their way, or landed and not yet shown. A screen disposed while
     * they are saves an empty draft, not them.
     */
    fun owns(personaHex: String, text: String): Boolean {
        val t = text.trim()
        if (t.isEmpty()) return false
        if (bodies[personaHex] == t) return true
        return outcomes[personaHex]?.any { it is Outcome.Landed && it.body == t } ?: false
    }

    /** What has finished since the last look, oldest first. Taken, not
     *  read: whichever screen asks first shows it, and only once. */
    fun take(personaHex: String): List<Outcome> {
        val q = outcomes[personaHex] ?: return emptyList()
        return generateSequence { q.poll() }.toList()
    }

    /**
     * Run one send off the screen.
     *
     * [body] is the composer's text for a text send, already trimmed, so
     * the outcome can say it landed; null for everything else. [block]
     * does the sending, and is handed a progress callback for attachments.
     * Failures come back as [Outcome.Failed] rather than being thrown —
     * nobody is awaiting this.
     */
    fun launch(
        store: ContactStore,
        personaHex: String,
        what: String?,
        body: String? = null,
        block: (progress: (Int, Int) -> Unit) -> Unit,
    ) {
        counts.merge(personaHex, 1, Int::plus)
        if (body != null) bodies[personaHex] = body
        tick()
        scope.launch {
            val seqBefore = if (body != null) {
                store.all().firstOrNull { it.personaHex == personaHex }?.outSeq ?: 0L
            } else 0L
            val r = runCatching {
                block { done, total ->
                    progress[personaHex] = done to total
                    tick()
                }
            }
            val outcome = r.fold(
                onSuccess = { body?.let(Outcome::Landed) },
                onFailure = { e ->
                    // The row is what says whether the words left the
                    // composer: Mailbox.send persists it before the network
                    // write, cleaned the way the wire will carry it, so a
                    // failure that left one behind is a message in the
                    // thread marked "not sent yet" — and the draft must not
                    // send it again. Same test TabStore.close uses for its
                    // closing word.
                    val landed = body != null && store.thread(personaHex)
                        .lastOrNull { it.outgoing }
                        ?.let {
                            it.seq >= seqBefore && it.kind == 0 && !it.delivered &&
                                it.body == withoutDisplayHazards(body)
                        }
                        ?: false
                    if (landed) Outcome.Landed(body!!) else Outcome.Failed(e, what, body)
                },
            )
            if (body != null) {
                // The saved draft, for a screen that is not up to hear
                // this — or a process that will not be. A disposed screen
                // saved these words before the answer came (owns() covers
                // the dispose that comes after), and a send that never
                // left must not have cost them.
                val saved = store.draftOf(personaHex)
                when (outcome) {
                    is Outcome.Landed -> if (saved.trim() == body) store.saveDraft(personaHex, "")
                    is Outcome.Failed -> if (saved.isBlank()) store.saveDraft(personaHex, body)
                    null -> Unit
                }
            }
            outcome?.let { outcomes.getOrPut(personaHex) { ConcurrentLinkedQueue() }.add(it) }
            if (body != null) bodies.remove(personaHex, body)
            counts.merge(personaHex, -1, Int::plus)
            progress.remove(personaHex)
            tick()
        }
    }
}
