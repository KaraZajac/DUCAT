<script lang="ts">
  // Sites: one mutable head at a stable address, a bundle on the swarm.
  import { onMount } from "svelte";
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

  async function refresh() {
    try {
      rows = await api.sites();
    } catch (e) {
      err = String(e);
    }
  }

  onMount(() => {
    refresh();
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
  <h1 class="page-title">Sites</h1>
  <button class="btn primary" onclick={() => { showPublish = !showPublish; if (!showPublish) { updating = null; dir = null; lint = null; title = ""; } }}>
    {showPublish ? "Close" : "Publish a site…"}
  </button>
</div>
<p class="page-lede">
  Pages that travel like publications: a folder with an <code>index.html</code>, put on the network
  at an address you keep for the life of the site. Update it and readers see the new edition at the
  same address.
</p>

{#if showPublish || updating}
<div class="card">
  <h3>{updating ? "Update a site" : "Publish a site"}</h3>
  <div class="field">
    <label for="folder">Folder</label>
    <input id="folder" class="input" readonly value={dir ?? ""} placeholder="A folder with index.html at its root" />
    <button class="btn" onclick={chooseFolder}>Choose…</button>
  </div>
  <div class="field">
    <label for="title">Title</label>
    <input id="title" class="input" bind:value={title} placeholder="What readers will see" />
  </div>
  {#if lint}
    <p class="err">That page reaches the network — {lint}. A site is served from its bundle and nothing else; fix the reference and choose the folder again.</p>
  {:else if dir}
    <p class="note ok-text">Sealed: every reference stays inside the bundle.</p>
  {/if}
  <div class="actions" style="justify-content: flex-start; gap: 8px">
    <button class="btn primary" disabled={!dir || !!lint || busy === "publish"} onclick={publish}>
      {busy === "publish" ? "Seeding…" : updating ? "Publish the update" : "Publish"}
    </button>
    {#if updating}<button class="btn" onclick={() => { updating = null; showPublish = false; title = ""; dir = null; lint = null; }}>Cancel</button>{/if}
  </div>
  <p class="note">The bundle is checked for anything that reaches the clearnet before it is seeded, because one external fetch hands a reader's address to a third party.</p>
</div>
{/if}

<div class="card">
  <div class="field">
    <label for="saddr">Add by address</label>
    <input id="saddr" class="input" placeholder="ducat:site/…" bind:value={addr} onkeydown={(e) => e.key === "Enter" && add()} />
    <button class="btn" onclick={add} disabled={!addr.trim() || busy === "add"}>{busy === "add" ? "Reading…" : "Add"}</button>
  </div>
  {#if err}<p class="err">{err}</p>{/if}
</div>

<div class="card">
  {#if rows.length === 0}
    <p class="empty">No sites yet.</p>
  {/if}
  {#each rows as r (r.record_key)}
    <div class="row">
      <div class="lead">
        <div class="title">{r.title || "(untitled)"} {#if r.mine}<span class="meta">· yours</span>{/if}</div>
        <div class="meta">
          {r.cached ? (r.current ? "Ready offline" : "An older edition is on this desk") : "Not fetched"}
          {r.updated ? ` · updated ${fmtWhen(r.updated)}` : ""}
        </div>
      </div>
      <div class="actions">
        <button class="btn small" onclick={() => copy(r.uri)}>Copy address</button>
        <button class="btn small primary" disabled={busy !== null} onclick={() => open(r)}>{busy === r.record_key ? "Fetching…" : "Open"}</button>
        {#if r.mine}<button class="btn small" onclick={() => startUpdate(r)}>Update</button>{/if}
        <label class="toggle"><input type="checkbox" checked={r.keep_alive} onchange={(e) => keep(r, (e.target as HTMLInputElement).checked)} /> keep alive</label>
        <button class="btn small danger" onclick={() => remove(r)}>Remove</button>
      </div>
      <div class="addr">{r.uri}</div>
      {#if busy === r.record_key}
        <div class="wide"><PieceBar progress={progress[r.share] ?? null} /></div>
      {/if}
    </div>
  {/each}
</div>
