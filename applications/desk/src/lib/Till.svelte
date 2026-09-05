<script lang="ts">
  // The till: a sale to whoever is in front of you, tabs that run, and the
  // catalogue both draw from. The phone's POS and Bar Tab, side by side.
  import { onMount } from "svelte";
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
  let claimTimer: ReturnType<typeof setInterval> | null = null;

  const saleTotal = $derived(saleLines.reduce((s, l) => s + l.a, 0));

  // --- tabs
  let openTab = $state<TabRow | null>(null);
  let tabLineName = $state("");
  let tabLinePrice = $state("");
  let pickingContact = $state(false);

  // --- catalogue
  let newName = $state("");
  let newPrice = $state("");

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

  onMount(() => {
    refresh();
    return () => { if (claimTimer) clearInterval(claimTimer); };
  });

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
    if (!lineName.trim() || !a) { err = "A line needs a name and a price (fiat needs a rate; XMR always works)."; return; }
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
      card = await api.saleCard();
      const inbox = card.inbox_key;
      const lines = saleLines;
      const tax = taxText.trim() ? await pxmrOf(taxText, "") : null;
      claimTimer = setInterval(async () => {
        try {
          const who = await api.cardClaimant(inbox);
          if (!who) return;
          if (claimTimer) clearInterval(claimTimer);
          claimTimer = null;
          let t = await api.openTab(who.persona_hex, "pos");
          for (const l of lines) t = await api.tabAddLine(t.id, l.d, l.a);
          if (tax) t = await api.tabSetTax(t.id, tax);
          saleTab = await api.settleTab(t.id);
          card = null;
          saleLines = [];
          taxText = "";
        } catch (e) {
          err = String(e);
        }
      }, 3000);
    } catch (e) {
      err = String(e);
    } finally {
      presenting = false;
    }
  }

  function stopPresenting() {
    if (claimTimer) clearInterval(claimTimer);
    claimTimer = null;
    card = null;
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
    if (!tabLineName.trim() || !a) { err = "A line needs a name and a price."; return; }
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

  function stateWord(t: TabRow): string {
    switch (t.state) {
      case "open": return "running";
      case "settled": return t.seen_tx ? "payment seen — settling" : "billed, waiting";
      case "paid": return t.receipt_owed ? "paid · receipt owed" : "paid";
      case "paid_oob": return t.receipt_owed ? "settled outside · receipt owed" : "settled outside";
      case "cancelled": return "cancelled";
      default: return t.state;
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
  <h1 class="page-title">Till</h1>
  <div class="tabs" style="margin: 0">
    <button class="tab" class:active={mode === "sale"} onclick={() => (mode = "sale")}>Sale</button>
    <button class="tab" class:active={mode === "tabs"} onclick={() => (mode = "tabs")}>Tabs</button>
    <button class="tab" class:active={mode === "catalogue"} onclick={() => (mode = "catalogue")}>Catalogue</button>
  </div>
</div>

{#if mode === "sale"}
  {#if saleTab}
    <div class="card sale-status">
      <h3>{saleTab.name}</h3>
      <div class="balance-big">{saleTab.shown.primary}</div>
      {#if saleTab.shown.secondary}<div class="meta">{saleTab.shown.secondary}</div>{/if}
      <p class="pill" class:ok={saleTab.state === "paid" || saleTab.state === "paid_oob"}>{stateWord(saleTab)}</p>
      <div class="bill">
        {#each saleTab.lines as [d, a]}<div class="bill-line"><span>{d}</span><span>{fmtXmr(a)}</span></div>{/each}
        {#if saleTab.tax_pxmr}<div class="bill-line"><span>Tax</span><span>{fmtXmr(saleTab.tax_pxmr)}</span></div>{/if}
        {#if saleTab.tip_pxmr > 0}<div class="bill-line"><span>Tip</span><span>{fmtXmr(saleTab.tip_pxmr)}</span></div>{/if}
      </div>
      <div class="actions">
        {#if saleTab.state === "settled"}
          <button class="btn" onclick={salePaidOutside}>Paid another way</button>
          <button class="btn danger" onclick={saleCancel}>Cancel the bill</button>
        {:else}
          {#if saleTab.receipt_owed}<button class="btn" onclick={() => act(() => api.tabSendReceipt(saleTab!.id))}>Send the receipt</button>{/if}
          <button class="btn primary" onclick={saleDone}>Next customer</button>
        {/if}
      </div>
      {#if err}<p class="err">{err}</p>{/if}
    </div>
  {:else if card}
    <div class="card">
      <h3>Scan to pay {fmtXmr(saleTotal)}</h3>
      <div class="code-wrap">
        <div class="qr">{@html card.svg}</div>
        <div class="code-side">
          <p class="note">The customer scans this with their phone. As soon as they do, the bill goes to them and this screen follows the payment.</p>
          <div class="bill">
            {#each saleLines as l}<div class="bill-line"><span>{l.d}</span><span>{fmtXmr(l.a)}</span></div>{/each}
          </div>
          <div class="addr">{card.uri}</div>
          <div class="actions">
            <button class="btn small" onclick={() => copy(card?.uri ?? "")}>Copy code</button>
            <button class="btn small" onclick={stopPresenting}>Back</button>
          </div>
        </div>
      </div>
      {#if err}<p class="err">{err}</p>{/if}
    </div>
  {:else}
    <div class="till-grid">
      <div class="card">
        <h3>This sale</h3>
        {#if saleLines.length === 0}<p class="empty">Add lines from the catalogue, or type one.</p>{/if}
        {#each saleLines as l, i}
          <div class="bill-line row-line"><span>{l.d}</span><span>{fmtXmr(l.a)} <button class="linkish" onclick={() => (saleLines = saleLines.filter((_, j) => j !== i))}>✕</button></span></div>
        {/each}
        <div class="field">
          <input class="input" placeholder="Line" bind:value={lineName} />
          <input class="input narrow" placeholder="Price" bind:value={linePrice} title="In your currency" />
          <input class="input narrow" placeholder="or XMR" bind:value={lineXmr} />
          <button class="btn" onclick={addSaleLine}>Add</button>
        </div>
        <div class="field">
          <label for="tax">Tax</label>
          <input id="tax" class="input narrow" placeholder="0.00" bind:value={taxText} />
        </div>
        <div class="total-row"><span>Total</span><strong>{fmtXmr(saleTotal)}</strong></div>
        <div class="actions">
          <button class="btn primary" disabled={saleLines.length === 0 || presenting} onclick={present}>{presenting ? "Cutting a code…" : "Present"}</button>
        </div>
        {#if err}<p class="err">{err}</p>{/if}
      </div>
      <div class="card">
        <h3>Catalogue</h3>
        {#if items.length === 0}<p class="empty">No items yet — add some under Catalogue.</p>{/if}
        <div class="chips">
          {#each items.filter((i) => !i.sold_out) as i (i.id)}
            <button class="chip" onclick={() => addFromCatalogue(i)} title={i.pxmr ? fmtXmr(i.pxmr) : "not priceable yet"}>{i.name} · {i.price} {i.currency}</button>
          {/each}
        </div>
      </div>
    </div>
  {/if}

{:else if mode === "tabs"}
  <div class="till-grid">
    <div class="card">
      <div class="page-head" style="margin-bottom: 8px">
        <h3 style="margin: 0">Running tabs</h3>
        <button class="btn small" onclick={() => (pickingContact = !pickingContact)}>{pickingContact ? "Close" : "Open a tab"}</button>
      </div>
      {#if pickingContact}
        <p class="note">Whose tab? Somebody you already know — a stranger scans a sale code instead.</p>
        {#each contacts as c (c.persona_hex)}
          <button class="thread-row" onclick={() => startTab(c)}><div class="avatar">{c.name.slice(0, 1).toUpperCase()}</div><div class="thread-text"><div class="thread-name">{c.name}</div></div></button>
        {/each}
        {#if contacts.length === 0}<p class="empty">No contacts yet.</p>{/if}
      {/if}
      {#each tabs.filter((t) => t.state === "open" || t.state === "settled" || t.receipt_owed) as t (t.id)}
        <button class="thread-row" class:active={openTab?.id === t.id} onclick={() => (openTab = t)}>
          <div class="thread-text">
            <div class="thread-top"><span class="thread-name">{t.name}{t.origin !== "bar" ? ` · ${t.origin}` : ""}</span><span class="thread-when">{t.shown.primary}</span></div>
            <div class="thread-last">{stateWord(t)} · opened {fmtTime(Math.floor(t.opened_at / 1000))}</div>
          </div>
        </button>
      {/each}
      {#if tabs.filter((t) => t.state !== "open" && t.state !== "settled" && !t.receipt_owed).length}
        <details><summary class="meta">Closed tabs</summary>
          {#each tabs.filter((t) => t.state !== "open" && t.state !== "settled" && !t.receipt_owed) as t (t.id)}
            <div class="row"><div class="lead"><div class="title">{t.name} <span class="meta">· {t.shown.primary} · {stateWord(t)}</span></div><div class="meta">{fmtTime(Math.floor(t.opened_at / 1000))}</div></div><div class="actions"><button class="btn small" onclick={() => act(() => api.deleteTab(t.id))}>Clear</button></div></div>
          {/each}
        </details>
      {/if}
    </div>
    <div class="card">
      {#if openTab}
        <h3>{openTab.name} <span class="meta">· {stateWord(openTab)}</span></h3>
        <div class="bill">
          {#each openTab.lines as [d, a], i}
            <div class="bill-line row-line"><span>{d}</span><span>{fmtXmr(a)} {#if openTab.state === "open"}<button class="linkish" onclick={() => act(() => api.tabRemoveLine(openTab!.id, i))}>✕</button>{/if}</span></div>
          {/each}
          {#if openTab.tax_pxmr}<div class="bill-line"><span>Tax</span><span>{fmtXmr(openTab.tax_pxmr)}</span></div>{/if}
          {#if openTab.tip_pxmr > 0}<div class="bill-line"><span>Tip</span><span>{fmtXmr(openTab.tip_pxmr)}</span></div>{/if}
        </div>
        <div class="total-row"><span>Total</span><strong>{openTab.shown.primary}{openTab.shown.secondary ? ` · ${openTab.shown.secondary}` : ""}</strong></div>
        {#if openTab.state === "open"}
          <div class="chips">
            {#each items.filter((i) => !i.sold_out && i.pxmr) as i (i.id)}<button class="chip" onclick={() => addTabItem(i)}>{i.name}</button>{/each}
          </div>
          <div class="field">
            <input class="input" placeholder="Line" bind:value={tabLineName} />
            <input class="input narrow" placeholder="Price" bind:value={tabLinePrice} />
            <button class="btn" onclick={addTabLine}>Add</button>
          </div>
          <div class="actions">
            <button class="btn primary" disabled={openTab.lines.length === 0} onclick={settleOpen}>Settle up — send the bill</button>
            <button class="btn danger" onclick={() => act(async () => { await api.deleteTab(openTab!.id); openTab = null; })}>Discard</button>
          </div>
        {:else if openTab.state === "settled"}
          <p class="note">{openTab.seen_tx ? "Their payment is in the mempool — the receipt goes out when it lands." : "The bill is with them. This follows the payment; nothing to do."}</p>
          <div class="actions">
            <button class="btn" onclick={() => act(() => api.tabPaidOutside(openTab!.id))}>Paid another way</button>
            <button class="btn danger" onclick={() => act(() => api.cancelTab(openTab!.id))}>Cancel the bill</button>
          </div>
        {:else}
          {#if openTab.receipt_owed}<div class="actions"><button class="btn" onclick={() => act(() => api.tabSendReceipt(openTab!.id))}>Send the receipt</button></div>{/if}
        {/if}
        {#if err}<p class="err">{err}</p>{/if}
      {:else}
        <p class="empty">Pick a tab, or open one.</p>
        {#if err}<p class="err">{err}</p>{/if}
      {/if}
    </div>
  </div>

{:else}
  <div class="card">
    <h3>Catalogue</h3>
    <p class="note">Prices in your currency; each sale converts at the rate of the moment.</p>
    {#each items as i (i.id)}
      <div class="row">
        <div class="lead">
          <div class="title">{i.name} <span class="meta">· {i.price} {i.currency}{i.pxmr ? ` · ${fmtXmr(i.pxmr)} now` : i.snag === "NoRate" ? " · no rate yet" : ""}</span></div>
        </div>
        <div class="actions">
          <button class="btn small" onclick={() => act(() => api.putItem(i.id, i.name, i.price, !i.sold_out))}>{i.sold_out ? "Back in stock" : "Sold out"}</button>
          <button class="btn small danger" onclick={() => act(() => api.removeItem(i.id))}>Remove</button>
        </div>
      </div>
    {/each}
    <div class="field">
      <input class="input" placeholder="Name" bind:value={newName} />
      <input class="input narrow" placeholder="Price" bind:value={newPrice} onkeydown={(e) => e.key === "Enter" && addItem()} />
      <button class="btn" onclick={addItem} disabled={!newName.trim() || !newPrice.trim()}>Add</button>
    </div>
    {#if err}<p class="err">{err}</p>{/if}
  </div>
{/if}
