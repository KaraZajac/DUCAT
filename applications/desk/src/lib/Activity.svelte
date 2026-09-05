<script lang="ts">
  // Activity: the money, as a ledger — what came in, what went out, what
  // it was for, and a running balance. Exported as CSV or JSON for the
  // books, which is a thing a desk is for.
  import { onMount } from "svelte";
  import { t, tp, i18n } from "./i18n.svelte";
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
      msg = t("desk_written_to", n, path);
    } catch (e) {
      err = String(e);
    }
  }

  function signed(n: number): string {
    return (n < 0 ? "−" : "+") + fmtXmr(Math.abs(n));
  }

  /// The row's second line: whatever applies, separated once.
  function metaOf(e: LedgerEvent): string {
    const parts: string[] = [];
    if (e.note) parts.push(e.note);
    if (e.items.length) parts.push(e.items.map((i) => i.d).join(", "));
    if (e.receipted) parts.push(e.receipt_by ? t("desk_receipt_from", e.receipt_by) : t("desk_receipt"));
    if (e.locked) parts.push(tp("activity_unlocks_blocks", e.unlocks_in_blocks));
    if (e.pending) parts.push(t("txdetail_status_broadcast"));
    if (e.provisional) parts.push(t("desk_change_on_way"));
    if (e.unexplained) parts.push(t("desk_not_ours"));
    if (e.fee_pxmr) parts.push(t("desk_fee_x", fmtXmr(e.fee_pxmr)));
    return parts.join(" · ");
  }

  /// The first line: who, in the reader's words, or the honest blank.
  function whoLine(e: LedgerEvent): string {
    if (e.counterparty) return e.direction === "Sent" ? t("activity_to", e.counterparty) : t("activity_from", e.counterparty);
    if (e.direction === "Sent") return e.address ? t("activity_to", e.address.slice(0, 12) + "…") : t("activity_sent_unrecorded");
    return t("activity_received_unknown");
  }

  function doorWord(o: string): string {
    switch (o) {
      case "pos": return t("activity_door_pos");
      case "bar": return t("activity_door_bar");
      case "pub": return t("activity_door_pub");
      case "donate": return t("activity_door_donate");
      case "kiosk": return t("desk_door_kiosk");
      case "taxi": return t("activity_door_taxi");
      default: return o;
    }
  }
</script>

<div class="page-head">
  <h1 class="page-title">{t("tab_activity")}</h1>
  <div class="tabs" style="margin: 0">
    <button class="tab" class:active={range === "today"} onclick={() => (range = "today")}>{t("activity_period_today")}</button>
    <button class="tab" class:active={range === "week"} onclick={() => (range = "week")}>{t("activity_period_7d")}</button>
    <button class="tab" class:active={range === "month"} onclick={() => (range = "month")}>{t("desk_period_30d")}</button>
    <button class="tab" class:active={range === "all"} onclick={() => (range = "all")}>{t("activity_period_all")}</button>
  </div>
</div>

{#if summary}
  <div class="stats">
    <div class="stat"><div class="stat-v">{fmtXmr(summary.in_pxmr)}</div><div class="meta">{t("activity_sum_in")} · {summary.in_count}</div></div>
    <div class="stat"><div class="stat-v">{fmtXmr(summary.out_pxmr)}</div><div class="meta">{t("activity_sum_out")} · {summary.out_count} · {t("activity_sum_fees")} {fmtXmr(summary.fees_pxmr)}</div></div>
    <div class="stat"><div class="stat-v">{signed(summary.net_pxmr)}</div><div class="meta">{t("activity_sum_net")}</div></div>
    {#if business && business.sales_count}<div class="stat"><div class="stat-v">{fmtXmr(business.sales_pxmr)}</div><div class="meta">{tp("desk_sales_at_till", business.sales_count)}{business.by_origin.map(([o, d]) => ` · ${doorWord(o)} ${d.count}`).join("")}{business.tax_collected_pxmr ? ` · ${t("activity_tax_collected").toLowerCase()} ${fmtXmr(business.tax_collected_pxmr)}` : ""}</div></div>{/if}
    {#if summary.donations_pxmr}<div class="stat"><div class="stat-v">{fmtXmr(summary.donations_pxmr)}</div><div class="meta">{t("desk_given")}</div></div>{/if}
  </div>
{/if}

<div class="card">
  <div class="page-head" style="margin-bottom: 8px">
    <h3 style="margin: 0">{t("desk_ledger")}</h3>
    <div class="actions"><button class="btn small" onclick={() => exportAs(false)}>{t("activity_export")}</button><button class="btn small" onclick={() => exportAs(true)}>{t("desk_export_json")}</button></div>
  </div>
  {#if msg}<p class="note ok-text">{msg}</p>{/if}
  {#if err}<p class="err">{err}</p>{/if}
  {#if shown.length === 0}<p class="empty">{t("activity_period_none")}</p>{/if}
  <div class="ledger">
    {#each shown as e (e.txid + ":" + e.height + ":" + e.timestamp + ":" + e.direction)}
      <button class="ledger-row" class:out={e.direction === "Sent"} onclick={() => (open = open === e.txid + e.timestamp ? null : e.txid + e.timestamp)}>
        <div class="lw">{e.timestamp ? fmtTime(e.timestamp) : e.pending ? t("desk_pending") : "—"}</div>
        <div class="lc">
          <div class="title">{whoLine(e)}{e.donation ? " · " + t("activity_donation_chip").toLowerCase() : ""}</div>
          <div class="meta">{metaOf(e)}</div>
        </div>
        <div class="la"><div class="title">{signed(e.net_pxmr)}</div><div class="meta">{t("desk_then_balance", fmtXmr(Math.max(0, e.balance_after_pxmr)))}</div></div>
      </button>
      {#if open === e.txid + e.timestamp}
        <div class="ledger-detail">
          {#if e.txid}<div class="addr">{e.txid}</div>{/if}
          <div class="meta">{t("txdetail_block").toLowerCase()} {e.height || "—"} · {e.source === "Notice" ? t("desk_source_notice") : e.source === "OurRecord" ? t("desk_source_record") : e.source === "Order" ? t("desk_source_order") : t("desk_source_unknown")}{e.tax_pxmr ? ` · ${t("txdetail_tax").toLowerCase()} ${fmtXmr(e.tax_pxmr)}` : ""}</div>
          {#if e.items.length}<div class="bill">{#each e.items as i}<div class="bill-line"><span>{i.d}</span><span>{fmtXmr(i.a)}</span></div>{/each}</div>{/if}
        </div>
      {/if}
    {/each}
  </div>
</div>
