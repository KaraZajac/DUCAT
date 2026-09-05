<script lang="ts">
  // Activity: the money, as a ledger — what came in, what went out, what
  // it was for, and a running balance. Exported as CSV or JSON for the
  // books, which is a thing a desk is for.
  import { onMount } from "svelte";
  import { api, fmtXmr, fmtTime, type BusinessSummary, type LedgerEvent, type LedgerSummary } from "./api";
  import { gen } from "./state.svelte";

  type Range = "today" | "week" | "month" | "all";
  let range = $state<Range>("month");
  let events = $state<LedgerEvent[]>([]);
  let summary = $state<LedgerSummary | null>(null);
  let business = $state<BusinessSummary | null>(null);
  let err = $state<string | null>(null);
  let msg = $state<string | null>(null);
  let open = $state<string | null>(null);

  function bounds(r: Range): [number, number] {
    const now = Math.floor(Date.now() / 1000);
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    const today = Math.floor(d.getTime() / 1000);
    switch (r) {
      case "today": return [today, now + 1];
      case "week": return [today - 6 * 86400, now + 1];
      case "month": return [today - 29 * 86400, now + 1];
      default: return [0, Number.MAX_SAFE_INTEGER];
    }
  }

  async function refresh() {
    try {
      const [from, to] = bounds(range);
      const r = await api.ledger(from, to);
      events = r.events;
      summary = r.summary;
      business = r.business;
    } catch (e) {
      err = String(e);
    }
  }

  onMount(refresh);
  $effect(() => {
    void gen.value;
    void range;
    refresh();
  });

  const shown = $derived.by(() => {
    const [from, to] = bounds(range);
    return events.filter((e) => e.pending || (e.timestamp >= from && e.timestamp < to) || (e.timestamp === 0 && range === "all"));
  });

  async function exportAs(json: boolean) {
    err = null; msg = null;
    const path = await api.pickSavePath(json ? "ducat-ledger.json" : "ducat-ledger.csv");
    if (!path) return;
    try {
      const n = await api.exportLedger(path, json);
      msg = `Written: ${n} bytes to ${path}`;
    } catch (e) {
      err = String(e);
    }
  }

  function signed(n: number): string {
    return (n < 0 ? "−" : "+") + fmtXmr(Math.abs(n));
  }
</script>

<div class="page-head">
  <h1 class="page-title">Activity</h1>
  <div class="tabs" style="margin: 0">
    <button class="tab" class:active={range === "today"} onclick={() => (range = "today")}>Today</button>
    <button class="tab" class:active={range === "week"} onclick={() => (range = "week")}>7 days</button>
    <button class="tab" class:active={range === "month"} onclick={() => (range = "month")}>30 days</button>
    <button class="tab" class:active={range === "all"} onclick={() => (range = "all")}>All</button>
  </div>
</div>

{#if summary}
  <div class="stats">
    <div class="stat"><div class="stat-v">{fmtXmr(summary.in_pxmr)}</div><div class="meta">in · {summary.in_count}</div></div>
    <div class="stat"><div class="stat-v">{fmtXmr(summary.out_pxmr)}</div><div class="meta">out · {summary.out_count} · fees {fmtXmr(summary.fees_pxmr)}</div></div>
    <div class="stat"><div class="stat-v">{signed(summary.net_pxmr)}</div><div class="meta">net</div></div>
    {#if business && business.sales_count}<div class="stat"><div class="stat-v">{fmtXmr(business.sales_pxmr)}</div><div class="meta">{business.sales_count} sale{business.sales_count === 1 ? "" : "s"} at the till{business.by_origin.map(([o, d]) => ` · ${o} ${d.count}`).join("")}{business.tax_collected_pxmr ? ` · tax ${fmtXmr(business.tax_collected_pxmr)}` : ""}</div></div>{/if}
    {#if summary.donations_pxmr}<div class="stat"><div class="stat-v">{fmtXmr(summary.donations_pxmr)}</div><div class="meta">given</div></div>{/if}
  </div>
{/if}

<div class="card">
  <div class="page-head" style="margin-bottom: 8px">
    <h3 style="margin: 0">Ledger</h3>
    <div class="actions"><button class="btn small" onclick={() => exportAs(false)}>Export CSV</button><button class="btn small" onclick={() => exportAs(true)}>Export JSON</button></div>
  </div>
  {#if msg}<p class="note ok-text">{msg}</p>{/if}
  {#if err}<p class="err">{err}</p>{/if}
  {#if shown.length === 0}<p class="empty">Nothing in this period.</p>{/if}
  <div class="ledger">
    {#each shown as e (e.txid + ":" + e.height + ":" + e.timestamp + ":" + e.direction)}
      <button class="ledger-row" class:out={e.direction === "Sent"} onclick={() => (open = open === e.txid + e.timestamp ? null : e.txid + e.timestamp)}>
        <div class="lw">{e.timestamp ? fmtTime(e.timestamp) : e.pending ? "pending" : "—"}</div>
        <div class="lc">
          <div class="title">{e.direction === "Sent" ? "To" : "From"} {e.counterparty ?? (e.direction === "Sent" ? (e.address ? e.address.slice(0, 12) + "…" : "somewhere") : "someone")}{e.donation ? " · donation" : ""}</div>
          <div class="meta">{e.note ?? ""}{e.items.length ? (e.note ? " · " : "") + e.items.map((i) => i.d).join(", ") : ""}{e.receipted ? " · receipt" + (e.receipt_by ? ` from ${e.receipt_by}` : "") : ""}{e.locked ? ` · unlocks in ${e.unlocks_in_blocks} blocks` : ""}{e.pending ? " · not on the chain yet" : ""}{e.provisional ? " · change on its way" : ""}{e.unexplained ? " · not one of ours?" : ""}</div>
        </div>
        <div class="la"><div class="title">{signed(e.net_pxmr)}</div><div class="meta">{e.fee_pxmr ? `fee ${fmtXmr(e.fee_pxmr)} · ` : ""}bal {fmtXmr(Math.max(0, e.balance_after_pxmr))}</div></div>
      </button>
      {#if open === e.txid + e.timestamp}
        <div class="ledger-detail">
          {#if e.txid}<div class="addr">{e.txid}</div>{/if}
          <div class="meta">block {e.height || "—"} · {e.source === "Notice" ? "named by their payment notice" : e.source === "OurRecord" ? "from this desk's record of the send" : "no note says who"}{e.tax_pxmr ? ` · tax ${fmtXmr(e.tax_pxmr)}` : ""}</div>
          {#if e.items.length}<div class="bill">{#each e.items as i}<div class="bill-line"><span>{i.d}</span><span>{fmtXmr(i.a)}</span></div>{/each}</div>{/if}
        </div>
      {/if}
    {/each}
  </div>
</div>
