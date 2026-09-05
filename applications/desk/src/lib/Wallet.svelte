<script lang="ts">
  // Wallet: what is here, what is on its way, where money comes in, and
  // one form to send it. Stagenet until a mainnet build exists.
  import { onMount } from "svelte";
  import { t, tp } from "./i18n.svelte";
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
    if (!Number.isFinite(h) || h < 0) { err = t("desk_height_whole"); return; }
    rescanning = true;
    try { await api.walletRescan(h); await refresh(); } catch (e) { err = String(e); } finally { rescanning = false; }
  }
  let tab = $state<"send" | "receive" | "history">("receive");
  let showNotes = $state(false);

  // The empty state waits for the first answer; a blank list is not
  // the same as an empty one.
  let loaded = $state(false);
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
    refresh().then(() => (loaded = true));
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
    if (secs < 90) return tp("balance_unlock_minutes", 1);
    if (secs < 3600) return tp("balance_unlock_minutes", Math.round(secs / 60));
    return tp("balance_unlock_hours", Math.round(secs / 3600));
  }
</script>

<h1 class="page-title">{t("monero_wallet_title")}</h1>
<p class="page-lede">{view?.stagenet ? t("desk_wallet_lede_stagenet") : t("desk_wallet_lede")}</p>

{#if view}
  <div class="card">
    <div class="balance-row">
      <div>
        <div class="balance-big">{fmtXmr(view.balances.spendable_pxmr)}</div>
        <div class="meta">
          {t("desk_spendable")}{#if view.fiat_spendable}{" · "}{view.fiat_spendable.text}{view.fiat_spendable.notional ? ` (${t("desk_notional")})` : ""}{view.fiat_spendable.stale ? ` · ${t("desk_stale_rate")}` : ""}{/if}
          {#if view.balances.locked_pxmr > 0} · {t("desk_arriving", fmtXmr(view.balances.locked_pxmr))}{#if view.balances.blocks_to_unlock > 0}, {tp("monero_blocks_to_go", view.balances.blocks_to_unlock)}{/if}{/if}
        </div>
      </div>
      <div class="sync">
        {#if view.blocker === "NoWallet"}
          <span class="pill warn">{t("desk_minting")}</span>
        {:else if view.blocker === "NoNode"}
          <span class="pill warn">{t("monero_finding_node")}</span>
        {:else if view.blocker === "Failing"}
          <span class="pill warn" title={view.balances.error ?? ""}>{t("desk_node_trouble")}</span>
        {:else if view.balances.syncing}
          <span class="pill">{t("desk_catching_up")} · {Math.round(view.balances.progress * 100)}% · {tp("monero_blocks_to_go", view.balances.blocks_left)}{eta(view.balances.seconds_left) ? " · " + eta(view.balances.seconds_left) : ""}</span>
        {:else}
          <span class="pill ok">{t("desk_up_to_date")} · {t("txdetail_block_n", view.balances.tip)}</span>
        {/if}
      </div>
    </div>
    {#if view.balances.syncing}
      <div class="bar"><div class="bar-fill" style={`width:${Math.round(view.balances.progress * 100)}%`}></div></div>
    {/if}
    {#if view.balances.error}<p class="err">{view.balances.error}</p>{/if}
  </div>

  <div class="tabs">
    <button class="tab" class:active={tab === "receive"} onclick={() => (tab = "receive")}>{t("desk_receive")}</button>
    <button class="tab" class:active={tab === "send"} onclick={() => (tab = "send")}>{t("pay_send")}</button>
    <button class="tab" class:active={tab === "history"} onclick={() => (tab = "history")}>{t("desk_history")}</button>
  </div>

  {#if tab === "receive"}
    <div class="card">
      <h3>{t("desk_your_address")}</h3>
      {#if view.address}
        <div class="code-wrap">
          <div class="qr">{@html view.address_svg}</div>
          <div class="code-side">
            <p class="note">{t("desk_address_note")}</p>
            <div class="addr">{view.address}</div>
            <div class="actions"><button class="btn small" onclick={() => copy(view?.address ?? "")}>{t("desk_copy_address")}</button></div>
          </div>
        </div>
      {:else}
        <p class="empty">{t("desk_wallet_minted_note")}</p>
      {/if}
    </div>
  {:else if tab === "send"}
    <div class="card">
      <h3>{t("pay_send")}</h3>
      <div class="field">
        <label for="to">{t("txdetail_to")}</label>
        <input id="to" class="input" bind:value={to} placeholder={t("desk_monero_address_hint")} />
      </div>
      <div class="field">
        <label for="amt">{t("pay_amount")}</label>
        <input id="amt" class="input" bind:value={amount} oninput={requote} placeholder="0.00 XMR" />
        <button class="btn small" onclick={sendMax}>{t("pay_max")}</button>
      </div>
      <div class="field">
        <label for="note">{t("txdetail_note")}</label>
        <input id="note" class="input" bind:value={note} placeholder={t("desk_note_hint")} />
      </div>
      <div class="field">
        <label for="prio">{t("pay_speed")}</label>
        <select id="prio" class="input" bind:value={priority} onchange={requote}>
          <option value={0}>{t("pay_speed_slow")} · ~20 min</option>
          <option value={1}>{t("pay_speed_normal")} · ~6 min</option>
          <option value={2}>{t("pay_speed_fast")} · ~4 min</option>
          <option value={3}>{t("pay_speed_fastest")} · ~2 min</option>
        </select>
      </div>
      {#if quote}
        <p class="note">
          {#if quote.fee_known}{t("txdetail_fee")} {fmtXmr(quote.fee_pxmr)} · {tp("balance_notes", quote.notes)} · {t("pay_total").toLowerCase()} {fmtXmr(quote.total_pxmr)} · {quote.affordable ? t("desk_left_after", fmtXmr(quote.remaining_pxmr)) : t("desk_more_than_unlocked")}{:else}The fee is not known yet — no node has answered.{/if}
        </p>
      {:else if quoting}
        <p class="note">{t("desk_working_fee")}</p>
      {/if}
      <div class="actions">
        <button class="btn primary" disabled={!to.trim() || !amount.trim() || sending || (quote !== null && !quote.affordable)} onclick={send}>{sending ? t("desk_sending") : amount.trim() ? t("desk_send_x_xmr", amount.trim()) : t("desk_send_now")}</button>
      </div>
      {#if sentTx}<p class="note ok-text">{t("desk_sent_tx", sentTx.slice(0, 16) + "…")}</p>{/if}
      {#if err}<p class="err">{err}</p>{/if}
    </div>
  {:else}
    <div class="card">
      <h3>{t("txdetail_payment_sent")}</h3>
      {#if loaded && sends.length === 0}<p class="empty">{t("desk_nothing_sent")}</p>{/if}
      {#each sends as s (s.txid_hex + s.timestamp)}
        <div class="row">
          <div class="lead">
            <div class="title">{fmtXmr(s.amount_pxmr)} <span class="meta">· {t("desk_fee_x", fmtXmr(s.fee_pxmr))}{s.recovered ? ` · ${t("desk_recovered")}` : ""}{s.donation ? ` · ${t("activity_donation_chip").toLowerCase()}` : ""}</span></div>
            <div class="meta">{fmtTime(s.timestamp)} · {t("pay_sending_to", s.contact_name ?? s.to_address.slice(0, 16) + "…")}{s.note ? ` · ${s.note}` : ""}</div>
          </div>
          {#if s.txid_hex}<div class="addr">{s.txid_hex}</div>{/if}
        </div>
      {/each}
    </div>
    <div class="card">
      <h3>{t("txdetail_payment_received")} <button class="btn small" onclick={() => (showNotes = !showNotes)}>{showNotes ? t("desk_hide_notes") : t("desk_show_notes")}</button></h3>
      {#if showNotes}
        {#if loaded && notes.length === 0}<p class="empty">{t("desk_no_notes")}</p>{/if}
        {#each notes as n (n.tx_hash_hex + n.minor + n.height)}
          <div class="row">
            <div class="lead">
              <div class="title">{fmtXmr(n.amount_pxmr)} <span class="meta">· {n.spent ? t("txdetail_spent") : n.unlocked ? t("desk_spendable") : t("desk_locked")}{n.from ? ` · ${t("activity_from", n.from).toLowerCase()}` : n.minor ? ` · ${t("desk_subaddress_n", n.minor)}` : ""}</span></div>
              <div class="meta">{t("txdetail_block_n", n.height)}{n.timestamp ? ` · ${fmtTime(n.timestamp)}` : ""}</div>
            </div>
            <div class="addr">{n.tx_hash_hex}</div>
          </div>
        {/each}
      {/if}
    </div>
  {/if}

  <div class="card">
    <h3>{t("monero_title")}</h3>
    {#if editingNode}
      <div class="field">
        <label for="node">{t("monero_your_node")}</label>
        <input id="node" class="input" bind:value={ownNode} placeholder="http://host:38089" onkeydown={(e) => e.key === "Enter" && saveNode()} />
        <button class="btn" onclick={saveNode}>{t("monero_use_it")}</button>
        <button class="btn" onclick={() => { editingNode = false; ownNode = view?.own_node ?? ""; }}>{t("monero_cancel")}</button>
      </div>
    {:else}
      <p class="note">
        {#if view.node}{t("desk_asking_node")} <span class="mono">{view.node}</span>{view.own_node ? ` (${t("monero_your_node").toLowerCase()})` : ` (${t("desk_public_node_note")})`}{:else}{t("monero_no_node")}.{/if}
        <button class="linkish" onclick={() => (editingNode = true)}>{t("monero_change_your_node")}</button>
        {#if view.own_node}<button class="linkish" onclick={() => { ownNode = ""; saveNode(); }}>{t("monero_back_to_public")}</button>{/if}
      </p>
      <div class="field">
        <span class="meta">{t("desk_rescan_from")}</span>
        <input class="input narrow" bind:value={rescanFrom} placeholder={String(view.restore_height)} />
        <button class="btn small" disabled={rescanning} onclick={rescan}>{rescanning ? t("desk_rescanning") : t("desk_rescan")}</button>
        <span class="meta">{t("desk_rescan_note")}</span>
      </div>
    {/if}
  </div>
{:else}
  <p class="empty">{t("desk_opening_wallet")}</p>
{/if}
