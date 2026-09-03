package org.ducatproject.ducat

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

/**
 * Small groups over pairwise threads (§16.19).
 *
 * A group here is a name, sixteen random bytes, and a member list — nothing
 * else. There is no group key and no shared record: sending fans the same
 * body into each member's existing pairwise thread, so every property a
 * thread has (forward secrecy per pair, unforgeability member-to-member,
 * deniability) arrives unchanged, at the stated cost of one write per member.
 *
 * **The roster is a grow-only set.** Anyone in the group adds; nobody is
 * removed, ever. Removal is the one roster operation that needs a consensus a
 * peer-to-peer group cannot have — nothing can un-tell a phone a group id —
 * where a grow-only set needs none at all: merging two views is a union,
 * unions commute, and every member's roster converges whatever order the
 * adds arrive in.
 *
 * **The mesh is checked here, edge by edge.** Fan-out can only reach the
 * sender's own contacts, so a group works when everyone holds everyone — and
 * nobody can verify anyone else's contact list. Nobody has to: contact edges
 * are mutual, every edge has two ends, and each end checks its own. Every
 * member's local check passing *is* the mesh being complete. [missing] is
 * that local check; sending refuses while it is non-empty and the screen says
 * who rather than dimming a button.
 */
object Groups {
    private const val TAG = "Groups"

    private fun prefs(context: Context) = securePrefs(context, "ducat_groups")

    /** One group, as stored. */
    data class Group(
        val idHex: String,
        val name: String,
        /** Every member's persona hex, ourselves included. */
        val members: List<String>,
        /** Our own counter within the group — the half of (sender, seq)
         *  that names our messages for everyone. */
        val myGroupSeq: Long,
        /** Whether the disclosure has been shown on this phone. */
        val disclosed: Boolean = false,
    )

    fun all(context: Context): List<Group> {
        val raw = prefs(context).getString("groups", null) ?: return emptyList()
        return runCatching {
            val arr = JSONArray(raw)
            (0 until arr.length()).map { i ->
                val o = arr.getJSONObject(i)
                Group(
                    idHex = o.getString("id"),
                    name = o.getString("name"),
                    members = o.getJSONArray("members").let { m ->
                        (0 until m.length()).map { m.getString(it) }
                    },
                    myGroupSeq = o.optLong("my_seq", 0L),
                    disclosed = o.optBoolean("disclosed", false),
                )
            }
        }.getOrElse { emptyList() }
    }

    fun get(context: Context, idHex: String): Group? =
        all(context).firstOrNull { it.idHex == idHex }

    private val lock = Any()

    private fun save(context: Context, groups: List<Group>) {
        val arr = JSONArray()
        groups.forEach { g ->
            arr.put(JSONObject().apply {
                put("id", g.idHex); put("name", g.name)
                put("members", JSONArray(g.members))
                put("my_seq", g.myGroupSeq)
                put("disclosed", g.disclosed)
            })
        }
        prefs(context).edit().putString("groups", arr.toString()).apply()
        ContactStore.bump()
    }

    private fun upsert(context: Context, g: Group) = synchronized(lock) {
        save(context, all(context).filterNot { it.idHex == g.idHex } + g)
    }

    /**
     * Create a group and invite everyone: the first roster *is* the
     * invitation. Members must already be contacts — the screen only offers
     * contacts, and the send below would have nowhere to write otherwise.
     */
    /**
     * Which of our personas a roster names. A group is joined AS somebody —
     * the persona whose pairwise edges carry it — and with more than one
     * persona on the phone, "me" is a per-group fact read from the roster,
     * not a global. Falls back to the primary for a roster that predates
     * the compartments (it can only name the primary anyway).
     */
    private fun myHexIn(context: Context, members: List<String>): String {
        val ours = PersonaStore(context).allHexes()
        return members.firstOrNull { it in ours } ?: PersonaStore(context).personaHex()
    }

    /**
     * Who this phone is in a group — for the screens that offer people to
     * add. A member can only be somebody this persona holds: the roster
     * reaches them down their pairwise thread, signed by whichever of our
     * personas owns that contact, and a roster from one hat naming another
     * as "me" is a member the recipient does not hold — their mesh check
     * then refuses the whole group.
     */
    fun mineIn(context: Context, g: Group): String = myHexIn(context, g.members)

