<script lang="ts">
  // The feed (§16.23): the posts of everyone you keep, newest first, with
  // your own among them — and the Post button that adds to yours.
  import { onMount } from "svelte";
  import { t, tp } from "./i18n.svelte";
  import { api, confirmDanger, fmtTime, type ContactRow, type FeedBlock, type HomeView, type TimelineRow, fmtBytes } from "./api";
  import { gen, drive } from "./state.svelte";
  import Busy from "./Busy.svelte";
  import { icons } from "./icons";

  let rows = $state<TimelineRow[]>([]);
  let people = $state<ContactRow[]>([]);
  let home = $state<HomeView | null>(null);
  let loaded = $state(false);
  let err = $state<string | null>(null);
  let msg = $state<string | null>(null);
  let composing = $state(false);
  let text = $state("");
  let media = $state<string[]>([]);
  let files = $state<string[]>([]);
  let posting = $state(false);
  let looking = $state(false);
  let thumbs = $state<Record<string, string>>({});

  const kept = $derived(people.filter((p) => p.hearted));

  async function refresh() {
    try {
      rows = await api.timeline(200);
      people = await api.contacts();
      home = await api.homeView();
      for (const r of rows) {
        for (const m of r.post.media ?? []) {
          const key = r.persona + "/" + m.path;
          if (thumbs[key] === undefined) {
            api.homeFileDataUrl(r.persona, m.path).then((u) => { if (u) thumbs[key] = u; }).catch(() => {});
          }
        }
      }
    } catch (e) {
      err = String(e);
    }
  }

  async function look() {
    err = null; msg = null;
    looking = true;
    try {
      const n = await api.refreshFeeds();
      await refresh();
      msg = n > 0 ? tp("feed_new_editions", n) : t("desk_feed_nothing_new");
    } catch (e) { err = String(e); } finally { looking = false; }
  }

  async function addPhotos(typed?: string) {
    const picked = typed ? [typed] : await api.pickFiles();
    media = [...media, ...picked].slice(0, 8);
  }

  async function addFiles(typed?: string) {
    const picked = typed ? [typed] : await api.pickFiles();
    files = [...files, ...picked].slice(0, 8);
  }

  async function post() {
    err = null; msg = null;
    posting = true;
    try {
      await api.postFeed(text, media, files);
      text = ""; media = []; files = []; composing = false;
      msg = t("desk_posted");
      await refresh();
    } catch (e) { err = String(e); } finally { posting = false; }
  }

  async function remove(r: TimelineRow) {
    if (!(await confirmDanger(t("desk_confirm_delete_post")))) return;
    err = null;
    try { await api.deletePost(r.post.id); await refresh(); } catch (e) { err = String(e); }
  }

  async function heart(p: ContactRow, on: boolean) {
    err = null;
    try { await api.setHeart(p.persona_hex, on); await refresh(); } catch (e) { err = String(e); }
  }

  async function openPost(r: TimelineRow) {
    err = null;
    try {
      const key = await api.homeKeyOf(r.persona);
      await api.openSiteRoom(key, `posts/${r.post.id}.html`);
    } catch (e) { err = String(e); }
  }

  async function saveFile(addr: string, name: string) {
    err = null;
    try { await api.addRelease(addr, name); msg = t("desk_saved_to_files", name); } catch (e) { err = String(e); }
  }

  function base(p: string): string {
    return p.split(/[\\/]/).pop() ?? p;
  }


  onMount(async () => { await refresh(); loaded = true; });
  $effect(() => { void gen.value; refresh(); });
</script>

<div class="page-head">
  <h1 class="page-title">{t("desk_feed")}</h1>
  <div class="actions">
    <button class="btn small" disabled={looking} onclick={look}>{looking ? t("releases_fetching") : t("desk_feed_refresh")}</button>
    <button class="btn primary" onclick={() => (composing = !composing)}>{composing ? t("chat_close") : t("desk_post")}</button>
  </div>
</div>

