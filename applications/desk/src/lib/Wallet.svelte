<script lang="ts">
  // Wallet: what is here, what is on its way, where money comes in, and
  // one form to send it. Stagenet until a mainnet build exists.
  import { onMount } from "svelte";
  import { api, copy, fmtXmr, fmtTime, type NoteRow, type Quote, type SentRow, type WalletView } from "./api";
  import { gen } from "./state.svelte";

  let view = $state<WalletView | null>(null);
  let notes = $state<NoteRow[]>([]);
  let sends = $state<SentRow[]>([]);
  let err = $state<string | null>(null);
  let to = $state("");
  let amount = $state("");
  let note = $state("");
  let priority = $state(1);
  let quote = $state<Quote | null>(null);
  let quoting = $state(false);
  let sending = $state(false);
  let sentTx = $state<string | null>(null);
  let ownNode = $state("");
  let editingNode = $state(false);
  let rescanFrom = $state("");
  let rescanning = $state(false);
  async function rescan() {
    err = null;
    const h = Number(rescanFrom.trim() || (view?.restore_height ?? 0));
    if (!Number.isFinite(h) || h < 0) { err = "A block height is a whole number."; return; }
    rescanning = true;
    try { await api.walletRescan(h); await refresh(); } catch (e) { err = String(e); } finally { rescanning = false; }
  }
  let tab = $state<"send" | "receive" | "history">("receive");
  let showNotes = $state(false);

  async function refresh() {
    try {
      view = await api.walletStatus();
      notes = await api.walletNotes();
      sends = await api.walletSends();
      if (!editingNode) ownNode = view.own_node ?? "";
    } catch (e) {
      err = String(e);
    }
  }

  onMount(() => {
    refresh();
    // A watched wallet takes its scan steps faster than the lane's twenty
    // seconds; nothing is lost if both run.
    const t = setInterval(() => api.walletStep().catch(() => {}), 6000);
    return () => clearInterval(t);
  });

  $effect(() => {
    void gen.value;
    refresh();
  });

  let quoteTimer: ReturnType<typeof setTimeout> | null = null;
  function requote() {
    if (quoteTimer) clearTimeout(quoteTimer);
    quote = null;
    if (!amount.trim()) return;
    quoteTimer = setTimeout(async () => {
      quoting = true;
      try {
        quote = await api.walletQuote(amount.trim(), priority);
      } catch (e) {
        quote = null;
      } finally {
        quoting = false;
      }
    }, 400);
  }

  async function sendMax() {
    const max = await api.walletMax(priority);
    amount = (max / 1e12).toFixed(12).replace(/0+$/, "").replace(/\.$/, "");
    requote();
  }

  async function send() {
    err = null;
    sentTx = null;
    sending = true;
    try {
      sentTx = await api.walletSend(to.trim(), amount.trim(), note.trim() || null, priority);
      to = "";
      amount = "";
      note = "";
      quote = null;
      await refresh();
    } catch (e) {
      err = String(e);
    } finally {
      sending = false;
    }
  }

  async function saveNode() {
    err = null;
    try {
      await api.setOwnNode(ownNode.trim() || null);
      editingNode = false;
      await refresh();
    } catch (e) {
      err = String(e);
    }
  }

  function eta(secs: number | null): string {
    if (secs == null) return "";
    if (secs < 90) return "about a minute";
    if (secs < 3600) return `about ${Math.round(secs / 60)} minutes`;
    return `about ${(secs / 3600).toFixed(1)} hours`;
  }
</script>