    fun create(context: Context, name: String, memberHexes: List<String>): Group {
        // The doorway: a group made now belongs to the worn persona.
        val mine = PersonaStore(context).worn()
        val id = ByteArray(16).also { java.security.SecureRandom().nextBytes(it) }
        val members = (memberHexes + mine).distinct()
        val g = Group(id.toHexString(), name, members, 0L, disclosed = false)
        upsert(context, g)
        sendRoster(context, g)
        DucatLog.i(TAG, "created ${g.name} with ${members.size} member(s)")
        return g
    }

    /** Add someone: grow the set, tell everyone including the newcomer. */
    fun add(context: Context, idHex: String, personaHex: String) {
        val g = get(context, idHex) ?: return
        if (personaHex in g.members) return
        val grown = g.copy(members = g.members + personaHex)
        upsert(context, grown)
        sendRoster(context, grown)
        DucatLog.i(TAG, "${g.name}: added ${personaHex.take(8)}…")
    }

    /**
     * A roster arrived (kind 12). The admission rule lives here: for a group
     * we already know, only an existing member may grow it — a stranger who
     * somehow learned the id cannot add themselves by telling us a roster.
     * The first roster for an unknown id, from any contact, creates the
     * group: any contact may invite us to a *new* group.
     */
    fun absorbRoster(
        context: Context,
        senderHex: String,
        groupId: ByteArray?,
        payload: ByteArray?,
    ) {
        if (groupId == null || payload == null) return
        val idHex = groupId.toHexString()
        val roster = uniffi.ducat_mobile.groupRosterDecode(payload)
        val members = roster.members.map { it.toHexString() }
        if (senderHex !in members) {
            DucatLog.w(TAG, "roster from ${senderHex.take(8)}… does not include them — ignored")
            return
        }
        val known = get(context, idHex)
        if (known == null) {
            val ours = PersonaStore(context).allHexes()
            if (members.none { it in ours }) {
                // A roster for a group we are not in is somebody else's list.
                DucatLog.w(TAG, "roster for a group we are not in — ignored")
                return
            }
            upsert(context, Group(idHex, roster.name, members, 0L, disclosed = false))
            DucatLog.i(TAG, "joined ${roster.name} (${members.size} member(s))")
            // Being added is the event worth announcing — the roster bytes
            // themselves are machinery. Named by who did it, because "a group
            // appeared" invites exactly the suspicion "who put me in this".
            val adder = ContactStore(context).all()
                .firstOrNull { it.personaHex == senderHex }?.displayName()
                ?: "${senderHex.take(8)}…"
            Notify.post(
                context, roster.name,
                context.getString(R.string.group_added_notify, adder),
            )
            return
        }
        if (senderHex !in known.members) {
            DucatLog.w(TAG, "roster for ${known.name} from a non-member — ignored")
            return
        }
        // Union, never replacement: the set only grows, so a stale roster
        // from a member who has not yet heard of the newest addition cannot
        // shrink anybody's view. The name stays as first learned.
        val merged = (known.members + members).distinct()
        if (merged.size != known.members.size) {
            upsert(context, known.copy(members = merged))
            DucatLog.i(TAG, "${known.name}: roster grew to ${merged.size}")
        }
    }

