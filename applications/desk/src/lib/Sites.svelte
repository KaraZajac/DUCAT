<script lang="ts">
  // Sites: one mutable head at a stable address, a bundle on the swarm.
  import { onMount } from "svelte";
  import { t, tp } from "./i18n.svelte";
  import { api, copy, fmtWhen, type Progress, type SiteRow } from "./api";
  import PieceBar from "./PieceBar.svelte";

  let rows = $state<SiteRow[]>([]);
  let err = $state<string | null>(null);
  let busy = $state<string | null>(null);
  let addr = $state("");
  let progress = $state<Record<string, Progress>>({});

  // The publisher's form.
  let dir = $state<string | null>(null);
  let title = $state("");
  let lint = $state<string | null>(null);
  let updating = $state<string | null>(null);
  // The list comes first; the publisher's form opens on request.
  let showPublish = $state(false);

  // The empty state waits for the first answer; a blank list is not
  // the same as an empty one.
  let loaded = $state(false);
  async function refresh() {
    try {
      rows = await api.sites();
    } catch (e) {
      err = String(e);
    }
  }

  onMount(() => {
    refresh().then(() => (loaded = true));
    const t = setInterval(async () => {
      for (const r of rows) {
        if (busy === r.record_key) progress[r.share] = await api.fetchProgress(r.share);
      }
    }, 800);
    return () => clearInterval(t);
  });

  async function chooseFolder() {
    const p = await api.pickFolder();
    if (!p) return;
    dir = p;
    lint = await api.lintSite(p);
    if (!title) title = p.split(/[\\/]/).filter(Boolean).pop() ?? "";
  }

  async function publish() {
    if (!dir) return;
    err = null;
    busy = "publish";
    try {
      await api.publishSite(dir, title.trim() || "Untitled", updating ?? undefined);
      dir = null;
      title = "";
      lint = null;
      updating = null;
      showPublish = false;
      await refresh();
    } catch (e) {
      err = String(e);
    } finally {
      busy = null;
    }
  }

  function startUpdate(r: SiteRow) {
    showPublish = true;
    updating = r.record_key;
    title = r.title;
    dir = null;
    lint = null;
  }

  async function add() {
    err = null;
    busy = "add";
    try {
      await api.addSite(addr.trim());
      addr = "";
      await refresh();
    } catch (e) {
      err = String(e);
    } finally {
      busy = null;
    }
  }

  async function open(r: SiteRow) {
    err = null;
    busy = r.record_key;
    try {
      const p = await api.fetchSite(r.record_key);
      await refresh();
      await api.reveal(p);
    } catch (e) {
      err = String(e);
    } finally {
      busy = null;
      delete progress[r.share];
    }
  }

  async function keep(r: SiteRow, on: boolean) {
    await api.setSiteKeep(r.record_key, on);
    await refresh();
  }

  async function remove(r: SiteRow) {
    await api.removeSite(r.record_key);
    await refresh();
  }
</script>

<div class="page-head">
  <h1 class="page-title">{t("section_sites")}</h1>
  <button class="btn primary" onclick={() => { showPublish = !showPublish; if (!showPublish) { updating = null; dir = null; lint = null; title = ""; } }}>
    {showPublish ? t("chat_close") : t("desk_publish_site")}
  </button>
</div>
<p class="page-lede">{t("desk_sites_lede")}</p>

{#if showPublish || updating}
<div class="card">
  <h3>{updating ? t("desk_update_site") : t("desk_publish_site_title")}</h3>
  <div class="field">
    <label for="folder">{t("desk_folder")}</label>
    <input id="folder" class="input" readonly value={dir ?? ""} placeholder={t("desk_folder_hint")} />
    <button class="btn" onclick={chooseFolder}>{t("desk_choose")}</button>
  </div>
  <div class="field">
    <label for="title">{t("desk_title")}</label>
    <input id="title" class="input" bind:value={title} placeholder={t("desk_title_hint")} />
  </div>
  {#if lint}
    <p class="err">{t("desk_site_reaches_network", lint)}</p>
  {:else if dir}
    <p class="note ok-text">{t("desk_site_sealed")}</p>
  {/if}
  <div class="actions" style="justify-content: flex-start; gap: 8px">
    <button class="btn primary" disabled={!dir || !!lint || busy === "publish"} onclick={publish}>
      {busy === "publish" ? t("desk_seeding") : updating ? t("desk_publish_update") : t("desk_publish")}
    </button>
    {#if updating}<button class="btn" onclick={() => { updating = null; showPublish = false; title = ""; dir = null; lint = null; }}>{t("common_cancel")}</button>{/if}
  </div>
  <p class="note">{t("desk_site_check_note")}</p>
</div>
{/if}

<div class="card">
  <div class="field">
    <label for="saddr">{t("releases_add")}</label>
    <input id="saddr" class="input" placeholder={t("sites_uri_label")} bind:value={addr} onkeydown={(e) => e.key === "Enter" && add()} />
    <button class="btn" onclick={add} disabled={!addr.trim() || busy === "add"}>{busy === "add" ? t("desk_reading") : t("sites_add_confirm")}</button>
  </div>
  {#if err}<p class="err">{err}</p>{/if}
</div>

<div class="card">
  {#if loaded && rows.length === 0}
    <p class="empty">{t("sites_empty_title")}</p>
  {/if}
  {#each rows as r (r.record_key)}
    <div class="row">
      <div class="lead">
        <div class="title">{r.title || t("desk_untitled")} {#if r.mine}<span class="meta">· {t("desk_yours")}</span>{/if}</div>
        <div class="meta">
          {r.cached ? (r.current ? t("sites_offline_ready") : t("desk_older_edition")) : t("sites_not_fetched")}
          {r.updated ? ` · ${t("desk_updated_at", fmtWhen(r.updated))}` : ""}
        </div>
      </div>
      <div class="actions">
        <button class="btn small" onclick={() => copy(r.uri)}>{t("desk_copy_address")}</button>
        <button class="btn small primary" disabled={busy !== null} onclick={() => open(r)}>{busy === r.record_key ? t("releases_fetching") : t("sites_open")}</button>
        {#if r.mine}<button class="btn small" onclick={() => startUpdate(r)}>{t("desk_update")}</button>{/if}
        <label class="toggle"><input type="checkbox" checked={r.keep_alive} onchange={(e) => keep(r, (e.target as HTMLInputElement).checked)} /> {t("desk_keep_alive")}</label>
        <button class="btn small danger" onclick={() => remove(r)}>{t("sites_remove")}</button>
      </div>
      <div class="addr">{r.uri}</div>
      {#if busy === r.record_key}
        <div class="wide"><PieceBar progress={progress[r.share] ?? null} /></div>
      {/if}
    </div>
  {/each}
</div>
