<script lang="ts">
  import { icons } from "./icons";
  // The kiosk: orders at a counter. Each has a number, a code any Monero
  // wallet can pay — the total carries six digits of noise so the payment
  // is recognised — and, if the customer has DUCAT, a card that turns the
  // order into a bill with a receipt.
  import { onMount } from "svelte";
  import { t, tp } from "./i18n.svelte";
  import { api, copy, fmtXmr, fmtTime, type CounterRisk, type ItemRow, type OrderRow, confirmDanger } from "./api";
  import { gen } from "./state.svelte";

  let items = $state<ItemRow[]>([]);
  let orders = $state<OrderRow[]>([]);
  let err = $state<string | null>(null);
  let busy = $state<string | null>(null);
  let lines = $state<{ d: string; a: number }[]>([]);
  let lineName = $state("");
  let linePrice = $state("");
  let withCard = $state(true);
  let open = $state<string | null>(null);
  // How much of a stranger's word this counter takes. Held as the operator
  // typed it, not reformatted under their cursor.
  let risk = $state<CounterRisk | null>(null);
  let sightCap = $state("");
  let floor = $state("");
  let riskErr = $state<string | null>(null);

  const current = $derived(orders.find((o) => o.id === open) ?? null);
  const total = $derived(lines.reduce((s, l) => s + l.a, 0));

  // The empty state waits for the first answer; a blank list is not
  // the same as an empty one.
  let loaded = $state(false);
  async function refresh() {
    try {
      items = await api.catalogue();
      orders = await api.orders();
    } catch (e) {
      err = String(e);
    }
  }

  /** Bare, for a field somebody edits — fmtXmr is for reading. */
  const bare = (pxmr: number) => (pxmr > 0 ? fmtXmr(pxmr).replace(" XMR", "") : "");

  async function saveRisk() {
    riskErr = null;
    try {
      await api.setCounterRisk(sightCap, floor);
      risk = await api.counterRisk();
    } catch (e) {
      riskErr = String(e);
    }
  }

  onMount(async () => {
    await refresh();
    try {
      risk = await api.counterRisk();
      sightCap = bare(risk.sight_cap_pxmr);
      floor = bare(risk.small_sale_floor_pxmr);
    } catch (e) {
      riskErr = String(e);
    }
    loaded = true;
  });
  $effect(() => {
    void gen.value;
    refresh();
  });

  async function addTyped() {
    err = null;
    const a = await api.fiatToPxmr(linePrice);
    if (!lineName.trim() || !a) { err = t("desk_line_needs_price"); return; }
    lines = [...lines, { d: lineName.trim(), a }];
    lineName = ""; linePrice = "";
  }

  async function place() {
    err = null;
    busy = "place";
    try {
      const o = await api.placeOrder(lines.map((l) => [l.d, l.a] as [string, number]), null, withCard);
      lines = [];
      await refresh();
      open = o.id;
    } catch (e) {
      err = String(e);
    } finally {
      busy = null;
    }
  }

  async function act(key: string, fn: () => Promise<unknown>) {
    err = null;
    busy = key;
    try { await fn(); await refresh(); } catch (e) { err = String(e); } finally { busy = null; }
  }

  // §15.11: goods do not leave on a sighting alone. They may leave on one up
  // to the amount the operator said they would risk, and no further — a
  // payment in the mempool can still be replaced.
  const handsOver = (o: OrderRow) =>
    (risk?.sight_cap_pxmr ?? 0) > 0 && o.total_pxmr > 0 && o.total_pxmr <= (risk?.sight_cap_pxmr ?? 0);

  /** Paid, in the sense that the counter may hand the order over. */
  const released = (o: OrderRow) => o.state === "Confirmed" || (o.state === "Seen" && handsOver(o));

  function stateWord(o: OrderRow): string {
    switch (o.state) {
      case "Awaiting": return o.customer ? t("desk_billed_to", o.customer) : t("kiosk_state_awaiting");
      // Seen is not paid: it said "paid — seen, not yet on the chain" and the
      // counter read the first word.
      case "Seen": return handsOver(o)
        ? t("bartab_state_paid")
        : `${t("kiosk_state_seen")} · ${t("pos_settling_blocks", o.blocks, o.blocks_needed)}`;
      case "Confirmed": return o.ready_at ? t("desk_paid_called_ready") : t("bartab_state_paid");
      case "Abandoned": return t("kiosk_state_abandoned");
      default: return o.state;
    }
  }
</script>

<div class="page-head">
  <h1 class="page-title">{t("kiosk_mode_title")}</h1>
</div>
<p class="page-lede">{t("desk_kiosk_lede")}</p>

