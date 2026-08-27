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
    fun create(context: Context, name: String, memberHexes: List<String>): Group {
        val mine = PersonaStore(context).personaHex()
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
            val mine = PersonaStore(context).personaHex()
            if (mine !in members) {
                // A roster for a group we are not in is somebody else's list.
                DucatLog.w(TAG, "roster for a group we are not in — ignored")
                return
            }
            upsert(context, Group(idHex, roster.name, members, 0L, disclosed = false))
            DucatLog.i(TAG, "joined ${roster.name} (${members.size} member(s))")
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
        val mine = PersonaStore(context).personaHex()
        val payload = uniffi.ducat_mobile.groupRosterEncode(
            g.name, g.members.map { hexToBytes(it)!! },
        )
        val fresh = get(context, g.idHex) ?: g
        val seq = fresh.myGroupSeq + 1
        upsert(context, fresh.copy(myGroupSeq = seq))
        val store = ContactStore(context)
        for (m in g.members.filter { it != mine }) {
            val c = store.all().firstOrNull { it.personaHex == m } ?: continue
            runCatching {
                Mailbox.send(
                    context, c, "group: ${g.name}", mine,
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
            prefs(context).edit().putString("retry", arr.toString()).apply()
        }

    /** The local mesh check: members we do not hold as contacts. */
    fun missing(context: Context, idHex: String): List<String> {
        val g = get(context, idHex) ?: return emptyList()
        val mine = PersonaStore(context).personaHex()
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
        val mine = PersonaStore(context).personaHex()
        val seq = g.myGroupSeq + 1
        upsert(context, g.copy(myGroupSeq = seq))
        val store = ContactStore(context)
        var failed = 0
        for (m in g.members.filter { it != mine }) {
            val c = store.all().firstOrNull { it.personaHex == m } ?: continue
            runCatching {
                Mailbox.send(
                    context, c, body, mine,
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
        prefs(context).edit().putString("retry", arr.toString()).apply()
    }

    /** Poller hook: replay what did not land. Quietly — the queue is the news. */
    fun retryOutbox(context: Context) {
        val arr = retries(context)
        if (arr.length() == 0) return
        val mine = PersonaStore(context).personaHex()
        val store = ContactStore(context)
        val keep = JSONArray()
        for (i in 0 until arr.length()) {
            val o = arr.getJSONObject(i)
            val c = store.all().firstOrNull { it.personaHex == o.getString("m") }
            if (o.optBoolean("roster")) {
                val g = get(context, o.getString("g"))
                val ok = c != null && g != null && runCatching {
                    Mailbox.send(
                        context, c, "group: ${g.name}", mine,
                        kind = 12,
                        payload = uniffi.ducat_mobile.groupRosterEncode(
                            g.name, g.members.map { hexToBytes(it)!! },
                        ),
                        groupId = hexToBytes(g.idHex),
                        groupSeq = o.getLong("s"),
                    )
                }.isSuccess
                if (!ok) keep.put(o)
                continue
            }
            val ok = c != null && runCatching {
                Mailbox.send(
                    context, c, o.getString("b"), mine,
                    kind = o.optInt("k"),
                    groupId = hexToBytes(o.getString("g")),
                    groupSeq = o.getLong("s"),
                    groupReSender = o.optString("rs", "").ifBlank { null }?.let { hexToBytes(it) },
                    groupReSeq = if (o.has("rq")) o.getLong("rq") else null,
                )
            }.isSuccess
            if (!ok) keep.put(o)
            else DucatLog.i(TAG, "group retry landed for ${o.getString("m").take(8)}…")
        }
        synchronized(lock) {
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
        val mine = PersonaStore(context).personaHex()
        val seen = HashSet<Pair<String, Long>>()
        val out = ArrayList<Row>()
        for (m in g.members.filter { it != mine }) {
            for (msg in store.thread(m)) {
                if (msg.groupId != idHex) continue
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
}