    /**
     * The roster to everyone on it. Sent with our counter like any group
     * message, so a member holds one ordered stream per sender, roster
     * changes included.
     */
    private fun sendRoster(context: Context, g: Group) {
        val mine = myHexIn(context, g.members)
        val payload = uniffi.ducat_mobile.groupRosterEncode(
            g.name, g.members.map { hexToBytes(it)!! },
        )
        val fresh = get(context, g.idHex) ?: g
        val seq = fresh.myGroupSeq + 1
        upsert(context, fresh.copy(myGroupSeq = seq))
        val store = ContactStore(context)
        for (m in g.members.filter { it != mine }) {
            val c = store.all().firstOrNull { it.personaHex == m }
            if (c == null) {
                // Said, not swallowed. A member this phone does not hold is
                // exactly the mesh gap `missing` exists to report, and
                // dropping them here quietly meant a roster that reached
                // everyone but the person it was about to add.
                DucatLog.w(TAG, "roster: ${m.take(8)}… is not a contact — not sent")
                continue
            }
            runCatching {
                Mailbox.send(
                    context, c, "group: ${g.name}",
                    kind = 12,
                    payload = payload,
                    groupId = hexToBytes(g.idHex),
                    groupSeq = seq,
                )
            }.onFailure {
                queueRetryRoster(context, g.idHex, m, seq)
                DucatLog.w(TAG, "${g.name}: roster to ${c.displayName()} queued (${it.message})")
            }
        }
    }

    private fun queueRetryRoster(context: Context, idHex: String, memberHex: String, seq: Long) =
        synchronized(lock) {
            val arr = retries(context)
            arr.put(JSONObject().apply {
                put("g", idHex); put("m", memberHex); put("roster", true); put("s", seq)
            })
            prefs(context).edit().putString("retry", trimQueue(context, arr).toString()).apply()
        }

    /** The local mesh check: members we do not hold as contacts. */
    fun missing(context: Context, idHex: String): List<String> {
        val g = get(context, idHex) ?: return emptyList()
        val mine = myHexIn(context, g.members)
        val contacts = ContactStore(context).all().map { it.personaHex }.toSet()
        return g.members.filter { it != mine && it !in contacts }
    }

    /**
     * Fan a message out: the same body into each member's pairwise thread,
     * stamped with the group and our own counter. The counter advances once
     * per message, before any send, so a partial failure retries the same
     * (sender, seq) rather than minting a second name for the same words.
     *
     * Refused while our own mesh is incomplete — the caller shows [missing]
     * as names, not a dimmed button. Members whose send fails are queued and
     * retried by the poller; the copies already delivered are identical
     * bytes under the same name, so late delivery cannot fork the group.
     */
    fun send(
        context: Context,
        idHex: String,
        body: String,
        kind: Int = 0,
        reSender: String? = null,
        reSeq: Long? = null,
    ): Boolean {
        val g = get(context, idHex) ?: return false
        val gaps = missing(context, idHex)
        if (gaps.isNotEmpty()) {
            throw IllegalStateException("the group's mesh is incomplete")
        }
        val mine = myHexIn(context, g.members)
        // The counter from what the store says *now*, written under the same
        // lock. `g` was read before the mesh check, and upsert writes the
        // whole group back — so a roster that arrived in between (a member
        // added while this send was being prepared) was dropped, and the
        // member vanished from the group on this device only.
        val seq = synchronized(lock) {
            val fresh = get(context, idHex) ?: g
            val n = fresh.myGroupSeq + 1
            save(context, all(context).filterNot { it.idHex == idHex } + fresh.copy(myGroupSeq = n))
            n
        }
        val store = ContactStore(context)
        var failed = 0
        for (m in g.members.filter { it != mine }) {
            val c = store.all().firstOrNull { it.personaHex == m }
            if (c == null) {
                // Said, not swallowed. Fan-out reaches a member through
                // the pairwise thread, so one this phone does not hold gets
                // no copy at all — the mesh gap `missing` exists to report,
                // arriving here as a member who silently never received it.
                DucatLog.w(TAG, "send: ${m.take(8)}… is not a contact — their copy not written")
                continue
            }
            runCatching {
                Mailbox.send(
                    context, c, body,
                    kind = kind,
                    groupId = hexToBytes(idHex),
                    groupSeq = seq,
                    groupReSender = reSender?.let { hexToBytes(it) },
                    groupReSeq = reSeq,
                )
            }.onFailure {
                failed += 1
                queueRetry(context, idHex, m, body, kind, seq, reSender, reSeq)
                DucatLog.w(TAG, "${g.name}: ${c.displayName()} not reached — queued (${it.message})")
            }
        }
        return failed == 0
    }