<div class="till-grid kiosk-grid">
  <div class="card">
    <h3>{t("desk_new_order")}</h3>
    <div class="chips">
      {#each items.filter((i) => !i.sold_out && i.pxmr) as i (i.id)}
        <button class="chip" onclick={() => (lines = [...lines, { d: i.name, a: i.pxmr! }])}>{i.name} · {i.price} {i.currency}</button>
      {/each}
    </div>
    {#each lines as l, i}
      <div class="bill-line row-line"><span>{l.d}</span><span>{fmtXmr(l.a)} <button class="linkish" onclick={() => (lines = lines.filter((_, j) => j !== i))}>{@html icons.close}</button></span></div>
    {/each}
    <div class="field">
      <input class="input" placeholder={t("desk_line")} bind:value={lineName} />
      <input class="input narrow" placeholder={t("desk_price")} bind:value={linePrice} onkeydown={(e) => e.key === "Enter" && addTyped()} />
      <button class="btn" onclick={addTyped}>{t("pos_add_line")}</button>
    </div>
    <div class="total-row"><span>{t("kiosk_total")}</span><strong>{fmtXmr(total)}</strong></div>
    <label class="toggle"><input type="checkbox" bind:checked={withCard} /> {t("desk_also_cut_card")}</label>
    <div class="actions"><button class="btn primary" disabled={lines.length === 0 || busy === "place"} onclick={place}>{busy === "place" ? t("desk_placing") : t("desk_place_order")}</button></div>
    {#if err}<p class="err">{err}</p>{/if}

    <h4>{t("kiosk_orders")}</h4>
    {#each orders as o (o.id)}
      <button class="thread-row" class:active={open === o.id} onclick={() => (open = o.id)}>
        {#if o.customer_avatar_data_url}<img class="avatar pic" src={o.customer_avatar_data_url} alt="" />{:else}<div class="avatar" class:group={o.state !== "Confirmed"}>#{o.number}</div>{/if}
        <div class="thread-text">
          <div class="thread-top"><span class="thread-name">{o.customer_avatar_data_url ? `#${o.number} · ` : ""}{o.shown.primary}</span><span class="thread-when">{fmtTime(o.placed_at)}</span></div>
          <div class="thread-last">{stateWord(o)} · {o.lines.map(([d]) => d).join(", ")}</div>
        </div>
      </button>
    {/each}
    {#if loaded && orders.length === 0}<p class="empty">{t("kiosk_no_orders")}</p>{/if}

    <h4>{t("kiosk_sight_title")}</h4>
    <p class="note">{t("kiosk_sight_note")}</p>
    <div class="field">
      <input class="input narrow" placeholder={t("items_up_to")} bind:value={sightCap} onchange={saveRisk} />
    </div>
    <h4>{t("items_floor_title")}</h4>
    <p class="note">{t("items_floor_note")}</p>
    <div class="field">
      <input class="input narrow" placeholder={t("items_up_to")} bind:value={floor} onchange={saveRisk} />
    </div>
    {#if riskErr}<p class="err">{riskErr}</p>{/if}
  </div>

  <div class="card sale-status">
    {#if current}
      <h3>{t("kiosk_paid_number", current.number)}</h3>
      <div class="balance-big">{current.shown.primary}</div>
      <div class="meta">{current.shown.secondary ?? ""} · {stateWord(current)}</div>
      <div class="bill">
        {#each current.lines as [d, a]}<div class="bill-line"><span>{d}</span><span>{fmtXmr(a)}</span></div>{/each}
        {#if current.tax_pxmr}<div class="bill-line"><span>{t("pos_tax")}</span><span>{fmtXmr(current.tax_pxmr)}</span></div>{/if}
      </div>
      {#if current.state === "Seen" && !handsOver(current)}
        <div class="settling">
          <span class="settle-spin"></span>
          <div>
            <div><strong>{t("kiosk_settling")}</strong> · {t("pos_settling_blocks", current.blocks, current.blocks_needed)}</div>
            <div class="meta">{t("kiosk_settling_note")}</div>
          </div>
        </div>
      {/if}
      {#if current.state === "Awaiting" && !current.customer}
        <div class="code-pair">
          {#if current.pay_svg}
            <div><div class="qr">{@html current.pay_svg}</div><div class="meta">{t("donate_tab_monero")}</div></div>
          {/if}
          {#if current.card_svg}
            <div><div class="qr">{@html current.card_svg}</div><div class="meta">{t("desk_card_means_bill")}</div></div>
          {:else}
            <button class="btn small" disabled={busy === "card"} onclick={() => act("card", () => api.orderCard(current!.id))}>{busy === "card" ? t("desk_cutting") : t("desk_add_ducat_card")}</button>
          {/if}
        </div>
        <div class="addr">{current.pay_uri}</div>
        <div class="actions"><button class="btn small" onclick={() => copy(current?.pay_uri ?? "")}>{t("desk_copy_pay_code")}</button>{#if current.card}<button class="btn small" onclick={() => copy(current?.card ?? "")}>{t("desk_copy_card")}</button>{/if}</div>
      {/if}
      <div class="actions">
        {#if released(current) && current.customer && !current.ready_at}
          <button class="btn primary" disabled={busy === "ready"} onclick={() => act("ready", () => api.sayReady(current!.id))}>{t("kiosk_say_ready")}</button>
        {/if}
        {#if current.state === "Awaiting"}
          <button class="btn danger" onclick={async () => { if (!(await confirmDanger(t("desk_confirm_abandon_order")))) return; act("abandon", () => api.abandonOrder(current!.id)); }}>{t("desk_abandon")}</button>
        {/if}
      </div>
    {:else}
      <p class="empty">{t("desk_place_or_pick")}</p>
    {/if}
  </div>
</div>

<style>
  /* Seen, and not yet money: the counter must read a wait here, not a tick. */
  .settling { display: flex; align-items: center; gap: 10px; margin: 8px 0; }
  .settle-spin {
    width: 14px; height: 14px; flex: none; border-radius: 50%;
    border: 2px solid var(--primary-soft); border-top-color: var(--primary);
    animation: settle-turn 0.9s linear infinite;
  }
  @keyframes settle-turn { to { transform: rotate(360deg); } }
</style>