{#if composing}
  <div class="card">
    <h3>{t("desk_post")}</h3>
    <textarea class="input compose" rows="4" placeholder={t("desk_post_hint")} bind:value={text}></textarea>
    {#if media.length || files.length}
      <div class="chips">
        {#each media as m, i}<span class="chip">{@html icons.attach} {base(m)} <button class="linkish" onclick={() => (media = media.filter((_, j) => j !== i))}>{@html icons.close}</button></span>{/each}
        {#each files as f, i}<span class="chip">{@html icons.files} {base(f)} <button class="linkish" onclick={() => (files = files.filter((_, j) => j !== i))}>{@html icons.close}</button></span>{/each}
      </div>
    {/if}
    <div class="actions">
      <button class="btn small" onclick={() => addPhotos()}>{t("desk_add_photos")}</button>
      <button class="btn small" onclick={() => addFiles()}>{t("desk_add_files")}</button>
      <button class="btn primary" disabled={posting || (!text.trim() && !media.length && !files.length)} onclick={post}>{posting ? t("desk_posting") : t("desk_post")}</button>
      <Busy on={posting} />
    </div>
    {#if drive.on}
      <input id="feedmedia" class="input" hidden placeholder="/path/to/picture" onchange={(e) => addPhotos((e.target as HTMLInputElement).value)} />
      <input id="feedfile" class="input" hidden placeholder="/path/to/file" onchange={(e) => addFiles((e.target as HTMLInputElement).value)} />
    {/if}
    <p class="note">{t("desk_post_note")}</p>
  </div>
{/if}

{#if msg}<p class="note ok-text">{msg}</p>{/if}
{#if err}<p class="err">{err}</p>{/if}

<div class="feed-grid">
  <div>
    {#if loaded && rows.length === 0}
      <div class="card"><p class="empty">{t("desk_feed_empty")}</p></div>
    {/if}
    {#each rows as r (r.persona + ":" + r.post.id)}
      <div class="card post">
        <div class="post-head">
          {#if r.avatar_data_url}<img class="avatar pic" src={r.avatar_data_url} alt="" />{:else}<div class="avatar">{(r.name || "?").slice(0, 1).toUpperCase()}</div>{/if}
          <div class="post-who">
            <div class="title">{r.name || t("desk_unnamed")}{#if r.mine}<span class="meta">{" · "}{t("desk_you")}</span>{/if}</div>
            <div class="meta">{fmtTime(r.post.at)}{#if r.post.edited} · {t("desk_edited")}{/if}</div>
          </div>
          <div class="post-tools">
            <button class="linkish" title={t("desk_open_post")} onclick={() => openPost(r)}>{@html icons.sites}</button>
            {#if r.mine}<button class="linkish" title={t("desk_delete_post")} onclick={() => remove(r)}>{@html icons.close}</button>{/if}
          </div>
        </div>
        <div class="post-body">
          {#each r.blocks as b}
            {#if b.kind === "Paragraph"}
              <p>{#each b.spans as s}{#if s.link}<a href={s.link} onclick={(e) => { e.preventDefault(); if (s.link?.startsWith("ducat:")) window.dispatchEvent(new CustomEvent("ducat-link", { detail: s.link })); else openPost(r); }}>{s.text}</a>{:else if s.bold && s.italic}<strong><em>{s.text}</em></strong>{:else if s.bold}<strong>{s.text}</strong>{:else if s.italic}<em>{s.text}</em>{:else}{s.text}{/if}{/each}</p>
            {:else if b.kind === "Image"}
              {#if thumbs[r.persona + "/" + b.path]}<img class="post-img" src={thumbs[r.persona + "/" + b.path]} alt={b.alt} />{/if}
            {/if}
          {/each}
          {#if (r.post.media ?? []).length}
            <div class="post-media">
              {#each r.post.media ?? [] as m}
                {#if thumbs[r.persona + "/" + m.path]}
                  <button class="linkish img" title={t("desk_open_post")} onclick={() => openPost(r)}><img class="post-img" src={thumbs[r.persona + "/" + m.path]} alt={m.alt} /></button>
                {/if}
              {/each}
            </div>
          {/if}
          {#if (r.post.files ?? []).length}
            <div class="post-files">
              {#each r.post.files ?? [] as f}
                <span class="chip">{@html icons.files} {f.name} · {fmtBytes(f.bytes)} <button class="linkish" onclick={() => saveFile(f.addr, f.name)}>{t("desk_save_file")}</button></span>
              {/each}
            </div>
          {/if}
        </div>
      </div>
    {/each}
  </div>
  <div>
    <div class="card">
      <h3>{t("desk_your_home")}</h3>
      {#if home}
        <p class="note">{home.has_head ? tp("feed_home_posts", home.posts) : t("desk_no_home_yet_you")}</p>
        {#if home.has_head}<div class="actions"><button class="btn small" onclick={() => api.openSiteRoom(home!.record_key)}>{t("sites_open")}</button></div>{/if}
      {/if}
    </div>
    <div class="card">
      <h3>{t("desk_people_you_keep")}</h3>
      {#if loaded && kept.length === 0}<p class="empty">{t("desk_keep_someone")}</p>{/if}
      {#each kept as p (p.persona_hex)}
        <div class="row">
          <div class="title">{p.name}</div>
          <div class="actions"><button class="btn small" onclick={() => heart(p, false)}>{@html icons.heartFull} {t("desk_unheart")}</button></div>
        </div>
      {/each}
      {#if people.some((p) => !p.hearted && p.has_keys)}
        <div class="list-head"><span>{t("desk_others")}</span></div>
        <div class="chips">
          {#each people.filter((p) => !p.hearted && p.has_keys) as p (p.persona_hex)}
            <button class="chip" onclick={() => heart(p, true)}>{@html icons.heart} {p.name}</button>
          {/each}
        </div>
      {/if}
    </div>
  </div>
</div>