    // ---- the retry queue --------------------------------------------------
    //
    // A group message either reaches everyone or its sender knows who is
    // still owed a copy. Sends that fail (their node unreachable, our network
    // down) are parked here and replayed by the poller — same body, same
    // (sender, group_seq), so a copy that finally lands is the same message,
    // not a new one.

    private fun retries(context: Context): JSONArray =
        prefs(context).getString("retry", null)?.let { runCatching { JSONArray(it) }.getOrNull() }
            ?: JSONArray()

    /**
     * How many parked copies one pass may try.
     *
     * The queue was replayed whole, every pass, with a round trip for each
     * entry — so a member whose phone had been off all week cost the poll
     * one timeout per message they had missed, on the same loop that
     * delivers everybody else's mail. A backlog drains over several passes
     * instead, taken in turn so nothing at the back waits for ever.
     */
    private const val RETRIES_PER_PASS = 8

    /** How many are kept at all. Past this the oldest go, the way a
     *  listing keeps only its last few minted cards: the queue is replayed
     *  on every pass for as long as it exists, so unbounded here is
     *  unbounded work as well as unbounded storage. */
    private const val MAX_QUEUED = 200

    /** Where the last pass stopped. */
    private var retryCursor = 0

    /** Newest kept, oldest dropped, and said out loud — a copy quietly
     *  abandoned is a message somebody will never see and never hear about. */
    private fun trimQueue(context: Context, arr: JSONArray): JSONArray {
        if (arr.length() <= MAX_QUEUED) return arr
        var dropped = 0
        while (arr.length() > MAX_QUEUED) { arr.remove(0); dropped++ }
        DucatLog.w(TAG, "retry queue full — $dropped undelivered copy(s) dropped")
        return arr
    }

    private fun queueRetry(
        context: Context, idHex: String, memberHex: String, body: String,
        kind: Int, seq: Long, reSender: String?, reSeq: Long?,
    ) = synchronized(lock) {
        val arr = retries(context)
        arr.put(JSONObject().apply {
            put("g", idHex); put("m", memberHex); put("b", body)
            put("k", kind); put("s", seq)
            reSender?.let { put("rs", it) }; reSeq?.let { put("rq", it) }
        })
        prefs(context).edit().putString("retry", trimQueue(context, arr).toString()).apply()
    }

    /** Poller hook: replay what did not land. Quietly — the queue is the news. */
    fun retryOutbox(context: Context) {
        val arr = retries(context)
        val n = arr.length()
        if (n == 0) return
        val store = ContactStore(context)
        // Read once, not once per entry: `all()` decrypts the whole book,
        // and this loop asked it for every parked copy in the queue.
        val book = store.all()
        val landed = ArrayList<JSONObject>()
        if (retryCursor >= n) retryCursor = 0
        var at = retryCursor
        repeat(minOf(RETRIES_PER_PASS, n)) {
            val o = arr.getJSONObject(at % n)
            at++
            val c = book.firstOrNull { it.personaHex == o.getString("m") }
            if (o.optBoolean("roster")) {
                val g = get(context, o.getString("g"))
                val ok = c != null && g != null && runCatching {
                    Mailbox.send(
                        context, c, "group: ${g.name}",
                        kind = 12,
                        payload = uniffi.ducat_mobile.groupRosterEncode(
                            g.name, g.members.map { hexToBytes(it)!! },
                        ),
                        groupId = hexToBytes(g.idHex),
                        groupSeq = o.getLong("s"),
                    )
                }.isSuccess
                if (ok) landed.add(o)
                return@repeat
            }
            val ok = c != null && runCatching {
                Mailbox.send(
                    context, c, o.getString("b"),
                    kind = o.optInt("k"),
                    groupId = hexToBytes(o.getString("g")),
                    groupSeq = o.getLong("s"),
                    groupReSender = o.optString("rs", "").ifBlank { null }?.let { hexToBytes(it) },
                    groupReSeq = if (o.has("rq")) o.getLong("rq") else null,
                )
            }.isSuccess
            if (ok) {
                landed.add(o)
                DucatLog.i(TAG, "group retry landed for ${o.getString("m").take(8)}…")
            }
        }
        retryCursor = at % n
        if (landed.isEmpty()) return
        // Struck from the queue as it stands now, not the snapshot replayed:
        // the sends above take as long as the network does, and a message
        // that failed to a member meanwhile queued itself behind the
        // snapshot — writing the snapshot back dropped it.
        fun same(a: JSONObject, b: JSONObject) =
            a.optString("g") == b.optString("g") && a.optString("m") == b.optString("m") &&
                a.optLong("s") == b.optLong("s") && a.optBoolean("roster") == b.optBoolean("roster")
        synchronized(lock) {
            val cur = retries(context)
            val keep = JSONArray()
            for (i in 0 until cur.length()) {
                val o = cur.getJSONObject(i)
                if (landed.none { same(it, o) }) keep.put(o)
            }
            prefs(context).edit().putString("retry", keep.toString()).apply()
        }
    }

