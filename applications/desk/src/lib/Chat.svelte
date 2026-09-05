<script lang="ts">
  // Chat: every thread on the left, the open one on the right. A desk has
  // the room for both at once, which is the one place it beats the phone.
  import { onMount, tick } from "svelte";
  import { api, copy, fmtTime, fmtXmr, type ContactRow, type GroupMessage, type GroupRow, type MessageRow } from "./api";
  import { gen } from "./state.svelte";

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
    openGroup = null;
    open = hex;
    renaming = false;
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
      await api.sendText(open, body);
      draft = "";
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
      <div class="pane-head">
        <div>
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
        <div class="actions nowrap">
          <button class="btn small" onclick={() => { renaming = true; newName = current?.petname ?? ""; }}>Rename</button>
          <button class="btn small danger" onclick={remove}>Forget</button>
        </div>
      </div>
      <div class="bubbles" bind:this={list}>
        {#each thread as m (m.outgoing + ":" + m.seq + ":" + m.timestamp)}
          <div class="bubble-row" class:out={m.outgoing}>
            <div class="bubble" class:out={m.outgoing} class:dead={m.dead_letter} class:money={m.kind >= 1 && m.kind <= 3}>
              {#if kindLabel(m)}<div class="bubble-kind">{kindLabel(m)}</div>{/if}
              {#if m.items.length}
                <div class="bill">
                  {#each m.items as [d, a]}<div class="bill-line"><span>{d}</span><span>{fmtXmr(a)}</span></div>{/each}
                  {#if m.tax_pxmr}<div class="bill-line"><span>Tax</span><span>{fmtXmr(m.tax_pxmr)}</span></div>{/if}
                </div>
              {/if}
              {#if m.att_name}<div class="bubble-att">📎 {m.att_name}</div>{/if}
              {#if unpaid(m)}
                <div class="actions" style="margin: 6px 0 4px">
                  <button class="btn small primary" disabled={paying !== null} onclick={() => pay(m)}>{paying === m.seq ? "Paying…" : `Pay ${fmtXmr(m.amount_pxmr)}`}</button>
                </div>
              {:else if !m.outgoing && m.kind === 1}
                <div class="meta">paid</div>
              {/if}
              {#if m.body}<div class="bubble-body">{m.body}</div>{/if}
              <div class="bubble-meta">
                {fmtTime(m.timestamp)}
                {#if m.outgoing}{m.delivered ? (m.read_by_them ? " · read" : " · sent") : " · sending…"}{/if}
                {#if !m.forward_secret && !m.dead_letter} · no forward secrecy{/if}
              </div>
            </div>
          </div>
        {/each}
        {#if thread.length === 0}
          <p class="empty">Nothing here yet. Say hello.</p>
        {/if}
      </div>
      <div class="composer">
        <textarea class="input" rows="2" placeholder="Write a message" bind:value={draft} disabled={!current.has_keys}
          onkeydown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); send(); } }}></textarea>
        <button class="btn primary" disabled={!draft.trim() || sending || !current.has_keys} onclick={send}>{sending ? "Sending…" : "Send"}</button>
      </div>
      {#if err}<p class="err">{err}</p>{/if}
    {:else}
      <div class="pane-empty">
        <p class="empty">Pick a conversation, or add somebody with their card.</p>
        {#if err}<p class="err">{err}</p>{/if}
      </div>
    {/if}
  </div>
</div>
