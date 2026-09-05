<script lang="ts">
  // The kiosk: orders at a counter. Each has a number, a code any Monero
  // wallet can pay — the total carries six digits of noise so the payment
  // is recognised — and, if the customer has DUCAT, a card that turns the
  // order into a bill with a receipt.
  import { onMount } from "svelte";
  import { api, copy, fmtXmr, fmtTime, type ItemRow, type OrderRow } from "./api";
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

  const current = $derived(orders.find((o) => o.id === open) ?? null);
  const total = $derived(lines.reduce((s, l) => s + l.a, 0));

  async function refresh() {
    try {
      items = await api.catalogue();
      orders = await api.orders();
    } catch (e) {
      err = String(e);
    }
  }

  onMount(refresh);
  $effect(() => {
    void gen.value;
    refresh();
  });

  async function addTyped() {
    err = null;
    const a = await api.fiatToPxmr(linePrice);
    if (!lineName.trim() || !a) { err = "A line needs a name and a price in your currency."; return; }
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

  function stateWord(o: OrderRow): string {
    switch (o.state) {
      case "Awaiting": return o.customer ? `billed to ${o.customer}` : "waiting for payment";
      case "Seen": return "payment seen — settling";
      case "Confirmed": return o.ready_at ? "paid · called as ready" : "paid";
      case "Abandoned": return "abandoned";
      default: return o.state;
    }
  }
</script>

<div class="page-head">
  <h1 class="page-title">Kiosk</h1>
</div>
<p class="page-lede">Orders at a counter. Every order gets a number and a code any Monero wallet can pay; a customer with DUCAT can answer its card instead and get a receipt.</p>

<div class="till-grid kiosk-grid">
  <div class="card">
    <h3>New order</h3>
    <div class="chips">
      {#each items.filter((i) => !i.sold_out && i.pxmr) as i (i.id)}
        <button class="chip" onclick={() => (lines = [...lines, { d: i.name, a: i.pxmr! }])}>{i.name} · {i.price} {i.currency}</button>
      {/each}
    </div>
    {#each lines as l, i}
      <div class="bill-line row-line"><span>{l.d}</span><span>{fmtXmr(l.a)} <button class="linkish" onclick={() => (lines = lines.filter((_, j) => j !== i))}>✕</button></span></div>
    {/each}
    <div class="field">
      <input class="input" placeholder="Line" bind:value={lineName} />
      <input class="input narrow" placeholder="Price" bind:value={linePrice} onkeydown={(e) => e.key === "Enter" && addTyped()} />
      <button class="btn" onclick={addTyped}>Add</button>
    </div>
    <div class="total-row"><span>Total</span><strong>{fmtXmr(total)}</strong></div>
    <label class="toggle"><input type="checkbox" bind:checked={withCard} /> also cut a DUCAT card for it</label>
    <div class="actions"><button class="btn primary" disabled={lines.length === 0 || busy === "place"} onclick={place}>{busy === "place" ? "Placing…" : "Place the order"}</button></div>
    {#if err}<p class="err">{err}</p>{/if}

    <h4>Orders</h4>
    {#each orders as o (o.id)}
      <button class="thread-row" class:active={open === o.id} onclick={() => (open = o.id)}>
        <div class="avatar" class:group={o.state !== "Confirmed"}>#{o.number}</div>
        <div class="thread-text">
          <div class="thread-top"><span class="thread-name">{o.shown.primary}</span><span class="thread-when">{fmtTime(o.placed_at)}</span></div>
          <div class="thread-last">{stateWord(o)} · {o.lines.map(([d]) => d).join(", ")}</div>
        </div>
      </button>
    {/each}
    {#if orders.length === 0}<p class="empty">No orders yet.</p>{/if}
  </div>

  <div class="card sale-status">
    {#if current}
      <h3>Order #{current.number}</h3>
      <div class="balance-big">{current.shown.primary}</div>
      <div class="meta">{current.shown.secondary ?? ""} · {stateWord(current)}</div>
      <div class="bill">
        {#each current.lines as [d, a]}<div class="bill-line"><span>{d}</span><span>{fmtXmr(a)}</span></div>{/each}
        {#if current.tax_pxmr}<div class="bill-line"><span>Tax</span><span>{fmtXmr(current.tax_pxmr)}</span></div>{/if}
      </div>
      {#if current.state === "Awaiting" && !current.customer}
        <div class="code-pair">
          {#if current.pay_svg}
            <div><div class="qr">{@html current.pay_svg}</div><div class="meta">Any Monero wallet</div></div>
          {/if}
          {#if current.card_svg}
            <div><div class="qr">{@html current.card_svg}</div><div class="meta">DUCAT: answer for a bill and a receipt</div></div>
          {:else}
            <button class="btn small" disabled={busy === "card"} onclick={() => act("card", () => api.orderCard(current!.id))}>{busy === "card" ? "Cutting…" : "Add a DUCAT card"}</button>
          {/if}
        </div>
        <div class="addr">{current.pay_uri}</div>
        <div class="actions"><button class="btn small" onclick={() => copy(current?.pay_uri ?? "")}>Copy the pay code</button>{#if current.card}<button class="btn small" onclick={() => copy(current?.card ?? "")}>Copy the card</button>{/if}</div>
      {/if}
      <div class="actions">
        {#if current.state === "Confirmed" && current.customer && !current.ready_at}
          <button class="btn primary" disabled={busy === "ready"} onclick={() => act("ready", () => api.sayReady(current!.id))}>Call it ready</button>
        {/if}
        {#if current.state === "Awaiting"}
          <button class="btn danger" onclick={() => act("abandon", () => api.abandonOrder(current!.id))}>Abandon</button>
        {/if}
      </div>
    {:else}
      <p class="empty">Place an order, or pick one.</p>
    {/if}
  </div>
</div>
