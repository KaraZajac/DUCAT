<script lang="ts">
  // Chat: every thread on the left, the open one on the right. A desk has
  // the room for both at once, which is the one place it beats the phone.
  import { onMount, tick } from "svelte";
  import { api, copy, fmtTime, fmtXmr, type ContactRow, type GroupMessage, type GroupRow, type MessageRow, type StandingRow } from "./api";
  import { gen, drive } from "./state.svelte";

  let rows = $state<ContactRow[]>([]);
  let open = $state<string | null>(null);
  let thread = $state<MessageRow[]>([]);
  let draft = $state("");
  let sending = $state(false);
  let err = $state<string | null>(null);
  let adding = $state(false);
  let cardUri = $state("");
  let petname = $state("");
  let claiming = $state(false);
  let renaming = $state(false);
  let newName = $state("");
  let list: HTMLDivElement | undefined = $state();
  // Groups share the list: a group is a conversation too.
  let groups = $state<GroupRow[]>([]);
  let openGroup = $state<string | null>(null);
  let groupThread = $state<GroupMessage[]>([]);
  let makingGroup = $state(false);
  let groupName = $state("");
  let groupPick = $state<Set<string>>(new Set());
  let addingMember = $state(false);
  const currentGroup = $derived(groups.find((g) => g.id_hex === openGroup) ?? null);

  const current = $derived(rows.find((r) => r.persona_hex === open) ?? null);

  async function refresh() {
    try {
      rows = await api.contacts();
      groups = await api.groups();
      if (openGroup) {
        const t = await api.groupThread(openGroup);
        const grew = t.length !== groupThread.length;
        groupThread = t;
        if (grew) {
          scrollToEnd();
          await api.markGroupSeen(openGroup);
        }
      }
      if (open) {
        const t = await api.thread(open);
        const grew = t.length !== thread.length;
        thread = t;
        if (grew) {
          scrollToEnd();
          // The thread is on screen: what arrives while it is open is seen.
          await api.markSeen(open);
        }
      }
    } catch (e) {
      err = String(e);
    }
  }

  async function scrollToEnd() {
    await tick();
    if (list) list.scrollTop = list.scrollHeight;
  }

  async function selectGroup(id: string) {
    open = null;
    openGroup = id;
    groupThread = await api.groupThread(id);
    await api.markGroupSeen(id);
    scrollToEnd();
  }

  async function createGroup() {
    if (!groupName.trim() || groupPick.size === 0) return;
    err = null;
    try {
      const g = await api.createGroup(groupName.trim(), [...groupPick]);
      makingGroup = false;
      groupName = "";
      groupPick = new Set();
      await refresh();
      await selectGroup(g.id_hex);
    } catch (e) {
      err = String(e);
    }
  }

  async function sendToGroup() {
    const body = draft.trim();
    if (!body || !openGroup) return;
    err = null;
    sending = true;
    try {
      const all = await api.sendGroup(openGroup, body);
      if (!all) err = "Some copies did not go out yet — they are queued and retried.";
      draft = "";
      groupThread = await api.groupThread(openGroup);
      scrollToEnd();
    } catch (e) {
      err = String(e);
    } finally {
      sending = false;
    }
  }

  async function select(hex: string) {
    if (open && open !== hex) api.saveDraft(open, draft).catch(() => {});
    openGroup = null;
    open = hex;
    renaming = false;
    requesting = false;
    showSettings = false;
    picking = null;
    standing = await api.standingBills();
    draft = await api.draft(hex);
    disappear = await api.disappearAfter(hex);
    thread = await api.thread(hex);
    await api.markSeen(hex);
    scrollToEnd();
  }

  onMount(refresh);

  $effect(() => {
    void gen.value;
    refresh();
  });

  async function send() {
    const body = draft.trim();
    if (!body || !open) return;
    err = null;
    sending = true;
    try {
      if (replyTo) await api.sendReply(open, body, replyTo.seq, replyTo.own);
      else await api.sendText(open, body);
      replyTo = null;
      draft = "";
      api.saveDraft(open, "").catch(() => {});
      thread = await api.thread(open);
      scrollToEnd();
      // Their reply should not be fifteen seconds late.
      setTimeout(() => api.pollNow(open ?? undefined).catch(() => {}), 4000);
    } catch (e) {
      err = String(e);
    } finally {
      sending = false;
    }
  }

  async function claim() {
    const uri = cardUri.trim();
    if (!uri) return;
    err = null;
    claiming = true;
    try {
      const r = await api.claimCard(uri, petname.trim() || null);
      cardUri = "";
      petname = "";
      adding = false;
      await refresh();
      await select(r.contact.persona_hex);
      // The reply we published is their next poll away; ours is the lap's.
      setTimeout(() => api.pollNow().catch(() => {}), 8000);
    } catch (e) {
      err = String(e);
    } finally {
      claiming = false;
    }
  }

  async function rename() {
    if (!open) return;
    await api.setPetname(open, newName.trim() || null);
    renaming = false;
    await refresh();
  }

  async function remove() {
    if (!open) return;
    await api.removeContact(open);
    open = null;
    thread = [];
    await refresh();
  }

  let payingOut = $state(false);
  let payAmount = $state("");
  let payNote = $state("");

  async function payUnprompted() {
    if (!open) return;
    err = null;
    const n = Number(payAmount);
    if (!Number.isFinite(n) || n <= 0) { err = "An amount in XMR."; return; }
    try {
      await api.payBill(open, null, Math.floor(n * 1e12), payNote.trim() || null, 1);
      payingOut = false; payAmount = ""; payNote = "";
      thread = await api.thread(open);
      scrollToEnd();
    } catch (e) { err = String(e); }
  }

  // The smaller rites: a reaction, taking a message back, deleting a
  // row here, messages that disappear, the draft kept per thread.
  const QUICK = ["👍", "❤️", "😂", "😮", "😢", "🔥"];
  let hovered = $state<string | null>(null);
  let picking = $state<string | null>(null);
  let disappear = $state(0);
  let showSettings = $state(false);
  let draftTimer: ReturnType<typeof setTimeout> | null = null;

  function keyOf(m: MessageRow): string {
    return `${m.outgoing ? "o" : "i"}:${m.seq}:${m.timestamp}`;
  }

  async function reactTo(m: MessageRow, emoji: string) {
    if (!open) return;
    err = null;
    picking = null;
    try {
      // The same emoji again takes it back.
      const already = m.react_mine === emoji;
      await api.react(open, m.seq, m.outgoing, already ? "" : emoji);
      thread = await api.thread(open);
    } catch (e) { err = String(e); }
  }

  async function unsend(m: MessageRow) {
    if (!open) return;
    err = null;
    try {
      await api.retractMessage(open, m.seq);
      thread = await api.thread(open);
    } catch (e) { err = String(e); }
  }

  async function deleteHere(m: MessageRow) {
    if (!open) return;
    err = null;
    try {
      await api.deleteMessage(open, m.seq, m.outgoing, m.timestamp);
      thread = await api.thread(open);
    } catch (e) { err = String(e); }
  }

  async function clearThread() {
    if (!open) return;
    await api.deleteThread(open);
    thread = await api.thread(open);
  }

  async function hideThread() {
    if (!open) return;
    await api.setChatVisible(open, false);
    open = null;
    thread = [];
    await refresh();
  }

  async function setDisappear(secs: number) {
    if (!open) return;
    disappear = secs;
    await api.setDisappearAfter(open, secs);
  }

  function draftChanged() {
    if (!open) return;
    const hex = open;
    if (draftTimer) clearTimeout(draftTimer);
    draftTimer = setTimeout(() => api.saveDraft(hex, draft).catch(() => {}), 400);
  }

  let requesting = $state(false);
  let reqAmount = $state("");
  let reqNote = $state("");
  let reqRepeat = $state<"once" | "weekly" | "monthly">("once");
  let standing = $state<StandingRow[]>([]);

  async function sendRequest() {
    if (!open) return;
    err = null;
    const n = Number(reqAmount);
    if (!Number.isFinite(n) || n <= 0) { err = "An amount in XMR."; return; }
    const pxmr = Math.floor(n * 1e12);
    try {
      if (reqRepeat === "once") await api.requestPayment(open, pxmr, reqNote);
      else await api.addStandingBill(open, pxmr, reqNote, reqRepeat === "monthly");
      requesting = false; reqAmount = ""; reqNote = ""; reqRepeat = "once";
      await refresh();
      standing = await api.standingBills();
    } catch (e) { err = String(e); }
  }

  // Attachments: pictures inline once here, files as a line with Show.
  let pictures = $state<Record<string, string>>({});
  let fetching = $state<string | null>(null);
  let attaching = $state(false);

  $effect(() => {
    for (const m of thread) {
      if (m.att_hash && m.att_here && m.att_mime?.startsWith("image/") && !pictures[m.att_hash]) {
        const h = m.att_hash;
        api.attachmentPath(h).then((p) => p && api.pictureDataUrl(p)).then((u) => { if (u) pictures = { ...pictures, [h]: u }; }).catch(() => {});
      }
    }
  });

  // Voice memos and other audio: a data URL the page can play, once here.
  let audios = $state<Record<string, string>>({});
  $effect(() => {
    for (const m of thread) {
      if (m.att_hash && m.att_here && m.att_mime?.startsWith("audio/") && !audios[m.att_hash]) {
        const h = m.att_hash;
        api.attachmentDataUrl(h, m.att_mime ?? null).then((u) => { if (u) audios = { ...audios, [h]: u }; }).catch(() => {});
      }
    }
  });

  // Replying: the reference travels, the quote is read from our own copy.
  let replyTo = $state<{ seq: number; own: boolean; line: string } | null>(null);
  function startReply(m: MessageRow) {
    replyTo = { seq: m.seq, own: m.outgoing, line: quoteOf(m) };
  }
  function quoteOf(m: MessageRow): string {
    if (m.unsent) return "This message was withdrawn.";
    if (m.kind === 1) return "a request for money";
    if (m.kind === 2) return "a payment";
    if (m.kind === 3) return "a receipt";
    if (m.att_hash && !m.body.trim()) return "an attachment";
    if (m.body.trim()) return m.body;
    return "a message";
  }

  // A bill of theirs, declined; a bill of ours, taken back.
  let answering = $state<string | null>(null);
  async function decline(m: MessageRow) {
    if (!open) return;
    err = null;
    answering = keyOf(m);
    try { await api.declineBill(open, m.seq, m.timestamp); thread = await api.thread(open); } catch (e) { err = String(e); } finally { answering = null; }
  }
  async function cancelMine(m: MessageRow) {
    if (!open) return;
    err = null;
    answering = keyOf(m);
    try { await api.cancelBill(open, m.seq, m.timestamp); thread = await api.thread(open); } catch (e) { err = String(e); } finally { answering = null; }
  }

  // Sharing: a one-claim card for me, or somebody's profile.
  let sharing = $state(false);
  let people = $state<ContactRow[]>([]);
  let shareWho = $state("");
  async function openShare() {
    sharing = !sharing;
    if (sharing) { try { people = (await api.contacts()).filter((c) => c.persona_hex !== open); } catch {} }
  }
  async function shareCard() {
    if (!open) return;
    err = null;
    try { await api.shareIntroCard(open); sharing = false; thread = await api.thread(open); scrollToEnd(); } catch (e) { err = String(e); }
  }
  async function shareProfile() {
    if (!open || !shareWho) return;
    err = null;
    try { await api.shareContact(open, shareWho); sharing = false; thread = await api.thread(open); scrollToEnd(); } catch (e) { err = String(e); }
  }
  const CARD_LINK = /ducat:card\/\S+/;
  function cardIn(body: string): string | null {
    const m = body.match(CARD_LINK);
    return m ? m[0] : null;
  }
  let answeringCard = $state(false);
  async function answerCard(uri: string) {
    err = null;
    answeringCard = true;
    try {
      const r = await api.claimCard(uri, null);
      contacts = await api.contacts();
      open = r.contact.persona_hex;
    } catch (e) { err = String(e); } finally { answeringCard = false; }
  }

  // Voice memos: click to start, click to send; the counter ticks here.
  let recording = $state(false);
  let recMs = $state(0);
  let recTimer: ReturnType<typeof setInterval> | null = null;
  async function startMemo() {
    if (!open) return;
    err = null;
    try {
      await api.memoStart();
      recording = true;
      recMs = 0;
      recTimer = setInterval(async () => { const ms = await api.memoElapsedMs().catch(() => null); if (ms != null) recMs = ms; }, 500);
    } catch (e) { err = String(e); }
  }
  function endTicking() {
    if (recTimer) clearInterval(recTimer);
    recTimer = null;
    recording = false;
  }
  async function sendMemo() {
    if (!open) return;
    endTicking();
    sending = true;
    try { await api.memoStopSend(open); thread = await api.thread(open); scrollToEnd(); } catch (e) { err = String(e); } finally { sending = false; }
  }
  async function cancelMemo() {
    endTicking();
    await api.memoCancel().catch(() => {});
  }
  function clock(ms: number): string {
    const s = Math.floor(ms / 1000);
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  }

  async function attach(path?: string) {
    if (!open) return;
    const p = path ?? (await api.pickFile());
    if (!p) return;
    err = null;
    attaching = true;
    try {
      await api.sendAttachment(open, p, draft.trim() || null);
      draft = "";
      thread = await api.thread(open);
      scrollToEnd();
    } catch (e) {
      err = String(e);
    } finally {
      attaching = false;
    }
  }

  async function fetchBig(m: MessageRow) {
    if (!open) return;
    err = null;
    fetching = m.att_hash;
    try {
      await api.fetchSwarmAttachment(open, m.seq, m.outgoing);
      thread = await api.thread(open);
    } catch (e) {
      err = String(e);
    } finally {
      fetching = null;
    }
  }

  async function showFile(m: MessageRow) {
    if (!m.att_hash) return;
    const p = await api.attachmentPath(m.att_hash);
    if (p) await api.reveal(p);
  }

  let paying = $state<number | null>(null);
  let paid = $state<Record<number, string>>({});

  /// A bill of theirs this thread has not answered with a payment yet.
  function unpaid(m: MessageRow): boolean {
    return !m.outgoing && m.kind === 1 && !thread.some((x) => x.outgoing && x.kind === 2 && x.re_seq === m.seq);
  }

  async function pay(m: MessageRow) {
    if (!open) return;
    err = null;
    paying = m.seq;
    try {
      const tx = await api.payBill(open, m.seq, m.amount_pxmr, null, 1);
      paid = { ...paid, [m.seq]: tx };
      thread = await api.thread(open);
    } catch (e) {
      err = String(e);
    } finally {
      paying = null;
    }
  }

  function kindLabel(m: MessageRow): string | null {
    switch (m.kind) {
      case 1: return `Bill · ${fmtXmr(m.amount_pxmr)}`;
      case 2: return `Payment · ${fmtXmr(m.amount_pxmr)}`;
      case 3: return m.oob ? "Receipt · settled outside DUCAT" : `Receipt · ${fmtXmr(m.amount_pxmr)}`;
      case 4: return "Reaction";
      case 5: return "Retraction";
      case 6: return "Ride offer";
      case 7: return "Ride accepted";
      case 13: return "A publication key";
      case 14: return "Call";
      case 15: return "Call answered";
      case 16: return `Asked for an issue${m.pub_wanted ? ` · ${m.pub_wanted}` : ""}`;
      default: return null;
    }
  }