    /** One row of the merged view: who said it, and the copy that carried it. */
    data class Row(val senderHex: String, val message: StoredMessage)

    /**
     * The merged view: every stored copy with this group id, one row per
     * (sender, group_seq) — my own N outbox copies collapse to one, and an
     * inbound copy's sender is the member whose pairwise thread it arrived
     * in, which is the one fact fan-out makes unforgeable.
     */
    fun thread(context: Context, idHex: String): List<Row> {
        val g = get(context, idHex) ?: return emptyList()
        val store = ContactStore(context)
        return merge(context, g) { store.thread(it) }
    }

    /**
     * [thread] over threads already in hand. The chat list decodes every
     * visible conversation once per store bump to sort them; asking each
     * group to decode its members again on top was one decrypt per member
     * per group per bump, on the tab that is open most.
     */
    fun merge(context: Context, g: Group, threadOf: (String) -> List<StoredMessage>): List<Row> {
        val mine = myHexIn(context, g.members)
        val seen = HashSet<Pair<String, Long>>()
        val out = ArrayList<Row>()
        for (m in g.members.filter { it != mine }) {
            for (msg in threadOf(m)) {
                if (msg.groupId != g.idHex) continue
                // The roster is machinery, not conversation: its effect is the
                // member count in the top bar, and a bubble reading
                // "group: name" is internal words about nothing a person can
                // act on — the same reasoning that keeps ceremony kinds out
                // of the pairwise view.
                if (msg.kind == 12) continue
                val sender = if (msg.outgoing) mine else m
                if (!seen.add(sender to msg.groupSeq)) continue
                out.add(Row(sender, msg))
            }
        }
        return out.sortedWith(compareBy({ it.message.timestamp }, { it.message.groupSeq }))
    }

    /** Mark the disclosure shown, once. */
    fun markDisclosed(context: Context, idHex: String) {
        val g = get(context, idHex) ?: return
        if (!g.disclosed) upsert(context, g.copy(disclosed = true))
    }

    // --- read marks ---------------------------------------------------------
    //
    // A group had no notion of having been looked at. Its rows arrive in the
    // members' pairwise threads, so what a group message raised was the
    // *sender's* direct row: Sam posting in the ladder crew put the dot and
    // the tab badge on Sam, whose conversation then opened on nothing new,
    // while the group row — where the words were — showed no change. The
    // pairwise mark now steps over group rows (see ContactStore
    // .appendAndAdvance) and the group keeps a mark of its own here.
    //
    // The mark is each member's group counter as of the last look — the
    // half of (sender, group_seq) that names their messages for everyone,
    // which only ever climbs. Not the pairwise seq (it restarts with every
    // fresh card) and not a timestamp (their clock).
    //
    // **A maximum alone is not enough, though.** A group message is fanned
    // out per member, so one member's copy can fail while the rest land, and
    // the sender's retry arrives carrying its original counter — a number
    // *below* a mark this group has already been looked at. Nobody here has
    // ever seen those words, and the max says nothing changed: they land in
    // the middle of the thread, above everything already read past, silent.
    //
    // So a look also records how many of each member's words were on the
    // phone at the time. A raw count was rejected here once, for a good
    // reason — rows leave, a retention window sweeps them on every poll, a
    // long press deletes one, and a count that *moved* would raise the dot
    // over nothing said. Two things make it safe now. It is counted off the
    // merged view, which is already deduplicated on (sender, group_seq), so
    // a retry of something we have is not a row. And it is only ever read as
    // "more than last time": a sweep lowers it, the next look writes the
    // lower number down, and nothing is flagged either way.