<h1 class="page-title">Wallet</h1>
<p class="page-lede">Monero, kept here. Nobody else holds these keys{#if view?.stagenet}{" — "}stagenet coin for now, so the amounts are practice{/if}.</p>

{#if view}
  <div class="card">
    <div class="balance-row">
      <div>
        <div class="balance-big">{fmtXmr(view.balances.spendable_pxmr)}</div>
        <div class="meta">
          spendable{#if view.fiat_spendable}{" · "}{view.fiat_spendable.text}{view.fiat_spendable.notional ? " (notional)" : ""}{view.fiat_spendable.stale ? " · stale rate" : ""}{/if}
          {#if view.balances.locked_pxmr > 0} · {fmtXmr(view.balances.locked_pxmr)} arriving{#if view.balances.blocks_to_unlock > 0}, {view.balances.blocks_to_unlock} block{view.balances.blocks_to_unlock === 1 ? "" : "s"} to go{/if}{/if}
        </div>
      </div>
      <div class="sync">
        {#if view.blocker === "NoWallet"}
          <span class="pill warn">Minting a wallet…</span>
        {:else if view.blocker === "NoNode"}
          <span class="pill warn">Looking for a node…</span>
        {:else if view.blocker === "Failing"}
          <span class="pill warn" title={view.balances.error ?? ""}>Node trouble</span>
        {:else if view.balances.syncing}
          <span class="pill">Catching up · {Math.round(view.balances.progress * 100)}% · {view.balances.blocks_left} blocks{eta(view.balances.seconds_left) ? " · " + eta(view.balances.seconds_left) : ""}</span>
        {:else}
          <span class="pill ok">Up to date · block {view.balances.tip}</span>
        {/if}
      </div>
    </div>
    {#if view.balances.syncing}
      <div class="bar"><div class="bar-fill" style={`width:${Math.round(view.balances.progress * 100)}%`}></div></div>
    {/if}
    {#if view.balances.error}<p class="err">{view.balances.error}</p>{/if}
  </div>

  <div class="tabs">
    <button class="tab" class:active={tab === "receive"} onclick={() => (tab = "receive")}>Receive</button>
    <button class="tab" class:active={tab === "send"} onclick={() => (tab = "send")}>Send</button>
    <button class="tab" class:active={tab === "history"} onclick={() => (tab = "history")}>History</button>
  </div>

  {#if tab === "receive"}
    <div class="card">
      <h3>Your address</h3>
      {#if view.address}
        <div class="code-wrap">
          <div class="qr">{@html view.address_svg}</div>
          <div class="code-side">
            <p class="note">Anyone can pay this. A contact who pays you through DUCAT gets their own subaddress automatically, so their payments stand out.</p>
            <div class="addr">{view.address}</div>
            <div class="actions"><button class="btn small" onclick={() => copy(view?.address ?? "")}>Copy address</button></div>
          </div>
        </div>
      {:else}
        <p class="empty">The wallet is minted the first time a node answers.</p>
      {/if}
    </div>
  {:else if tab === "send"}
    <div class="card">
      <h3>Send</h3>
      <div class="field">
        <label for="to">To</label>
        <input id="to" class="input" bind:value={to} placeholder="A Monero address" />
      </div>
      <div class="field">
        <label for="amt">Amount</label>
        <input id="amt" class="input" bind:value={amount} oninput={requote} placeholder="0.00 XMR" />
        <button class="btn small" onclick={sendMax}>Max</button>
      </div>
      <div class="field">
        <label for="note">Note</label>
        <input id="note" class="input" bind:value={note} placeholder="For your own records (optional)" />
      </div>
      <div class="field">
        <label for="prio">Speed</label>
        <select id="prio" class="input" bind:value={priority} onchange={requote}>
          <option value={0}>Slow · ~20 min</option>
          <option value={1}>Normal · ~6 min</option>
          <option value={2}>Fast · ~4 min</option>
          <option value={3}>Fastest · ~2 min</option>
        </select>
      </div>
      {#if quote}
        <p class="note">
          {#if quote.fee_known}Fee {fmtXmr(quote.fee_pxmr)} over {quote.notes} note{quote.notes === 1 ? "" : "s"} · total {fmtXmr(quote.total_pxmr)} · {quote.affordable ? `${fmtXmr(quote.remaining_pxmr)} left after` : "more than is unlocked"}{:else}The fee is not known yet — no node has answered.{/if}
        </p>
      {:else if quoting}
        <p class="note">Working out the fee…</p>
      {/if}
      <div class="actions">
        <button class="btn primary" disabled={!to.trim() || !amount.trim() || sending || (quote !== null && !quote.affordable)} onclick={send}>{sending ? "Sending…" : amount.trim() ? `Send ${amount.trim()} XMR` : "Send now"}</button>
      </div>
      {#if sentTx}<p class="note ok-text">Sent. Transaction {sentTx.slice(0, 16)}…</p>{/if}
      {#if err}<p class="err">{err}</p>{/if}
    </div>
  {:else}
    <div class="card">
      <h3>Sent</h3>
      {#if sends.length === 0}<p class="empty">Nothing sent yet.</p>{/if}
      {#each sends as s (s.txid_hex + s.timestamp)}
        <div class="row">
          <div class="lead">
            <div class="title">{fmtXmr(s.amount_pxmr)} <span class="meta">· fee {fmtXmr(s.fee_pxmr)}{s.recovered ? " · recovered from the chain" : ""}{s.donation ? " · donation" : ""}</span></div>
            <div class="meta">{fmtTime(s.timestamp)} · to {s.contact_name ?? s.to_address.slice(0, 16) + "…"}{s.note ? ` · ${s.note}` : ""}</div>
          </div>
          {#if s.txid_hex}<div class="addr">{s.txid_hex}</div>{/if}
        </div>
      {/each}
    </div>
    <div class="card">
      <h3>Received <button class="btn small" onclick={() => (showNotes = !showNotes)}>{showNotes ? "Hide" : "Show"} notes</button></h3>
      {#if showNotes}
        {#if notes.length === 0}<p class="empty">No notes yet.</p>{/if}
        {#each notes as n (n.tx_hash_hex + n.minor + n.height)}
          <div class="row">
            <div class="lead">
              <div class="title">{fmtXmr(n.amount_pxmr)} <span class="meta">· {n.spent ? "spent" : n.unlocked ? "spendable" : "locked"}{n.from ? ` · from ${n.from}` : n.minor ? ` · subaddress ${n.minor}` : ""}</span></div>
              <div class="meta">block {n.height}{n.timestamp ? ` · ${fmtTime(n.timestamp)}` : ""}</div>
            </div>
            <div class="addr">{n.tx_hash_hex}</div>
          </div>
        {/each}
      {/if}
    </div>
  {/if}

  <div class="card">
    <h3>Node</h3>
    {#if editingNode}
      <div class="field">
        <label for="node">Your node</label>
        <input id="node" class="input" bind:value={ownNode} placeholder="http://host:38089" onkeydown={(e) => e.key === "Enter" && saveNode()} />
        <button class="btn" onclick={saveNode}>Use it</button>
        <button class="btn" onclick={() => { editingNode = false; ownNode = view?.own_node ?? ""; }}>Cancel</button>
      </div>
    {:else}
      <p class="note">
        {#if view.node}Asking <span class="mono">{view.node}</span>{view.own_node ? " (your node)" : " (a public node — it sees your address, not your keys)"}{:else}No node yet.{/if}
        <button class="linkish" onclick={() => (editingNode = true)}>Change</button>
        {#if view.own_node}<button class="linkish" onclick={() => { ownNode = ""; saveNode(); }}>Forget mine</button>{/if}
      </p>
      <div class="field">
        <span class="meta">Rescan from block</span>
        <input class="input narrow" bind:value={rescanFrom} placeholder={String(view.restore_height)} />
        <button class="btn small" disabled={rescanning} onclick={rescan}>{rescanning ? "Rescanning…" : "Rescan"}</button>
        <span class="meta">Reads the chain again from there; the notes and the balance come back as the scan reaches them.</span>
      </div>
    {/if}
  </div>
{:else}
  <p class="empty">Opening the wallet…</p>
{/if}
