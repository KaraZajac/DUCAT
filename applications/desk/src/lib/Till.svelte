<script lang="ts">
  import { icons } from "./icons";
  // The till: a sale to whoever is in front of you, tabs that run, and the
  // catalogue both draw from. The phone's POS and Bar Tab, side by side.
  import { onMount } from "svelte";
  import { t, tp } from "./i18n.svelte";
  import { api, copy, fmtXmr, fmtTime, type Code, type ContactRow, type ItemRow, type TabRow } from "./api";
  import { gen } from "./state.svelte";

  type Mode = "sale" | "tabs" | "catalogue";
  let mode = $state<Mode>("sale");
  let err = $state<string | null>(null);
  let items = $state<ItemRow[]>([]);
  let tabs = $state<TabRow[]>([]);
  let contacts = $state<ContactRow[]>([]);

  // --- a sale: lines first, then the card, then the bill, then the wait
  type Line = { d: string; a: number };
  let saleLines = $state<Line[]>([]);
  let lineName = $state("");
  let linePrice = $state("");
  let lineXmr = $state("");
  let taxText = $state("");
  let card = $state<Code | null>(null);
  let presenting = $state(false);
  let saleTab = $state<TabRow | null>(null);

  const saleTotal = $derived(saleLines.reduce((s, l) => s + l.a, 0));

  // --- tabs
  let openTab = $state<TabRow | null>(null);
  let tabLineName = $state("");
  let tabLinePrice = $state("");
  let pickingContact = $state(false);

  // --- catalogue
  let newName = $state("");
  let newPrice = $state("");

  // The empty state waits for the first answer; a blank list is not
  // the same as an empty one.
  let loaded = $state(false);
  async function refresh() {
    try {
      items = await api.catalogue();
      tabs = await api.tabs();
      contacts = await api.contacts();
      if (saleTab) saleTab = tabs.find((t) => t.id === saleTab?.id) ?? saleTab;
      if (openTab) openTab = tabs.find((t) => t.id === openTab?.id) ?? null;
    } catch (e) {
      err = String(e);
    }
  }

  onMount(async () => { await refresh(); loaded = true; });

  $effect(() => {
    void gen.value;
    refresh();
  });

  async function pxmrOf(fiat: string, xmr: string): Promise<number | null> {
    if (xmr.trim()) {
      const n = Number(xmr);
      return Number.isFinite(n) && n > 0 ? Math.floor(n * 1e12) : null;
    }
    if (fiat.trim()) return await api.fiatToPxmr(fiat);
    return null;
  }

  async function addSaleLine() {
    err = null;
    const a = await pxmrOf(linePrice, lineXmr);
    if (!lineName.trim() || !a) { err = t("desk_line_needs_price_or_xmr"); return; }
    saleLines = [...saleLines, { d: lineName.trim(), a }];
    lineName = ""; linePrice = ""; lineXmr = "";
  }

  function addFromCatalogue(i: ItemRow) {
    if (!i.pxmr) { err = i.snag === "NoRate" ? "No exchange rate yet — the wallet fetches one shortly." : "That item cannot be priced right now."; return; }
    saleLines = [...saleLines, { d: i.name, a: i.pxmr }];
  }

  async function present() {
    err = null;
    presenting = true;
    try {
      const tax = taxText.trim() ? await pxmrOf(taxText, "") : null;
      const r = await api.presentSale(saleLines.map((l) => [l.d, l.a] as [string, number]), tax);
      card = r.code;
      saleTab = r.tab;
      saleLines = [];
      taxText = "";
    } catch (e) {
      err = String(e);
    } finally {
      presenting = false;
    }
  }

  function stopPresenting() {
    // The tab stays bound to its card until it is answered or swept; the
    // screen just stops watching.
    card = null;
    saleTab = null;
  }

  async function saleDone() {
    saleTab = null;
  }

  async function saleCancel() {
    if (!saleTab) return;
    err = null;
    try {
      await api.cancelTab(saleTab.id);
      saleTab = null;
    } catch (e) {
      err = String(e);
    }
  }

  async function salePaidOutside() {
    if (!saleTab) return;
    err = null;
    try {
      const t = await api.tabPaidOutside(saleTab.id);
      if (t) saleTab = t;
    } catch (e) {
      err = String(e);
    }
  }

  // --- tabs
  async function startTab(c: ContactRow) {
    err = null;
    try {
      openTab = await api.openTab(c.persona_hex, "bar");
      pickingContact = false;
      await refresh();
    } catch (e) {
      err = String(e);
    }
  }

  async function addTabLine() {
    if (!openTab) return;
    err = null;
    const a = await pxmrOf(tabLinePrice, "");
    if (!tabLineName.trim() || !a) { err = t("desk_line_needs_price"); return; }
    try {
      openTab = await api.tabAddLine(openTab.id, tabLineName.trim(), a);
      tabLineName = ""; tabLinePrice = "";
    } catch (e) {
      err = String(e);
    }
  }

  async function addTabItem(i: ItemRow) {
    if (!openTab || !i.pxmr) return;
    openTab = await api.tabAddLine(openTab.id, i.name, i.pxmr);
  }

  async function settleOpen() {
    if (!openTab) return;
    err = null;
    try {
      openTab = await api.settleTab(openTab.id);
      await refresh();
    } catch (e) {
      err = String(e);
    }
  }

  async function act(fn: () => Promise<unknown>) {
    err = null;
    try {
      await fn();
      await refresh();
    } catch (e) {
      err = String(e);
    }
  }

  function stateWord(tab: TabRow): string {
    switch (tab.state) {
      case "open": return t("bartab_section_running").toLowerCase();
      case "settled": return tab.seen_tx ? t("bartab_state_payment_seen") : t("bartab_state_billed_unpaid");
      case "paid": return tab.receipt_owed ? t("desk_paid_receipt_owed") : t("bartab_state_paid");
      case "paid_oob": return tab.receipt_owed ? t("desk_paid_oob_receipt_owed") : t("bartab_state_paid_oob");
      case "cancelled": return t("bartab_state_cancelled");
      default: return tab.state;
    }
  }

  async function addItem() {
    err = null;
    try {
      await api.putItem(null, newName, newPrice, false);
      newName = ""; newPrice = "";
      await refresh();
    } catch (e) {
      err = String(e);
    }
  }