    /** What one look at a group recorded. */
    data class Look(
        /** Everybody else's newest word, by who said it: their group counter. */
        val high: Map<String, Long>,
        /** How many of their words were on this phone at the time. */
        val rows: Map<String, Long>,
    )

    /** Read [rows] — a merged group view — as a look at it. */
    fun lookAt(context: Context, rows: List<Row>): Look {
        val ours = PersonaStore(context).allHexes()
        val high = HashMap<String, Long>()
        val count = HashMap<String, Long>()
        for (r in rows) {
            if (r.senderHex in ours) continue
            high[r.senderHex] = maxOf(high[r.senderHex] ?: 0L, r.message.groupSeq)
            count[r.senderHex] = (count[r.senderHex] ?: 0L) + 1
        }
        return Look(high, count)
    }

    private fun marks(context: Context, key: String): Map<String, Long> =
        prefs(context).getString(key, null)?.let { raw ->
            runCatching {
                val o = JSONObject(raw)
                o.keys().asSequence().associateWith { o.getLong(it) }
            }.getOrNull()
        } ?: emptyMap()

    /** The look last taken; empty for a group never opened. */
    fun seenLook(context: Context, idHex: String): Look =
        Look(marks(context, "seen_$idHex"), marks(context, "rows_$idHex"))

    /**
     * Whether anybody has said something since the last look.
     *
     * The counts are consulted only for a member the last look actually
     * recorded one for. A phone upgrading into this has marks but no
     * counts, and reading absent as zero would flag every group it has
     * ever been in, once, for nothing — the first look on each writes the
     * count and the check starts working from there.
     */
    fun unread(seen: Look, now: Look): Boolean =
        now.high.any { (m, s) -> s > (seen.high[m] ?: 0L) } ||
            now.rows.any { (m, n) -> seen.rows[m]?.let { n > it } == true }

    /**
     * Looking at the group is what "seen" means, as for a thread. A mark
     * never comes down: what was looked at stays looked at even after the
     * rows that carried it have been swept.
     *
     * The counts beside them do the opposite — they are written as they are
     * found, sweep and all. They mean "this many of their words were here
     * when it was looked at", so a mark held above a sweep would leave a
     * number no future count can exceed, and the next gap filled in under
     * it would go unannounced.
     */
    fun markSeen(context: Context, idHex: String, now: Look) {
        val seen = seenLook(context, idHex)
        val merged = (seen.high.keys + now.high.keys)
            .associateWith { maxOf(seen.high[it] ?: 0L, now.high[it] ?: 0L) }
        if (merged == seen.high && now.rows == seen.rows) return
        prefs(context).edit()
            .putString("seen_$idHex", JSONObject(merged).toString())
            .putString("rows_$idHex", JSONObject(now.rows).toString())
            .apply()
        ContactStore.bump()
    }

    /** Groups with something unlooked-at, for the tab badge. */
    fun unreadGroups(context: Context): Int = unreadGroupIds(context).size

    /**
     * The same groups, each under the hat that is in it ([mineIn]) — the
     * drawer's per-persona chips, which have to add up to the tab badge.
     */
    fun unreadGroupsByOwner(context: Context): Map<String, Int> {
        val unread = unreadGroupIds(context)
        if (unread.isEmpty()) return emptyMap()
        return all(context).filter { it.idHex in unread }
            .groupingBy { mineIn(context, it) }.eachCount()
    }

    private fun unreadGroupIds(context: Context): Set<String> {
        val groups = all(context)
        if (groups.isEmpty()) return emptySet()
        val store = ContactStore(context)
        val threads = HashMap<String, List<StoredMessage>>()
        return groups.filter { g ->
            val rows = merge(context, g) { hex -> threads.getOrPut(hex) { store.thread(hex) } }
            unread(seenLook(context, g.idHex), lookAt(context, rows))
        }.mapTo(HashSet()) { it.idHex }
    }
}
