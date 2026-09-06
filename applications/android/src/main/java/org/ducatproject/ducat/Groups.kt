package org.ducatproject.ducat

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import uniffi.ducat_mobile.GroupBoardCreate
import uniffi.ducat_mobile.GroupBoardOut
import uniffi.ducat_mobile.GroupBoardSpec
import uniffi.ducat_mobile.GroupEntryOut
import uniffi.ducat_mobile.GroupPageSeal

/**
 * Small groups over pairwise threads (§16.19), and the board a group rides
 * on when it has one (§16.24).
 *
 * A group here is a name, sixteen random bytes, and a member list — nothing
 * else. Without a board there is no group key and no shared record: sending
 * fans the same body into each member's existing pairwise thread, so every
 * property a thread has (forward secrecy per pair, unforgeability
 * member-to-member, deniability) arrives unchanged, at the stated cost of one
 * write per member.
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
 *
 * **A board replaces the fan-out with one record** (§16.24): every member
 * writes their own ring of pages on it, signed by their own persona key, and
 * everyone reads the same record. A message is one write, a lap is one
 * inspection, a newcomer reads the recent pages, and the mesh is no longer
 * needed to speak — [missing] is empty for a board group. Growing the group
 * is a new record (a *generation*) with a fresh group key, formed by whoever
 * adds and carried to everyone on the roster. What it trades away — forward
 * secrecy per generation rather than per message, and words a persona key
 * signs rather than deniable ones — the disclosure states plainly.
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
        /** The board this group's words ride on (§16.24); a group without
         *  one fans out over pairwise threads as §16.19 says. */
        val board: Board? = null,
        /** Left on this phone: not read, not shown, not written to. The
         *  roster is grow-only, so nobody else learns; a roster that moves
         *  the group to a newer board clears it. */
        val left: Boolean = false,
    )

    /**
     * One generation of a group's board (§16.24): the record, its key, and
     * where this member's own pen is.
     */
    data class Board(
        val generation: Long,
        /** The persona that formed this generation — the record's owner. */
        val owner: String,
        /** The record key; computed from the roster, cached here. Empty
         *  until the node has been asked — a roster can arrive while it is
         *  away. */
        val key: String = "",
        val groupKeyHex: String,
        val pages: Int,
        /** The page of my ring I am writing. */
        val page: Int = 0,
        /** My current page has words the record does not hold yet. */
        val dirty: Boolean = false,
        /** The sequence last read, per subkey. */
        val seen: Map<String, Long> = emptyMap(),
    )

    /**
     * An entry of my current page, as kept on disk so a page can be sealed
     * again after a restart or a failed write.
     */
    private data class PageEntry(
        val s: Long,
        val t: Long,
        val k: Int,
        val b: String?,
        val rs: String?,
        val rq: Long?,
    ) {
        fun out() = GroupEntryOut(
            seq = s.toULong(),
            ts = t.toULong(),
            kind = k.toUInt(),
            body = b,
            reSender = rs?.let { hexToBytes(it) },
            reSeq = rq?.toULong(),
        )
    }

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
                    board = o.optJSONObject("board")?.let(::boardFrom),
                    left = o.optBoolean("left", false),
                )
            }
        }.getOrElse { emptyList() }
    }

    // The same shape the desk keeps (app/src/groups.rs): gen, owner, key,
    // gkey, pages, page, dirty, seen.
    private fun boardFrom(o: JSONObject): Board = Board(
        generation = o.optLong("gen", 0L),
        owner = o.optString("owner", ""),
        key = o.optString("key", ""),
        groupKeyHex = o.optString("gkey", ""),
        pages = o.optInt("pages", 0),
        page = o.optInt("page", 0),
        dirty = o.optBoolean("dirty", false),
        seen = o.optJSONObject("seen")?.let { s ->
            s.keys().asSequence().associateWith { s.optLong(it) }
        } ?: emptyMap(),
    )

    private fun Board.toJson(): JSONObject = JSONObject().apply {
        put("gen", generation); put("owner", owner); put("key", key); put("gkey", groupKeyHex)
        put("pages", pages); put("page", page)
        if (dirty) put("dirty", true)
        put("seen", JSONObject(seen))
    }

    fun get(context: Context, idHex: String): Group? =
        all(context).firstOrNull { it.idHex == idHex }

    /** The groups a list shows: the ones not left. */
    fun visible(context: Context): List<Group> = all(context).filter { !it.left }

    /** Leave a group here: kept, marked, and skipped by every sweep and
     *  list. A fresh generation from the others brings it back. */
    fun leave(context: Context, idHex: String) {
        val g = get(context, idHex) ?: return
        DucatLog.i(TAG, "${g.name}: left")
        upsert(context, g.copy(left = true))
    }

    private val lock = Any()

    private fun save(context: Context, groups: List<Group>) {
        val arr = JSONArray()
        groups.forEach { g ->
            arr.put(JSONObject().apply {
                put("id", g.idHex); put("name", g.name)
                put("members", JSONArray(g.members))
                put("my_seq", g.myGroupSeq)
                put("disclosed", g.disclosed)
                if (g.left) put("left", true)
                g.board?.let { put("board", it.toJson()) }
            })
        }
        prefs(context).edit().putString("groups", arr.toString()).apply()
        ContactStore.bump()
    }

    private fun upsert(context: Context, g: Group) = synchronized(lock) {
        // In place: the chat list shows groups in stored order (it sorts
        // only the pairwise threads below them), so appending on every
        // update walked a group to the bottom of its section each time
        // anything about it changed.
        val cur = all(context)
        save(
            context,
            if (cur.none { it.idHex == g.idHex }) {
                cur + g
            } else {
                cur.map { if (it.idHex == g.idHex) g else it }
            },
        )
    }

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

    /**
     * Create a group and invite everyone: the first roster *is* the
     * invitation. Members must already be contacts — the screen only offers
     * contacts, and the roster below would have nowhere to write otherwise.
     *
     * The board first: a group made here rides on one from its first word
     * (§16.24). Forming it needs the node, so a group cannot be made
     * offline — and one that could not be formed is not saved at all.
     */
    fun create(context: Context, name: String, memberHexes: List<String>): Group = try {
        // The doorway: a group made now belongs to the worn persona.
        val mine = PersonaStore(context).worn()
        val id = ByteArray(16).also { java.security.SecureRandom().nextBytes(it) }
        val members = (memberHexes + mine).distinct()
        // Two waits, each said as it starts (Busy): the record formed on
        // the DHT, then the sealed roster down every member's thread. Said
        // here rather than inside the helpers, which the poll and `add`
        // also call with nobody's button held down.
        Busy.say(context.getString(R.string.group_forming_board))
        val board = formBoard(context, id.toHexString(), members, 1L)
        val g = Group(id.toHexString(), name, members, 0L, disclosed = false, board = board)
        upsert(context, g)
        Busy.say(context.getString(R.string.group_telling_members))
        sendRoster(context, g)
        DucatLog.i(TAG, "created ${g.name} with ${members.size} member(s), on a board")
        g
    } finally {
        Busy.clear()
    }

    /**
     * Add someone: grow the set, tell everyone including the newcomer.
     *
     * A different member list is a different record: the adder forms the
     * next generation and tells everyone, the newcomer included. A group
     * that had no board gets its first one here.
     */
    fun add(context: Context, idHex: String, personaHex: String) {
        val g = get(context, idHex) ?: return
        if (personaHex in g.members) return
        val members = g.members + personaHex
        val next = g.board?.let { it.generation + 1 } ?: 1L
        val grown = g.copy(members = members, board = formBoard(context, g.idHex, members, next))
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
     *
     * The board it names (§16.24): a higher generation moves us to that
     * record; an equal one from a different owner is the tie the section
     * breaks by the lower owner key, and if the losing record was ours, the
     * next generation is formed here with the union of both rosters.
     */
    fun absorbRoster(
        context: Context,
        senderHex: String,
        groupId: ByteArray?,
        payload: ByteArray?,
    ) {
        if (groupId == null || payload == null) return
        val idHex = groupId.toHexString()
        val short = "${senderHex.take(8)}…"
        val roster = runCatching { uniffi.ducat_mobile.groupRosterDecode(payload) }
            .getOrElse {
                DucatLog.w(TAG, "roster from $short does not decode — ignored")
                return
            }
        val members = roster.members.map { it.toHexString() }
        if (senderHex !in members) {
            DucatLog.w(TAG, "roster from $short does not include them — ignored")
            return
        }
        // As this phone would hold it: nothing read yet, the record key
        // still to be computed once the node is asked.
        val incoming = roster.board?.let { b ->
            Board(
                generation = b.generation.toLong(),
                owner = b.owner.toHexString(),
                key = "",
                groupKeyHex = b.groupKey.toHexString(),
                pages = b.pages.toInt(),
            )
        }
        val ours = PersonaStore(context).allHexes()
        val known = get(context, idHex)
        if (known == null) {
            if (members.none { it in ours }) {
                // A roster for a group we are not in is somebody else's list.
                DucatLog.w(TAG, "roster for a group we are not in — ignored")
                return
            }
            upsert(context, Group(idHex, roster.name, members, 0L, disclosed = false, board = incoming))
            // Everything on the board is unread: due on the next sweep.
            touch(idHex)
            // Being added is the event worth announcing — the roster bytes
            // themselves are machinery. Named by who did it, because "a group
            // appeared" invites exactly the suspicion "who put me in this".
            val adder = ContactStore(context).all()
                .firstOrNull { it.personaHex == senderHex }?.displayName()
                ?: short
            DucatLog.i(TAG, "joined ${roster.name} (${members.size} member(s)) — added by $adder")
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
        // The board: a higher generation moves us; an equal one from a
        // different owner is the tie §16.24 breaks by the lower owner key,
        // and the loser re-forms with the union.
        var board = known.board
        var lost = false
        if (incoming != null) {
            val k = known.board
            when {
                k == null -> board = incoming
                incoming.generation > k.generation -> board = incoming
                incoming.generation == k.generation && incoming.owner != k.owner -> {
                    if (incoming.owner < k.owner) {
                        lost = k.owner in ours
                        board = incoming
                    } else {
                        lost = incoming.owner in ours
                    }
                }
                else -> {}
            }
        }
        val moved = board?.generation != known.board?.generation ||
            board?.owner != known.board?.owner
        if (merged.size != known.members.size || moved) {
            val n = merged.size
            upsert(context, known.copy(members = merged, board = board, left = known.left && !moved))
            if (moved) {
                // A new generation is a new record: everything on it is unread.
                touch(idHex)
                DucatLog.i(TAG, "${known.name}: on generation ${board?.generation ?: 0L} now, $n member(s)")
            } else {
                DucatLog.i(TAG, "${known.name}: roster grew to $n")
            }
        }
        if (lost) {
            // Two of us formed the same generation; mine lost the tie. The
            // next one carries everyone.
            val next = (get(context, idHex)?.board?.generation ?: 0L) + 1
            runCatching { formBoard(context, idHex, merged, next) }.fold(
                onSuccess = { b ->
                    get(context, idHex)?.let { g ->
                        val formed = g.copy(board = b)
                        upsert(context, formed)
                        sendRoster(context, formed)
                    }
                },
                onFailure = {
                    DucatLog.w(TAG, "${known.name}: could not re-form after a tie: ${it.message}")
                },
            )
        }
    }

    /** The roster as it crosses the wire: name, members, and the board when there is one. */
    private fun rosterPayload(g: Group): ByteArray {
        val members = g.members.map {
            hexToBytes(it) ?: throw IllegalStateException("a member key is not hex")
        }
        val board = g.board?.let { b ->
            GroupBoardOut(
                generation = b.generation.toULong(),
                owner = hexToBytes(b.owner) ?: throw IllegalStateException("a board owner is a persona key"),
                groupKey = hexToBytes(b.groupKeyHex) ?: throw IllegalStateException("a group key is hex"),
                pages = b.pages.toUInt(),
            )
        }
        return uniffi.ducat_mobile.groupRosterEncode(g.name, members, board)
    }

    /**
     * The roster to everyone on it. Sent with our counter like any group
     * message, so a member holds one ordered stream per sender, roster
     * changes included. Pairwise even for a board group: the roster is what
     * carries the group key (§16.24).
     */
    private fun sendRoster(context: Context, g: Group) {
        val mine = myHexIn(context, g.members)
        val payload = runCatching { rosterPayload(g) }.getOrElse {
            DucatLog.w(TAG, "${g.name}: roster does not encode — not sent (${it.message})")
            return
        }
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

    /**
     * The local mesh check: members we do not hold as contacts. Empty for a
     * board group — one write reaches everyone, so no mesh is needed.
     */
    fun missing(context: Context, idHex: String): List<String> {
        val g = get(context, idHex) ?: return emptyList()
        if (g.board != null) return emptyList()
        val mine = myHexIn(context, g.members)
        val contacts = ContactStore(context).all().map { it.personaHex }.toSet()
        return g.members.filter { it != mine && it !in contacts }
    }

    /**
     * Say something to the group. On a board (§16.24) that is one write —
     * see [sendOnBoard]. Otherwise fan a message out: the same body into
     * each member's pairwise thread, stamped with the group and our own
     * counter. The counter advances once per message, before any send, so
     * a partial failure retries the same (sender, seq) rather than minting
     * a second name for the same words.
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
        val (seq, fresh) = synchronized(lock) {
            val fresh = get(context, idHex) ?: g
            val n = fresh.myGroupSeq + 1
            val advanced = fresh.copy(myGroupSeq = n)
            save(context, all(context).map { if (it.idHex == idHex) advanced else it })
            n to advanced
        }
        // The board, as the store holds it now — a generation that moved
        // while the words were being typed is written to, not the old one.
        fresh.board?.let { board ->
            return sendOnBoard(context, fresh, board, mine, seq, body, kind, reSender, reSeq)
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

    // ---- the board (§16.24) ------------------------------------------------
    //
    // One DHT record under the SMPL schema: the owner (whoever formed this
    // generation) writes subkeys 0 to PAGES-1, every other member — in
    // ascending key order — the PAGES after. A member's subkeys are a ring
    // of pages; a page is a strict-reader object sealed under the
    // generation's group key with the record key and subkey as associated
    // data, so it cannot be moved. Reading is one inspection (every
    // subkey's sequence in one call) and then only the pages that moved.

    /** Sealing adds a nonce and a tag; a page must fit under the cap with them. */
    private const val SEAL_OVERHEAD = 40

    /** Unsigned, byte by byte — the order the record's schema lists members in. */
    private val byteOrder = Comparator<ByteArray> { a, b ->
        val n = minOf(a.size, b.size)
        for (i in 0 until n) {
            val d = (a[i].toInt() and 0xff) - (b[i].toInt() and 0xff)
            if (d != 0) return@Comparator d
        }
        a.size - b.size
    }

    /**
     * The shape a board's record takes from its roster: owner first, every
     * other member in ascending key order — the same on every phone, so the
     * same record key.
     */
    /**
     * The shape a board's record takes from its roster: owner first, the
     * nameplate, then every other member in ascending key order — the same
     * on every phone, so the same record key.
     */
    private fun boardSpec(idHex: String, members: List<String>, board: Board): GroupBoardSpec {
        val ownerPublic = hexToBytes(board.owner)
            ?: throw IllegalStateException("a board owner is a persona key")
        val groupId = hexToBytes(idHex) ?: throw IllegalStateException("a group id is hex")
        val others = members.filter { it != board.owner }
            .mapNotNull { hexToBytes(it) }
            .sortedWith(byteOrder)
            .distinctBy { it.toHexString() }
        if (others.size + 1 > 255) throw IllegalStateException("a board holds at most 255 members")
        val nameplate = uniffi.ducat_mobile.groupBoardNameplate(groupId, board.generation.toULong())
        return GroupBoardSpec(ownerPublic, nameplate, others, board.pages.toUInt())
    }

    /** Subkeys on the record: the owner's pages, the nameplate's one, and a ring per other member. */
    private fun boardSubkeys(spec: GroupBoardSpec): UInt =
        spec.pages * (1u + spec.members.size.toUInt()) + 1u

    /** The first subkey of a member's ring, or null for a key not on the board. */
    private fun boardBase(spec: GroupBoardSpec, member: ByteArray): UInt? {
        if (member.contentEquals(spec.ownerPublic)) return 0u
        val i = spec.members.indexOfFirst { it.contentEquals(member) }
        return if (i < 0) null else spec.pages * (1u + i.toUInt()) + 1u
    }

    /** Whose ring a subkey is in; null for the nameplate's. */
    private fun boardMemberOf(spec: GroupBoardSpec, subkey: UInt): ByteArray? {
        val pages = maxOf(spec.pages, 1u)
        if (subkey < pages) return spec.ownerPublic
        if (subkey == pages) return null
        return spec.members.getOrNull(((subkey - pages - 1u) / pages).toInt())
    }

    /**
     * Form a generation: mint its key, create its record as the member
     * whose persona is in this roster.
     */
    private fun formBoard(context: Context, idHex: String, members: List<String>, generation: Long): Board {
        val mine = myHexIn(context, members)
        val secret = PersonaStore(context).secretFor(mine)
            ?: throw IllegalStateException("no secret for my persona in this group")
        val draft = Board(
            generation = generation,
            owner = mine,
            key = "",
            groupKeyHex = uniffi.ducat_mobile.randomBytes(32u).toHexString(),
            pages = uniffi.ducat_mobile.groupBoardPages().toInt(),
        )
        val spec = boardSpec(idHex, members, draft)
        val key = uniffi.ducat_mobile.nodeDhtBoardCreate(GroupBoardCreate(spec, secret))
        return draft.copy(key = key)
    }

    /**
     * The board's record key, computed from the roster when a roster
     * arrived while the node was away — and kept, once known.
     */
    /**
     * The board's record key: computed from the roster every time (a local
     * derivation, no network), and the cache refreshed when it differs — a
     * client that once computed it under an older layout heals itself on
     * its next read instead of watching an empty record for ever.
     */
    private fun boardKey(context: Context, g: Group, board: Board): String {
        val key = uniffi.ducat_mobile.nodeDhtBoardKey(boardSpec(g.idHex, g.members, board))
        if (key != board.key) {
            get(context, g.idHex)?.let { fresh ->
                val b = fresh.board
                if (b != null && b.generation == board.generation) {
                    upsert(context, fresh.copy(board = b.copy(key = key, seen = emptyMap())))
                }
            }
        }
        return key
    }

    private fun myPage(context: Context, idHex: String): List<PageEntry> =
        prefs(context).getString("page_$idHex", null)?.let { raw ->
            runCatching {
                val arr = JSONArray(raw)
                (0 until arr.length()).map { i ->
                    val o = arr.getJSONObject(i)
                    PageEntry(
                        s = o.getLong("s"),
                        t = o.getLong("t"),
                        k = o.optInt("k", 0),
                        b = if (o.has("b") && !o.isNull("b")) o.getString("b") else null,
                        rs = o.optString("rs", "").ifBlank { null },
                        rq = if (o.has("rq") && !o.isNull("rq")) o.getLong("rq") else null,
                    )
                }
            }.getOrNull()
        } ?: emptyList()

    private fun saveMyPage(context: Context, idHex: String, entries: List<PageEntry>) {
        val arr = JSONArray()
        entries.forEach { e ->
            arr.put(JSONObject().apply {
                put("s", e.s); put("t", e.t); put("k", e.k)
                e.b?.let { put("b", it) }
                e.rs?.let { put("rs", it) }
                e.rq?.let { put("rq", it) }
            })
        }
        prefs(context).edit().putString("page_$idHex", arr.toString()).apply()
    }

    /** The record, opened with my own key so my subkeys take my writes. Returns (record key, my hex). */
    private fun openBoardAsMe(context: Context, g: Group, board: Board): Pair<String, String> {
        val mine = myHexIn(context, g.members)
        val secret = PersonaStore(context).secretFor(mine)
            ?: throw IllegalStateException("no secret for my persona in this group")
        val key = boardKey(context, g, board)
        val public = hexToBytes(mine) ?: throw IllegalStateException("my persona key is not hex")
        uniffi.ducat_mobile.nodeDhtOpen(key, public, secret)
        return key to mine
    }

    /**
     * One pen per board, and one reader per board. The lane and the sweep
     * may ask for the same board in the same breath, and two readers that
     * both computed "held" before either appended would store every new
     * entry twice; two writers would race the page cursor. Separate locks,
     * because a read need not wait for a write on the network.
     */
    private val penLocks = java.util.concurrent.ConcurrentHashMap<String, Any>()
    private val readLocks = java.util.concurrent.ConcurrentHashMap<String, Any>()

    /**
     * Say something on the board: append to my page, seal it, write it.
     * The row and the page go to disk first, so a write the network
     * refuses is retried from disk by the lap ([flushBoards]), not lost.
     * Returns whether the network took the page now.
     */
    private fun sendOnBoard(
        context: Context,
        g: Group,
        board: Board,
        mine: String,
        seq: Long,
        body: String,
        kind: Int,
        reSender: String?,
        reSeq: Long?,
    ): Boolean = synchronized(penLocks.getOrPut(g.idHex) { Any() }) {
        val entry = PageEntry(
            s = seq,
            t = System.currentTimeMillis() / 1000,
            k = kind,
            b = if (kind == 5) null else body,
            rs = reSender,
            rq = reSeq,
        )
        val spec = boardSpec(g.idHex, g.members, board)
        val cap = uniffi.ducat_mobile.groupPageCap(boardSubkeys(spec)).toInt()
        var page = board.page
        var entries = myPage(context, g.idHex) + entry
        var bytes = uniffi.ducat_mobile.groupPageEncode(
            board.generation.toULong(), entries.map { it.out() },
        )
        if (bytes.size + SEAL_OVERHEAD > cap && entries.size > 1) {
            // The page is full: this word opens the next one in the ring.
            page = (page + 1) % maxOf(board.pages, 1)
            entries = listOf(entry)
            bytes = uniffi.ducat_mobile.groupPageEncode(
                board.generation.toULong(), entries.map { it.out() },
            )
        }
        if (bytes.size + SEAL_OVERHEAD > cap) {
            throw IllegalStateException("too long for this board's pages")
        }
        Mailbox.appendGroupRow(
            context, mine,
            StoredMessage(
                outgoing = true,
                seq = seq,
                body = body,
                timestamp = entry.t,
                kind = kind,
                groupId = g.idHex,
                groupSeq = seq,
                groupReSender = reSender,
                groupReSeq = reSeq,
            ),
            announce = false,
        )
        saveMyPage(context, g.idHex, entries)
        fun setBoard(dirty: Boolean, at: Int) {
            get(context, g.idHex)?.let { fresh ->
                val b = fresh.board
                if (b != null && b.generation == board.generation) {
                    upsert(context, fresh.copy(board = b.copy(page = at, dirty = dirty)))
                }
            }
        }
        setBoard(true, page)
        touch(g.idHex)
        runCatching { writeMyPage(context, g, board, mine, page, bytes) }.fold(
            onSuccess = {
                setBoard(false, page)
                true
            },
            onFailure = {
                DucatLog.w(TAG, "${g.name}: the board did not take the page — kept for the lap (${it.message})")
                false
            },
        )
    }

    private fun writeMyPage(context: Context, g: Group, board: Board, mine: String, page: Int, bytes: ByteArray) {
        val spec = boardSpec(g.idHex, g.members, board)
        val public = hexToBytes(mine) ?: throw IllegalStateException("my persona key is not hex")
        val base = boardBase(spec, public) ?: throw IllegalStateException("my persona is not on this board")
        val (key, _) = openBoardAsMe(context, g, board)
        val groupKey = hexToBytes(board.groupKeyHex) ?: throw IllegalStateException("a group key is hex")
        val subkey = base + page.toUInt()
        val sealed = uniffi.ducat_mobile.groupPageSeal(GroupPageSeal(groupKey, key, subkey, bytes))
        uniffi.ducat_mobile.nodeDhtSet(key, subkey, sealed)
    }

    /** Pages the network did not take yet, written again. */
    private fun flushBoards(context: Context) {
        for (g in all(context)) {
            if (g.left) continue
            val board = g.board ?: continue
            if (!board.dirty) continue
            synchronized(penLocks.getOrPut(g.idHex) { Any() }) {
                val entries = myPage(context, g.idHex)
                if (entries.isEmpty()) return@synchronized
                val mine = myHexIn(context, g.members)
                val bytes = runCatching {
                    uniffi.ducat_mobile.groupPageEncode(
                        board.generation.toULong(), entries.map { it.out() },
                    )
                }.getOrElse {
                    DucatLog.w(TAG, "${g.name}: my page does not encode: ${it.message}")
                    return@synchronized
                }
                runCatching { writeMyPage(context, g, board, mine, board.page, bytes) }.fold(
                    onSuccess = {
                        get(context, g.idHex)?.let { fresh ->
                            fresh.board?.let { b -> upsert(context, fresh.copy(board = b.copy(dirty = false))) }
                        }
                        DucatLog.i(TAG, "${g.name}: page landed")
                    },
                    onFailure = { DucatLog.w(TAG, "${g.name}: page still not taken (${it.message})") },
                )
            }
        }
    }

    /**
     * Read what moved on one board: one inspection, then only the pages
     * whose sequence changed. An entry lands under its *sender's* thread —
     * the member the subkey belongs to, which the schema says and the page
     * does not — contact or not, and is ours when that member is one of
     * our personas. Entries already held are skipped. Returns how many
     * were new.
     */
    private fun readBoard(context: Context, g: Group, board: Board): Int =
        synchronized(readLocks.getOrPut(g.idHex) { Any() }) {
            val (key, mine) = openBoardAsMe(context, g, board)
            val spec = boardSpec(g.idHex, g.members, board)
            val groupKey = hexToBytes(board.groupKeyHex)
                ?: throw IllegalStateException("a group key is hex")
            val seqs = uniffi.ducat_mobile.nodeDhtInspect(key)
            if (board.seen.isEmpty()) {
                val held = seqs.count { it != UInt.MAX_VALUE }
                DucatLog.i(TAG, "${g.name}: board inspected — ${seqs.size} subkey(s), $held written")
            }
            val ours = PersonaStore(context).allHexes()
            val store = ContactStore(context)
            var got = 0
            var pending = false
            val seen = HashMap(board.seen)
            for ((i, seq) in seqs.withIndex()) {
                val subkey = i.toUInt()
                val slot = subkey.toString()
                if (seq == UInt.MAX_VALUE || seen[slot] == seq.toLong()) continue
                // The network first; then the node's own copy, which a watch
                // on the record keeps current and which answers when a get
                // stops three nodes short of the one that holds the page. A
                // page the inspection names but neither can produce keeps
                // the board hot for the next sweep instead of backing off.
                val read = uniffi.ducat_mobile.nodeDhtGetVersioned(key, subkey, true)
                    ?: runCatching { uniffi.ducat_mobile.nodeDhtGetVersioned(key, subkey, false) }.getOrNull()
                if (read == null) { pending = true; continue }
                val sender = boardMemberOf(spec, subkey) ?: continue
                val senderHex = sender.toHexString()
                val plain = try {
                    uniffi.ducat_mobile.groupPageOpen(GroupPageSeal(groupKey, key, subkey, read.data))
                } catch (e: Exception) {
                    DucatLog.w(TAG, "${g.name}: page $subkey does not open: ${e.message}")
                    seen[slot] = seq.toLong()
                    continue
                }
                val page = try {
                    uniffi.ducat_mobile.groupPageDecode(plain)
                } catch (e: Exception) {
                    DucatLog.w(TAG, "${g.name}: page $subkey refused: ${e.message}")
                    seen[slot] = seq.toLong()
                    continue
                }
                if (page.generation.toLong() != board.generation) {
                    DucatLog.w(TAG, "${g.name}: page $subkey is from generation ${page.generation}, not ${board.generation}")
                    seen[slot] = seq.toLong()
                    continue
                }
                // What is held of THIS sender's counter: their rows in their
                // thread, or mine in mine. A member's thread also holds my
                // outgoing rows to them — the roster among them, under my
                // counter — and those must not mask their entries.
                val theirs = senderHex !in ours
                val held = store.thread(senderHex)
                    .filter { it.groupId == g.idHex && it.outgoing != theirs }
                    .mapTo(HashSet()) { it.groupSeq }
                for (e in page.entries) {
                    val eseq = e.seq.toLong()
                    if (eseq in held) continue
                    Mailbox.appendGroupRow(
                        context, senderHex,
                        StoredMessage(
                            outgoing = senderHex == mine,
                            seq = eseq,
                            body = e.body ?: "",
                            timestamp = e.ts.toLong(),
                            kind = e.kind.toInt(),
                            groupId = g.idHex,
                            groupSeq = eseq,
                            groupReSender = e.reSender?.toHexString(),
                            groupReSeq = e.reSeq?.toLong(),
                        ),
                        announce = senderHex !in ours,
                    )
                    got++
                }
                seen[slot] = (read.seq ?: seq).toLong()
            }
            if (board.seen.isEmpty()) {
                DucatLog.i(TAG, "${g.name}: first pages read — ${seen.size} subkey(s) hold something")
            }
            if (pending) touch(g.idHex)
            if (seen != board.seen) {
                get(context, g.idHex)?.let { fresh ->
                    val b = fresh.board
                    if (b != null && b.generation == board.generation) {
                        upsert(context, fresh.copy(board = b.copy(seen = seen)))
                    }
                }
            }
            got
        }

    /**
     * Poller hook: the boards that are due, on the same plan as the logs —
     * a board that spoke is read every sweep, a quiet one backs off, and
     * the watch on its record rings the lane for it alone. Pages this phone
     * owes go first. Returns how many entries arrived.
     */
    fun lap(context: Context): Int {
        flushBoards(context)
        val now = System.currentTimeMillis()
        var got = 0
        for (g in all(context)) {
            if (g.left) continue
            val board = g.board ?: continue
            val slot = "g:${g.idHex}"
            if (Mailbox.planDueAt(slot) > now) continue
            runCatching { readBoard(context, g, board) }.fold(
                onSuccess = { n ->
                    got += n
                    Mailbox.planSettle(slot, n > 0)
                },
                onFailure = {
                    DucatLog.w(TAG, "${g.name}: board not read (${it.message})")
                    Mailbox.planSettle(slot, false)
                },
            )
            if (Mailbox.planWatchStale(slot, now) && board.key.isNotEmpty() &&
                runCatching { uniffi.ducat_mobile.nodeDhtWatch(board.key) }.getOrDefault(false)
            ) {
                Mailbox.planSetWatched(slot, now)
            }
        }
        return got
    }

    /** Looking at a group, or having just spoken in it: its board is read every sweep again. */
    fun touch(idHex: String) = Mailbox.touch("g:$idHex")

    /** Records the network rang for that are boards: due now. Returns them. */
    fun markBoardsChanged(context: Context, keys: Set<String>): List<Group> =
        all(context)
            .filter { g -> g.board?.key?.let { it.isNotEmpty() && it in keys } == true }
            .onEach { touch(it.idHex) }

    /** The lane's read of one board that rang, settled like a contact's log. */
    fun pollBoard(context: Context, g: Group): Int {
        val fresh = get(context, g.idHex) ?: return 0
        val board = fresh.board ?: return 0
        val n = runCatching { readBoard(context, fresh, board) }.getOrElse {
            DucatLog.w(TAG, "${fresh.name}: board not read (${it.message})")
            0
        }
        Mailbox.planSettle("g:${g.idHex}", n > 0)
        return n
    }

    // ---- the retry queue --------------------------------------------------
    //
    // A group message either reaches everyone or its sender knows who is
    // still owed a copy. Sends that fail (their node unreachable, our network
    // down) are parked here and replayed by the poller — same body, same
    // (sender, group_seq), so a copy that finally lands is the same message,
    // not a new one. Rosters ride this queue for a board group too; its
    // words do not — a page the network refused stays on disk, marked
    // dirty, and the lap writes it again (see flushBoards).

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
                // The roster as the group stands now — the board included,
                // so a late copy still carries the key it needs.
                val ok = c != null && g != null && runCatching {
                    Mailbox.send(
                        context, c, "group: ${g.name}",
                        kind = 12,
                        payload = rosterPayload(g),
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
     * in, which is the one fact fan-out makes unforgeable. On a board the
     * sender is the member whose subkey the page came from, which the
     * record's schema makes unforgeable, and the row sits under that key.
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
        // Pairwise copies of my own words live in each member's thread; on
        // a board they live once, under my own key, so that thread is read
        // too (the dedupe makes reading both harmless).
        for (m in g.members) {
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