</script>

<div class="page-head">
  <h1 class="page-title">{t("desk_nav_till")}</h1>
  <div class="tabs" style="margin: 0">
    <button class="tab" class:active={mode === "sale"} onclick={() => (mode = "sale")}>{t("pos_sale")}</button>
    <button class="tab" class:active={mode === "tabs"} onclick={() => (mode = "tabs")}>{t("bartab_open_tabs")}</button>
    <button class="tab" class:active={mode === "catalogue"} onclick={() => (mode = "catalogue")}>{t("items_tab")}</button>
  </div>
</div>

{#if mode === "sale"}
  {#if saleTab && saleTab.state !== "open"}
    <div class="card sale-status">
      <h3>{saleTab.name}</h3>
      <div class="balance-big">{saleTab.shown.primary}</div>
      {#if saleTab.shown.secondary}<div class="meta">{saleTab.shown.secondary}</div>{/if}
      <p class="pill" class:ok={saleTab.state === "paid" || saleTab.state === "paid_oob"}>{stateWord(saleTab)}</p>
      <div class="bill">
        {#each saleTab.lines as [d, a]}<div class="bill-line"><span>{d}</span><span>{fmtXmr(a)}</span></div>{/each}
        {#if saleTab.tax_pxmr}<div class="bill-line"><span>{t("pos_tax")}</span><span>{fmtXmr(saleTab.tax_pxmr)}</span></div>{/if}
        {#if saleTab.tip_pxmr > 0}<div class="bill-line"><span>{t("kiosk_tip")}</span><span>{fmtXmr(saleTab.tip_pxmr)}</span></div>{/if}
      </div>
      <div class="actions">
        {#if saleTab.state === "settled"}
          <button class="btn" onclick={salePaidOutside}>{t("bartab_paid_outside_button")}</button>
          <button class="btn danger" onclick={saleCancel}>{t("bartab_cancel_bill")}</button>
        {:else}
          {#if saleTab.receipt_owed}<button class="btn" onclick={() => act(() => api.tabSendReceipt(saleTab!.id))}>{t("bartab_send_receipt")}</button>{/if}
          <button class="btn primary" onclick={saleDone}>{t("kiosk_next_customer")}</button>
        {/if}
      </div>
      {#if err}<p class="err">{err}</p>{/if}
    </div>
  {:else if card && saleTab}
    <div class="card">
      <h3>{t("desk_scan_to_pay", fmtXmr(saleTab.total_pxmr))}</h3>
      <div class="code-wrap">
        <div class="qr">{@html card.svg}</div>
        <div class="code-side">
          <p class="note">{t("desk_sale_scan_note")}</p>
          <div class="bill">
            {#each saleTab.lines as [d, a]}<div class="bill-line"><span>{d}</span><span>{fmtXmr(a)}</span></div>{/each}
          </div>
          <div class="addr">{card.uri}</div>
          <div class="actions">
            <button class="btn small" onclick={() => copy(card?.uri ?? "")}>{t("desk_copy_code")}</button>
            <button class="btn small" onclick={stopPresenting}>{t("pos_back")}</button>
          </div>
        </div>
      </div>
      {#if err}<p class="err">{err}</p>{/if}
    </div>
  {:else}
    <div class="till-grid">
      <div class="card">
        <h3>{t("pos_this_sale")}</h3>
        {#if saleLines.length === 0}<p class="empty">{t("desk_sale_empty")}</p>{/if}
        {#each saleLines as l, i}
          <div class="bill-line row-line"><span>{l.d}</span><span>{fmtXmr(l.a)} <button class="linkish" onclick={() => (saleLines = saleLines.filter((_, j) => j !== i))}>{@html icons.close}</button></span></div>
        {/each}
        <div class="field">
          <input class="input" placeholder={t("desk_line")} bind:value={lineName} />
          <input class="input narrow" placeholder={t("desk_price")} bind:value={linePrice} title={t("desk_in_your_currency")} />
          <input class="input narrow" placeholder={t("desk_or_xmr")} bind:value={lineXmr} />
          <button class="btn" onclick={addSaleLine}>{t("pos_add_line")}</button>
        </div>
        <div class="field">
          <label for="tax">{t("pos_tax")}</label>
          <input id="tax" class="input narrow" placeholder="0.00" bind:value={taxText} />
        </div>
        <div class="total-row"><span>{t("pay_total")}</span><strong>{fmtXmr(saleTotal)}</strong></div>
        <div class="actions">
          <button class="btn primary" disabled={saleLines.length === 0 || presenting} onclick={present}>{presenting ? t("pos_getting_code_ready") : t("desk_present")}</button>
        </div>
        {#if err}<p class="err">{err}</p>{/if}
      </div>
      <div class="card">
        <h3>{t("items_tab")}</h3>
        {#if loaded && items.length === 0}<p class="empty">{t("desk_no_items_yet")}</p>{/if}
        <div class="chips">
          {#each items.filter((i) => !i.sold_out) as i (i.id)}
            <button class="chip" onclick={() => addFromCatalogue(i)} title={i.pxmr ? fmtXmr(i.pxmr) : t("items_no_rate")}>{i.name} · {i.price} {i.currency}</button>
          {/each}
        </div>
      </div>
    </div>
  {/if}

{:else if mode === "tabs"}
  <div class="till-grid">
    <div class="card">
      <div class="page-head" style="margin-bottom: 8px">
        <h3 style="margin: 0">{t("bartab_open_tabs")}</h3>
        <button class="btn small" onclick={() => (pickingContact = !pickingContact)}>{pickingContact ? t("chat_close") : t("bartab_start_tab")}</button>
      </div>
      {#if pickingContact}
        <p class="note">{t("desk_whose_tab")}</p>
        {#each contacts as c (c.persona_hex)}
          <button class="thread-row" onclick={() => startTab(c)}><div class="avatar">{c.name.slice(0, 1).toUpperCase()}</div><div class="thread-text"><div class="thread-name">{c.name}</div></div></button>
        {/each}
        {#if loaded && contacts.length === 0}<p class="empty">{t("desk_no_contacts")}</p>{/if}
      {/if}
      {#each tabs.filter((x) => x.state === "open" || x.state === "settled" || x.receipt_owed) as tb (tb.id)}
        <button class="thread-row" class:active={openTab?.id === tb.id} onclick={() => (openTab = tb)}>
          <div class="thread-text">
            <div class="thread-top"><span class="thread-name">{tb.name}{tb.origin !== "bar" ? ` · ${tb.origin}` : ""}</span><span class="thread-when">{tb.shown.primary}</span></div>
            <div class="thread-last">{stateWord(tb)} · {t("desk_opened_at", fmtTime(Math.floor(tb.opened_at / 1000)))}</div>
          </div>
        </button>
      {/each}
      {#if tabs.filter((x) => x.state !== "open" && x.state !== "settled" && !x.receipt_owed).length}
        <details><summary class="meta">{t("bartab_section_settled")}</summary>
          {#each tabs.filter((x) => x.state !== "open" && x.state !== "settled" && !x.receipt_owed) as tb (tb.id)}
            <div class="row"><div class="lead"><div class="title">{tb.name} <span class="meta">· {tb.shown.primary} · {stateWord(tb)}</span></div><div class="meta">{fmtTime(Math.floor(tb.opened_at / 1000))}</div></div><div class="actions"><button class="btn small" onclick={() => act(() => api.deleteTab(tb.id))}>Clear</button></div></div>
          {/each}
        </details>
      {/if}
    </div>
    <div class="card">
      {#if openTab}
        <h3>{openTab.name} <span class="meta">· {stateWord(openTab)}</span></h3>
        <div class="bill">
          {#each openTab.lines as [d, a], i}
            <div class="bill-line row-line"><span>{d}</span><span>{fmtXmr(a)} {#if openTab.state === "open"}<button class="linkish" onclick={() => act(() => api.tabRemoveLine(openTab!.id, i))}>{@html icons.close}</button>{/if}</span></div>
          {/each}
          {#if openTab.tax_pxmr}<div class="bill-line"><span>{t("pos_tax")}</span><span>{fmtXmr(openTab.tax_pxmr)}</span></div>{/if}
          {#if openTab.tip_pxmr > 0}<div class="bill-line"><span>{t("kiosk_tip")}</span><span>{fmtXmr(openTab.tip_pxmr)}</span></div>{/if}
        </div>
        <div class="total-row"><span>{t("pay_total")}</span><strong>{openTab.shown.primary}{openTab.shown.secondary ? ` · ${openTab.shown.secondary}` : ""}</strong></div>
        {#if openTab.state === "open"}
          <div class="chips">
            {#each items.filter((i) => !i.sold_out && i.pxmr) as i (i.id)}<button class="chip" onclick={() => addTabItem(i)}>{i.name}</button>{/each}
          </div>
          <div class="field">
            <input class="input" placeholder={t("desk_line")} bind:value={tabLineName} />
            <input class="input narrow" placeholder={t("desk_price")} bind:value={tabLinePrice} />
            <button class="btn" onclick={addTabLine}>{t("pos_add_line")}</button>
          </div>
          <div class="actions">
            <button class="btn primary" disabled={openTab.lines.length === 0} onclick={settleOpen}>{t("desk_settle_send_bill")}</button>
            <button class="btn danger" onclick={() => act(async () => { await api.deleteTab(openTab!.id); openTab = null; })}>{t("bartab_discard_confirm")}</button>
          </div>
        {:else if openTab.state === "settled"}
          <p class="note">{openTab.seen_tx ? t("desk_payment_in_mempool") : t("desk_bill_with_them")}</p>
          <div class="actions">
            <button class="btn" onclick={() => act(() => api.tabPaidOutside(openTab!.id))}>{t("bartab_paid_outside_button")}</button>
            <button class="btn danger" onclick={() => act(() => api.cancelTab(openTab!.id))}>{t("bartab_cancel_bill")}</button>
          </div>
        {:else}
          {#if openTab.receipt_owed}<div class="actions"><button class="btn" onclick={() => act(() => api.tabSendReceipt(openTab!.id))}>{t("bartab_send_receipt")}</button></div>{/if}
        {/if}
        {#if err}<p class="err">{err}</p>{/if}
      {:else}
        <p class="empty">{t("desk_pick_a_tab")}</p>
        {#if err}<p class="err">{err}</p>{/if}
      {/if}
    </div>
  </div>

{:else}
  <div class="card">
    <h3>{t("items_title")}</h3>
    <p class="note">{t("desk_catalogue_note")}</p>
    {#each items as i (i.id)}
      <div class="row">
        <div class="lead">
          <div class="title">{i.name} <span class="meta">· {i.price} {i.currency}{i.pxmr ? ` · ${fmtXmr(i.pxmr)} ${t("desk_now")}` : i.snag === "NoRate" ? ` · ${t("items_no_rate")}` : ""}</span></div>
        </div>
        <div class="actions">
          <button class="btn small" onclick={() => act(() => api.putItem(i.id, i.name, i.price, !i.sold_out))}>{i.sold_out ? t("items_back_on") : t("items_sold_out")}</button>
          <button class="btn small danger" onclick={() => act(() => api.removeItem(i.id))}>{t("items_remove")}</button>
        </div>
      </div>
    {/each}
    <div class="field">
      <input class="input" placeholder={t("items_name")} bind:value={newName} />
      <input class="input narrow" placeholder={t("desk_price")} bind:value={newPrice} onkeydown={(e) => e.key === "Enter" && addItem()} />
      <button class="btn" onclick={addItem} disabled={!newName.trim() || !newPrice.trim()}>{t("items_add")}</button>
    </div>
    {#if err}<p class="err">{err}</p>{/if}
  </div>
{/if}
