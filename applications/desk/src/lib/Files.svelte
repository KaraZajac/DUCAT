<script lang="ts">
  // Releases: a file put out once, at an address that cannot change.
  import { onMount } from "svelte";
  import { t, tp } from "./i18n.svelte";
  import { api, copy, fmtBytes, fmtWhen, type Progress, type ReleaseRow } from "./api";
  import PieceBar from "./PieceBar.svelte";

  let rows = $state<ReleaseRow[]>([]);
  let err = $state<string | null>(null);
  let busy = $state<string | null>(null);
  let addr = $state("");
  let over = $state(false);
  let progress = $state<Record<string, Progress>>({});

  async function refresh() {
    try {
      rows = await api.releases();
    } catch (e) {
      err = String(e);
    }
  }

  onMount(() => {
    refresh();
    const t = setInterval(async () => {
      // Only the fetches in flight are polled; a list at rest asks nothing.
      for (const r of rows) {
        if (busy === r.digest_hex) progress[r.share_key] = await api.fetchProgress(r.share_key);
      }
    }, 800);
    return () => clearInterval(t);
  });

  async function share(path: string) {
    err = null;
    busy = "share";
    try {
      await api.shareFile(path, "");
      await refresh();
    } catch (e) {
      err = String(e);
    } finally {
      busy = null;
    }
  }

  async function pick() {
    const p = await api.pickFile();
    if (p) await share(p);
  }

  async function add() {
    err = null;
    try {
      await api.addRelease(addr.trim());
      addr = "";
      await refresh();
    } catch (e) {
      err = String(e);
    }
  }

  async function get(r: ReleaseRow) {
    err = null;
    busy = r.digest_hex;
    try {
      await api.fetchRelease(r.digest_hex);
      await refresh();
    } catch (e) {
      err = String(e);
    } finally {
      busy = null;
      delete progress[r.share_key];
    }
  }

  async function keep(r: ReleaseRow, on: boolean) {
    await api.setReleaseKeep(r.digest_hex, on);
    await refresh();
  }

  async function remove(r: ReleaseRow) {
    await api.removeRelease(r.digest_hex);
    await refresh();
  }

  function onDrop(e: DragEvent) {
    e.preventDefault();
    over = false;
    // Tauri hands real paths on drop through its own event; the DOM drop
    // carries names only. Fall back to the picker, which is one click.
    pick();
  }
</script>

<h1 class="page-title">{t("releases_title")}</h1>
<p class="page-lede">{t("desk_files_lede")}</p>

<div class="card">
  <div
    class="drop"
    class:over
    role="button"
    tabindex="0"
    ondragover={(e) => { e.preventDefault(); over = true; }}
    ondragleave={() => (over = false)}
    ondrop={onDrop}
    onclick={pick}
    onkeydown={(e) => e.key === "Enter" && pick()}
  >
    {busy === "share" ? t("desk_seeding") : t("desk_drop_file")}
  </div>
  <div class="field" style="margin-top: 14px">
    <label for="addr">{t("releases_add")}</label>
    <input id="addr" class="input" placeholder={t("releases_addr_hint")} bind:value={addr} onkeydown={(e) => e.key === "Enter" && add()} />
    <button class="btn" onclick={add} disabled={!addr.trim()}>{t("releases_add_go")}</button>
  </div>
  {#if err}<p class="err">{err}</p>{/if}
</div>

<div class="card">
  {#if rows.length === 0}
    <p class="empty">{t("desk_nothing_here")}</p>
  {/if}
  {#each rows as r (r.digest_hex)}
    <div class="row">
      <div class="lead">
        <div class="title">{r.title || t("desk_untitled")} <span class="meta">· {fmtBytes(r.bytes)}{r.mine ? ` · ${t("desk_shared_from_here")}` : ""}</span></div>
        <div class="meta">{r.here ? t("releases_here") : t("sites_not_fetched")}{r.added_at ? ` · ${fmtWhen(r.added_at)}` : ""}</div>
      </div>
      <div class="actions">
        <button class="btn small" onclick={() => copy(r.uri)}>{t("desk_copy_address")}</button>
        {#if r.here}
          <button class="btn small" onclick={() => api.reveal(r.dir)}>{t("desk_show")}</button>
        {:else}
          <button class="btn small primary" disabled={busy !== null} onclick={() => get(r)}>{busy === r.digest_hex ? t("releases_fetching") : t("releases_get")}</button>
        {/if}
        <label class="toggle"><input type="checkbox" checked={r.keep_alive} onchange={(e) => keep(r, (e.target as HTMLInputElement).checked)} /> {t("desk_keep_alive")}</label>
        <button class="btn small danger" onclick={() => remove(r)}>{t("releases_remove")}</button>
      </div>
      <div class="addr">{r.uri}</div>
      {#if busy === r.digest_hex}
        <div class="wide"><PieceBar progress={progress[r.share_key] ?? null} /></div>
      {/if}
    </div>
  {/each}
  <p class="note">{t("desk_keep_alive_note")}</p>
</div>
