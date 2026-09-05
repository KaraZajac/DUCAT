<script lang="ts">
  // The library: what you publish, and what you read. A period's key
  // opens exactly one edition; a shelf is small and on the DHT, a
  // shipment is big and on the swarm.
  import { onMount } from "svelte";
  import { t, tp } from "./i18n.svelte";
  import { api, copy, fmtBytes, fmtXmr, fmtTime, type Code, type ContactRow, type PublicationRow, type SubscriptionRow } from "./api";
  import { gen, drive } from "./state.svelte";

  let mode = $state<"reading" | "press">("reading");
  let pubs = $state<PublicationRow[]>([]);
  let subs = $state<SubscriptionRow[]>([]);
  let contacts = $state<ContactRow[]>([]);
  let err = $state<string | null>(null);
  let busy = $state<string | null>(null);

  // press
  let selected = $state<string | null>(null);
  let newTitle = $state("");
  let priceText = $state("");
  let period = $state("");
  let file = $state<string | null>(null);
  let note = $state("");
  let preferSwarm = $state(false);
  let code = $state<Code | null>(null);
  let addingSub = $state(false);

  const current = $derived(pubs.find((p) => p.id === selected) ?? null);

  async function refresh() {
    try {
      pubs = await api.publications();
      subs = await api.subscriptions();
      contacts = await api.contacts();
      if (!selected && pubs.length) selected = pubs[0].id;
    } catch (e) {
      err = String(e);
    }
  }

  onMount(refresh);
  $effect(() => {
    void gen.value;
    refresh();
  });

  async function act(key: string, fn: () => Promise<unknown>) {
    err = null;
    busy = key;
    try {
      await fn();
      await refresh();
    } catch (e) {
      err = String(e);
    } finally {
      busy = null;
    }
  }

  async function create() {
    await act("create", async () => {
      selected = await api.createPublication(newTitle);
      newTitle = "";
    });
  }

  async function savePrice() {
    if (!current) return;
    const n = Number(priceText);
    if (!Number.isFinite(n) || n < 0) { err = "A price is a number of XMR (0 for free)."; return; }
    await act("price", () => api.setPublicationPrice(current!.id, Math.floor(n * 1e12)));
  }

  async function pickFile() {
    const p = await api.pickFile();
    if (p) file = p;
    if (p && !period) {
      const d = new Date();
      period = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`;
    }
  }

  async function publish() {
    if (!current || !file) return;
    await act("publish", async () => {
      const n = await api.publishIssue(current!.id, period, file!, preferSwarm, note);
      note = "";
      file = null;
      err = null;
      lastResult = `Published '${period}' — ${current!.price_pxmr > 0 ? `${n} bill(s) sent` : `sent to ${n} subscriber(s)`}.`;
    });
  }
  let lastResult = $state<string | null>(null);

  async function showCode() {
    if (!current) return;
    await act("code", async () => {
      code = await api.pressCode(current!.id);
    });
  }

  function periodState(r: { has_key: boolean; on_shelf: boolean; on_swarm: boolean; fetched_bytes: number | null; asked: boolean }): string {
    if (r.fetched_bytes != null) return `here · ${fmtBytes(r.fetched_bytes)}`;
    if (r.has_key && (r.on_shelf || r.on_swarm)) return "ready to fetch";
    if (r.has_key) return "key held, not on the shelf yet";
    if (r.asked) return "asked for";
    return "on their shelf";
  }
</script>

<div class="page-head">
  <h1 class="page-title">{t("section_library")}</h1>
  <div class="tabs" style="margin: 0">
    <button class="tab" class:active={mode === "reading"} onclick={() => (mode = "reading")}>{t("desk_reading")}</button>
    <button class="tab" class:active={mode === "press"} onclick={() => (mode = "press")}>{t("desk_press")}</button>
  </div>
</div>

{#if mode === "reading"}
  {#if subs.length === 0}
    <div class="card"><p class="empty">{t("desk_shelf_empty")}</p></div>
  {/if}
  {#each subs as s (s.publisher_hex)}
    <div class="card" class:muted-card={s.muted}>
      <div class="page-head" style="margin-bottom: 6px">
        <h3 style="margin: 0">{s.name} {#if s.muted}<span class="meta">· {t("desk_muted")}</span>{/if}</h3>
        <div class="actions">
          {#if s.has_shelf}<button class="btn small" disabled={busy === "shelf" + s.publisher_hex} onclick={() => act("shelf" + s.publisher_hex, () => api.refreshShelf(s.publisher_hex))}>{busy === "shelf" + s.publisher_hex ? t("desk_reading") : t("desk_check_shelf")}</button>{/if}
          <label class="toggle"><input type="checkbox" checked={s.mirror} onchange={(e) => act("mirror", () => api.setMirroring(s.publisher_hex, (e.target as HTMLInputElement).checked))} /> {t("desk_mirror")}</label>
          <button class="btn small" onclick={() => act("mute", () => api.setMuted(s.publisher_hex, !s.muted))}>{s.muted ? t("desk_unmute") : t("desk_mute")}</button>
        </div>
      </div>
      {#if s.shelf_seen_at}<p class="meta">{t("desk_shelf_read_at", fmtTime(Math.floor(s.shelf_seen_at / 1000)))}</p>{/if}
      {#each s.periods as r (r.period)}
        <div class="row">
          <div class="lead">
            <div class="title">{r.period} <span class="meta">· {periodState(r)}{r.on_shelf && r.shelf_bytes ? ` · ${t("desk_on_the_shelf", fmtBytes(r.shelf_bytes))}` : ""}{r.on_swarm ? ` · ${t("desk_on_the_swarm")}` : ""}</span></div>
          </div>
          <div class="actions">
            {#if r.fetched_bytes != null}
              <button class="btn small" onclick={() => api.reveal(r.dir)}>{t("desk_show")}</button>
            {:else if r.has_key && (r.on_shelf || r.on_swarm)}
              <button class="btn small primary" disabled={busy === r.period + s.publisher_hex} onclick={() => act(r.period + s.publisher_hex, () => api.fetchIssue(s.publisher_hex, r.period))}>{busy === r.period + s.publisher_hex ? t("releases_fetching") : t("releases_get")}</button>
            {:else if !r.has_key && !r.asked}
              <button class="btn small" onclick={() => act("ask", () => api.askForPeriod(s.publisher_hex, r.period))}>{t("library_unlock_free")}</button>
            {/if}
          </div>
        </div>
      {/each}
      {#if s.periods.length === 0}<p class="empty">{t("desk_no_periods")}</p>{/if}
    </div>
  {/each}
  {#if err}<p class="err">{err}</p>{/if}

{:else}
  <div class="library-grid">
    <div class="card">
      <h3>{t("desk_your_publications")}</h3>
      {#each pubs as p (p.id)}
        <button class="thread-row" class:active={selected === p.id} onclick={() => { selected = p.id; code = null; priceText = ""; }}>
          <div class="thread-text">
            <div class="thread-top"><span class="thread-name">{p.title}</span><span class="thread-when">{p.price_pxmr ? fmtXmr(p.price_pxmr) : t("desk_free")}</span></div>
            <div class="thread-last">{tp("desk_issues_n", p.issues.length)} · {tp("desk_subscribers_n", p.subscribers.length)}</div>
          </div>
        </button>
      {/each}
      <div class="field">
        <input class="input" placeholder={t("desk_new_publication_hint")} bind:value={newTitle} onkeydown={(e) => e.key === "Enter" && create()} />
        <button class="btn" onclick={create} disabled={!newTitle.trim() || busy === "create"}>{t("pub_create")}</button>
      </div>
    </div>

    <div class="card">
      {#if current}
        <h3>{current.title}</h3>
        <div class="field">
          <label for="price">{t("desk_price_per_issue")}</label>
          <input id="price" class="input narrow" placeholder={current.price_pxmr ? (current.price_pxmr / 1e12).toString() : t("desk_zero_is_free")} bind:value={priceText} onkeydown={(e) => e.key === "Enter" && savePrice()} />
          <span class="meta">XMR</span>
          <button class="btn small" onclick={savePrice} disabled={!priceText.trim()}>{t("pub_price_set")}</button>
        </div>
        <p class="meta">{current.price_pxmr ? t("desk_priced_note", fmtXmr(current.price_pxmr)) : t("desk_free_note")}</p>

        <h4>{t("pub_roster_title")}</h4>
        {#each current.subscribers as c (c.persona_hex)}
          <div class="row"><div class="lead"><div class="title">{c.name}</div></div><div class="actions"><button class="btn small" onclick={() => act("sub", () => api.setSubscriber(current!.id, c.persona_hex, false))}>{t("items_remove")}</button></div></div>
        {/each}
        <div class="actions">
          <button class="btn small" onclick={() => (addingSub = !addingSub)}>{addingSub ? t("chat_close") : t("desk_add_a_contact")}</button>
          <button class="btn small" onclick={showCode} disabled={busy === "code"}>{busy === "code" ? t("desk_cutting") : t("pub_sub_code")}</button>
        </div>
        {#if addingSub}
          <div class="chips">
            {#each contacts.filter((c) => !current!.subscribers.some((s) => s.persona_hex === c.persona_hex)) as c (c.persona_hex)}
              <button class="chip" onclick={() => act("sub", () => api.setSubscriber(current!.id, c.persona_hex, true))}>{c.name}</button>
            {/each}
          </div>
        {/if}
        {#if code}
          <div class="code-wrap" style="margin-top: 10px">
            <div class="qr">{@html code.svg}</div>
            <div class="code-side">
              <p class="note">{t("desk_sub_code_note")}</p>
              <div class="addr">{code.uri}</div>
              <div class="actions"><button class="btn small" onclick={() => copy(code?.uri ?? "")}>{t("desk_copy_code")}</button></div>
            </div>
          </div>
        {/if}

        <h4>{t("pub_issues_title")}</h4>
        {#each current.issues as i (i.period)}
          <div class="row">
            <div class="lead">
              <div class="title">{i.period} <span class="meta">· {fmtBytes(i.bytes)} · {i.on_shelf ? t("desk_shelf_word") : ""}{i.on_shelf && i.on_swarm ? ` ${t("desk_and")} ` : ""}{i.on_swarm ? t("desk_on_the_swarm") : ""}{!i.on_shelf && !i.on_swarm ? t("desk_not_published") : ""}</span></div>
              <div class="meta">{t("desk_sent_to_n", i.sent.length)}{current.price_pxmr ? ` · ${t("desk_billed_n", i.billed.length)}` : ""}</div>
            </div>
          </div>
        {/each}
        <div class="field">
          <label for="period">{t("desk_period")}</label>
          <input id="period" class="input narrow" placeholder="2026-09" bind:value={period} />
          <button class="btn" onclick={pickFile}>{file ? file.split(/[\\/]/).pop() : t("pub_choose_file")}</button>
        </div>
        {#if drive.on}
          <div class="field" hidden><label for="fpath">{t("desk_path")}</label><input id="fpath" class="input" hidden placeholder="/path/to/issue" onchange={(e) => (file = (e.target as HTMLInputElement).value)} /></div>
        {/if}
        <div class="field">
          <label for="note">{t("txdetail_note")}</label>
          <input id="note" class="input" placeholder={t("pub_note_label")} bind:value={note} />
        </div>
        <label class="toggle"><input type="checkbox" bind:checked={preferSwarm} /> {t("desk_prefer_swarm")}</label>
        <div class="actions">
          <button class="btn primary" disabled={!file || !period.trim() || busy === "publish"} onclick={publish}>{busy === "publish" ? t("pub_publishing") : t("pub_publish")}</button>
          <button class="btn small danger" onclick={() => act("del", async () => { await api.deletePublication(current!.id); selected = null; })}>{t("pub_delete_btn")}</button>
        </div>
        {#if lastResult}<p class="note ok-text">{lastResult}</p>{/if}
        {#if err}<p class="err">{err}</p>{/if}
      {:else}
        <p class="empty">{t("desk_create_or_pick_publication")}</p>
        {#if err}<p class="err">{err}</p>{/if}
      {/if}
    </div>
  </div>
{/if}