</script>

<div class="chat">
  <div class="threads">
    <div class="threads-head">
      <h2>Chat</h2>
      <button class="btn small" onclick={() => (adding = !adding)}>{adding ? "Close" : "Add"}</button>
    </div>
    {#if adding}
      <div class="add-card">
        <input class="input" placeholder="Paste a ducat:card/… code" bind:value={cardUri} />
        <input class="input" placeholder="What you call them (optional)" bind:value={petname} />
        <button class="btn primary" disabled={!cardUri.trim() || claiming} onclick={claim}>{claiming ? "Answering…" : "Answer the card"}</button>
        <p class="note">A card is answered once. Yours is on the Me page.</p>
      </div>
    {/if}
    {#if rows.length === 0 && !adding}
      <p class="empty">Nobody yet. Answer somebody's card, or show them yours on the Me page.</p>
    {/if}
    {#if groups.length || rows.length >= 2}
      <div class="list-head"><span>Groups</span><button class="linkish" onclick={() => (makingGroup = !makingGroup)}>{makingGroup ? "close" : "new"}</button></div>
    {/if}
    {#if makingGroup}
      <div class="add-card">
        <input class="input" placeholder="A name for the group" bind:value={groupName} />
        <div class="chips">
          {#each rows as r (r.persona_hex)}
            <button class="chip" class:on={groupPick.has(r.persona_hex)} onclick={() => { const n = new Set(groupPick); n.has(r.persona_hex) ? n.delete(r.persona_hex) : n.add(r.persona_hex); groupPick = n; }}>{r.name}</button>
          {/each}
        </div>
        <button class="btn primary" disabled={!groupName.trim() || groupPick.size === 0} onclick={createGroup}>Create</button>
        <p class="note">Everyone in it must already know everyone else; the roster goes out to each member.</p>
      </div>
    {/if}
    {#each groups as g (g.id_hex)}
      <button class="thread-row" class:active={g.id_hex === openGroup} onclick={() => selectGroup(g.id_hex)}>
        <div class="avatar group">#</div>
        <div class="thread-text">
          <div class="thread-top"><span class="thread-name" class:unread={g.unread}>{g.name}</span><span class="thread-when">{fmtTime(g.last_at)}</span></div>
          <div class="thread-last" class:unread={g.unread}>{#if g.last_body}{g.last_body}{:else}<i>{g.members.length + 1} in the group</i>{/if}</div>
        </div>
        {#if g.unread}<span class="dot-unread"></span>{/if}
      </button>
    {/each}
    {#if groups.length}<div class="list-head"><span>People</span></div>{/if}
    {#each rows.filter((r) => r.chat_visible) as r (r.persona_hex)}
      <button class="thread-row" class:active={r.persona_hex === open} onclick={() => select(r.persona_hex)}>
        <div class="avatar" class:unnamed={!r.named}>{r.name.slice(0, 1).toUpperCase()}</div>
        <div class="thread-text">
          <div class="thread-top">
            <span class="thread-name" class:unread={r.unread}>{r.name}</span>
            <span class="thread-when">{fmtTime(r.last_at)}</span>
          </div>
          <div class="thread-last" class:unread={r.unread}>
            {#if r.last_body}{r.last_outgoing ? "You: " : ""}{r.last_body}{:else}<i>No messages yet</i>{/if}
          </div>
        </div>
        {#if r.unread}<span class="dot-unread"></span>{/if}
      </button>
    {/each}
  </div>

  <div class="pane">
    {#if currentGroup}
      <div class="pane-head">
        <div>
          <div class="pane-name">{currentGroup.name}</div>
          <div class="meta">
            {currentGroup.members.map((m) => m.name).join(", ")} and you
            {#if currentGroup.missing.length} · {currentGroup.missing.length} member{currentGroup.missing.length === 1 ? "" : "s"} you do not know yet — nothing can be sent until you do{/if}
          </div>
        </div>
        <div class="actions nowrap">
          <button class="btn small" onclick={() => (addingMember = !addingMember)}>{addingMember ? "Close" : "Add"}</button>
        </div>
      </div>
      {#if addingMember}
        <div class="chips" style="padding: 8px 24px">
          {#each rows.filter((r) => !currentGroup!.members.some((m) => m.persona_hex === r.persona_hex)) as r (r.persona_hex)}
            <button class="chip" onclick={async () => { await api.addToGroup(currentGroup!.id_hex, r.persona_hex); addingMember = false; await refresh(); }}>{r.name}</button>
          {/each}
        </div>
      {/if}
      <div class="bubbles" bind:this={list}>
        {#each groupThread as g (g.sender_hex + ":" + g.message.group_id + ":" + g.message.seq + ":" + g.message.timestamp)}
          <div class="bubble-row" class:out={g.mine}>
            <div class="bubble" class:out={g.mine}>
              {#if !g.mine}<div class="bubble-kind">{g.sender_name}</div>{/if}
              <div class="bubble-body">{g.message.body}</div>
              <div class="bubble-meta">{fmtTime(g.message.timestamp)}</div>
            </div>
          </div>
        {/each}
        {#if groupThread.length === 0}<p class="empty">Nothing here yet.</p>{/if}
      </div>
      <div class="composer">
        <textarea class="input" rows="2" placeholder="Write to the group" bind:value={draft} disabled={currentGroup.missing.length > 0}
          onkeydown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); sendToGroup(); } }}></textarea>
        <button class="btn primary" disabled={!draft.trim() || sending || currentGroup.missing.length > 0} onclick={sendToGroup}>{sending ? "Sending…" : "Send"}</button>
      </div>
      {#if err}<p class="err">{err}</p>{/if}
    {:else if current}
      <div class="pane-head stacked">
        <div class="pane-lead">
          {#if renaming}
            <input class="input" bind:value={newName} placeholder={current.asserted_name ?? "A name for them"} onkeydown={(e) => e.key === "Enter" && rename()} />
            <button class="btn small" onclick={rename}>Save</button>
            <button class="btn small" onclick={() => (renaming = false)}>Cancel</button>
          {:else}
            <div class="pane-name">{current.name}</div>
            <div class="meta">
              {#if current.petname && current.asserted_name && current.petname !== current.asserted_name}calls themself {current.asserted_name} · {/if}
              {#if !current.has_keys}their keys have not arrived yet · {/if}
              {#if current.pending_address}a new payment address is being held for you · {/if}
              {#if current.email}{current.email} · {/if}{#if current.phone}{current.phone} · {/if}{#if current.signal}Signal {current.signal} · {/if}
              <button class="linkish mono" title="Copy their key" onclick={() => copy(current?.persona_hex ?? "")}>{current.persona_hex.slice(0, 12)}…</button>
            </div>
          {/if}
        </div>
        <div class="actions pane-actions">
          <button class="btn small" title="Voice call" disabled={!current.has_keys} onclick={async () => { err = null; try { await api.placeCall(current!.persona_hex); } catch (e) { err = String(e); } }}>📞 Call</button>
          <button class="btn small" class:active={payingOut} disabled={!current.their_address} title={current.their_address ? "Send them money" : "They have not given a payment address"} onclick={() => { payingOut = !payingOut; requesting = false; }}>Pay</button>
          <button class="btn small" class:active={requesting} onclick={() => { requesting = !requesting; payingOut = false; }}>Request</button>
          <button class="btn small" onclick={() => { renaming = true; newName = current?.petname ?? ""; }}>Rename</button>
          <button class="btn small" class:active={showSettings} title="Thread settings" onclick={() => (showSettings = !showSettings)}>⋯ More</button>
        </div>
      </div>
      {#if showSettings}
        <div class="request-bar">
          <span class="meta">Messages disappear after</span>
          <select class="input narrow" value={disappear} onchange={(e) => setDisappear(Number((e.target as HTMLSelectElement).value))}>
            <option value={0}>never</option>
            <option value={3600}>an hour</option>
            <option value={86400}>a day</option>
            <option value={604800}>a week</option>
          </select>
          <button class="btn small" onclick={clearThread}>Clear the thread here</button>
          <button class="btn small" onclick={hideThread}>Hide</button>
          <button class="btn small danger" onclick={remove}>Forget them</button>
          <button class="btn small" class:active={sharing} onclick={openShare}>Share…</button>
          <span class="meta">Clearing and hiding touch only this desk; forgetting drops the contact.</span>
        </div>
      {/if}
      {#if sharing}
        <div class="request-bar">
          <button class="btn small" title="A one-claim link they can pass to someone who should reach you" onclick={shareCard}>A card for me</button>
          <select class="input" bind:value={shareWho}>
            <option value="">Someone's profile…</option>
            {#each people as c (c.persona_hex)}<option value={c.persona_hex}>{c.name}</option>{/each}
          </select>
          <button class="btn small" disabled={!shareWho} onclick={shareProfile}>Share the profile</button>
          <span class="meta">A profile is their name and what they chose to publish — not their connection code.</span>
        </div>
      {/if}
      {#if payingOut}
        <div class="request-bar">
          <input class="input narrow" placeholder="XMR" bind:value={payAmount} />
          <input class="input" placeholder="A note for your records (optional)" bind:value={payNote} onkeydown={(e) => e.key === "Enter" && payUnprompted()} />
          <button class="btn primary" onclick={payUnprompted} disabled={!payAmount.trim()}>Send the money</button>
          <span class="meta">{current.card_purpose === "donate" ? "Their code is a donation jar: this goes as a gift." : "Unprompted; they get a payment notice."}</span>
        </div>
      {/if}
      {#if requesting}
        <div class="request-bar">
          <input class="input narrow" placeholder="XMR" bind:value={reqAmount} />
          <input class="input" placeholder="What for" bind:value={reqNote} onkeydown={(e) => e.key === "Enter" && sendRequest()} />
          <select class="input narrow" bind:value={reqRepeat}><option value="once">once</option><option value="weekly">every week</option><option value="monthly">every month</option></select>
          <button class="btn primary" onclick={sendRequest} disabled={!reqAmount.trim()}>Send the bill</button>
        </div>
        {#each standing.filter((b) => b.persona_hex === open) as b (b.id)}
          <div class="request-bar meta">Standing: {fmtXmr(b.amount_pxmr)} {b.monthly ? "monthly" : "weekly"}{b.note ? ` · ${b.note}` : ""} · next {fmtTime(Math.floor(b.next_at / 1000))} <button class="linkish" onclick={async () => { await api.stopStandingBill(b.id); standing = await api.standingBills(); }}>stop</button></div>
        {/each}
      {/if}
      <div class="bubbles" bind:this={list}>
        {#each thread as m (m.outgoing + ":" + m.seq + ":" + m.timestamp)}
          <div class="bubble-row" role="listitem" class:out={m.outgoing} onmouseenter={() => (hovered = keyOf(m))} onmouseleave={() => { if (picking !== keyOf(m)) hovered = null; }}>
            {#if hovered === keyOf(m) && !m.dead_letter}
              <div class="bubble-tools" class:out={m.outgoing}>
                <button class="linkish" title="React" onclick={() => (picking = picking === keyOf(m) ? null : keyOf(m))}>☺</button>
                {#if !m.unsent}<button class="linkish" title="Reply" onclick={() => startReply(m)}>↩</button>{/if}
                {#if m.kind === 0 && m.body && !m.unsent}<button class="linkish" title="Copy the text" onclick={() => copy(m.body)}>⧉</button>{/if}
                {#if m.outgoing && m.kind === 0 && m.delivered && !m.unsent}<button class="linkish" title="Take it back for both of you" onclick={() => unsend(m)}>↶</button>{/if}
                <button class="linkish" title="Delete here only" onclick={() => deleteHere(m)}>✕</button>
              </div>
            {/if}
            <div class="bubble" class:out={m.outgoing} class:dead={m.dead_letter} class:money={m.kind >= 1 && m.kind <= 3} class:unsent={m.unsent}>
              {#if picking === keyOf(m)}
                <div class="picker">{#each QUICK as q}<button class="linkish" class:on={m.react_mine === q} onclick={() => reactTo(m, q)}>{q}</button>{/each}</div>
              {/if}
              {#if m.unsent}<div class="bubble-kind">{m.outgoing ? "You took this back" : "They took this back"}</div>{/if}
              {#if m.withdrawn}<div class="bubble-kind">{m.outgoing ? "Withdrawn" : "They withdrew this bill"}</div>{/if}
              {#if m.refused}<div class="bubble-kind">{m.outgoing ? "They declined this bill" : "Declined"}</div>{/if}
              {#if kindLabel(m)}<div class="bubble-kind">{kindLabel(m)}</div>{/if}
              {#if m.kind === 0 && m.re_seq !== null}
                <div class="quote" class:gone={m.quote === null}>{m.quote ?? "a message that is no longer here"}</div>
              {/if}
              {#if m.items.length}
                <div class="bill">
                  {#each m.items as [d, a]}<div class="bill-line"><span>{d}</span><span>{fmtXmr(a)}</span></div>{/each}
                  {#if m.tax_pxmr}<div class="bill-line"><span>Tax</span><span>{fmtXmr(m.tax_pxmr)}</span></div>{/if}
                </div>
              {/if}
              {#if m.att_hash}
                {#if m.att_here && m.att_mime?.startsWith("image/") && pictures[m.att_hash]}
                  <img class="bubble-pic" src={pictures[m.att_hash]} alt="" />
                {:else if m.att_here && m.att_mime?.startsWith("audio/") && audios[m.att_hash]}
                  <div class="bubble-audio"><span class="meta">🎤 Voice memo</span><audio controls preload="metadata" src={audios[m.att_hash]}></audio></div>
                {:else if m.att_here}
                  <div class="bubble-att">📎 {m.att_name ?? m.att_mime} · <button class="linkish" onclick={() => showFile(m)}>Show</button></div>
                {:else if m.att_on_swarm}
                  <div class="bubble-att">📎 {m.att_name ?? m.att_mime} · {(m.att_len / 1024 / 1024).toFixed(1)} MB · <button class="linkish" disabled={fetching === m.att_hash} onclick={() => fetchBig(m)}>{fetching === m.att_hash ? "fetching…" : "Fetch"}</button></div>
                {:else}
                  <div class="bubble-att">📎 {m.att_name ?? m.att_mime ?? "attachment"} · arriving…</div>
                {/if}
              {/if}
              {#if unpaid(m) && !m.withdrawn && !m.refused}
                <div class="actions" style="margin: 6px 0 4px">
                  <button class="btn small primary" disabled={paying !== null} onclick={() => pay(m)}>{paying === m.seq ? "Paying…" : `Pay ${fmtXmr(m.amount_pxmr)}`}</button>
                  <button class="btn small" disabled={answering !== null} title="Not this time" onclick={() => decline(m)}>{answering === keyOf(m) ? "…" : "Decline"}</button>
                </div>
              {:else if !m.outgoing && m.kind === 1 && !m.withdrawn && !m.refused}
                <div class="meta">paid</div>
              {/if}
              {#if m.outgoing && m.kind === 1 && m.delivered && !m.withdrawn && !m.refused && !m.bill_answered}
                <div class="actions" style="margin: 6px 0 4px">
                  <button class="btn small" disabled={answering !== null} title="Take this bill back — nothing to pay" onclick={() => cancelMine(m)}>{answering === keyOf(m) ? "…" : "Cancel this bill"}</button>
                </div>
              {/if}
              {#if m.body && !(m.att_hash && (m.body === "📷" || m.body === "🎤" || m.body.startsWith("📎 ")))}<div class="bubble-body">{m.body}</div>{/if}
              {#if !m.outgoing && m.kind === 0 && cardIn(m.body)}
                <div class="actions" style="margin: 6px 0 2px"><button class="btn small primary" disabled={answeringCard} onclick={() => answerCard(cardIn(m.body)!)}>{answeringCard ? "Answering…" : "Answer this card"}</button></div>
              {/if}
              <div class="bubble-meta">
                {fmtTime(m.timestamp)}
                {#if m.outgoing}{m.delivered ? (m.read_by_them ? " · read" : " · sent") : " · sending…"}{/if}
                {#if !m.forward_secret && !m.dead_letter} · no forward secrecy{/if}
              </div>
              {#if m.react_mine || m.react_theirs}
                <div class="reactions">{#if m.react_theirs}<span title={current?.name}>{m.react_theirs}</span>{/if}{#if m.react_mine}<span class="mine" title="you">{m.react_mine}</span>{/if}</div>
              {/if}
            </div>
          </div>
        {/each}
        {#if thread.length === 0}
          <p class="empty">Nothing here yet. Say hello.</p>
        {/if}
      </div>
      {#if replyTo}
        <div class="reply-banner"><span class="meta">Replying to</span><span class="reply-line">{replyTo.line}</span><button class="linkish" title="Not replying to that" onclick={() => (replyTo = null)}>✕</button></div>
      {/if}
      {#if recording}
        <div class="composer recording">
          <span class="rec-dot"></span><span>Recording · {clock(recMs)}</span>
          <button class="btn" onclick={cancelMemo}>Discard</button>
          <button class="btn primary" disabled={sending} onclick={sendMemo}>{sending ? "Sending…" : "Send the memo"}</button>
        </div>
      {:else}
        <div class="composer">
          <textarea class="input" rows="2" placeholder={replyTo ? "Your reply" : "Write a message"} bind:value={draft} disabled={!current.has_keys} oninput={draftChanged}
            onkeydown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); send(); } }}></textarea>
          <button class="btn" title="Attach a picture or a file" disabled={attaching || !current.has_keys} onclick={() => attach()}>{attaching ? "…" : "📎"}</button>
          <button class="btn" title="Record a voice memo" disabled={sending || !current.has_keys} onclick={startMemo}>🎤</button>
          <button class="btn primary" disabled={!draft.trim() || sending || !current.has_keys} onclick={send}>{sending ? "Sending…" : "Send"}</button>
        </div>
      {/if}
      {#if drive.on}<div class="request-bar"><input id="attpath" class="input" placeholder="/path/to/attach" onchange={(e) => attach((e.target as HTMLInputElement).value)} /></div>{/if}
      {#if err}<p class="err">{err}</p>{/if}
    {:else}
      <div class="pane-empty">
        <p class="empty">Pick a conversation, or add somebody with their card.</p>
        {#if err}<p class="err">{err}</p>{/if}
      </div>
    {/if}
  </div>
</div>
