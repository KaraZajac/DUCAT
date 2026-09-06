<script lang="ts">
  import { icons } from "./icons";
  // The market: what is offered around a place, and what you offer. A
  // notice on a board is a day long and a card wide; a desk with no GPS
  // takes the place as a geohash cell.
  import { onMount } from "svelte";
  import { t, i18n, LANGS } from "./i18n.svelte";
  import { api, copy, fmtXmr, fmtTime, fmtBytes, type FoundRow, type ListingAttachment, type ListingBundle, type ListingDraft, type ListingRow, type MarketRow, LISTING_DESCRIPTION_MAX, MARKET_CATEGORIES, confirmDanger } from "./api";
  import { gen, drive } from "./state.svelte";

  let mode = $state<"browse" | "mine">("browse");
  let err = $state<string | null>(null);
  let cell = $state("");
  let kind = $state<number | null>(null);
  let found = $state<FoundRow[]>([]);
  let searching = $state(false);
  let openFound = $state<FoundRow | null>(null);
  // The bundle behind the open notice (§16.18.3): fetched for the listing
  // somebody opened, one at a time, never while browsing.
  let bundle = $state<ListingBundle | null>(null);
  let bundleBusy = $state(false);
  let bundleErr = $state<string | null>(null);
  let bundleProgress = $state<{ done: number; total: number } | null>(null);
  let bundleSeq = 0;
  let lightbox = $state<number | null>(null);
  let saved = $state<string | null>(null);
  let asking = $state(false);

  // Where to look: the boards around a place, or the six shelves the
  // whole network shares (§16.18.2). No category is every shelf at once;
  // no language is the bare board, which every notice is also on.
  let scope = $state<"near" | "world">("near");
  let cat = $state<string | null>(null);
  let lang = $state<string | null>(null);
  let world = $state<MarketRow[]>([]);
  let worldLooked = $state(false);
  let worldBusy = $state(false);
  let worldSeq = 0;
  let subscribing = $state<string | null>(null);
  let worldMsg = $state<string | null>(null);
  const deskLang = $derived(LANGS.find((l) => l.code === i18n.lang) ?? LANGS[0]);

  function catLabel(slug: string | null): string {
    switch (slug) {
      case "news": return t("market_cat_news");
      case "serials": return t("market_cat_serials");
      case "sound": return t("market_cat_sound");
      case "software": return t("market_cat_software");
      case "art": return t("market_cat_art");
      case "other": return t("market_cat_other");
      default: return t("market_what_all");
    }
  }

  // What the shelf said last time paints now; the live read replaces it.
  // A newer choice while a read runs keeps its own answer.
  async function lookWorld() {
    err = null;
    const my = ++worldSeq;
    const [c, l] = [cat, lang];
    try { localStorage.setItem("ducat.market.cat", c ?? ""); localStorage.setItem("ducat.market.lang", l ?? ""); } catch {}
    worldBusy = true;
    try {
      const warm = await api.marketBrowseWorldCached(c, l);
      if (my === worldSeq && warm.length) { world = warm; worldLooked = true; }
    } catch {}
    try {
      const fresh = await api.marketBrowseWorld(c, l);
      if (my === worldSeq) world = fresh;
    } catch (e) {
      if (my === worldSeq) err = String(e);
    } finally {
      if (my === worldSeq) { worldBusy = false; worldLooked = true; }
    }
  }

  function pickScope(s: "near" | "world") {
    scope = s;
    try { localStorage.setItem("ducat.market.scope", s); } catch {}
    if (s === "world" && !worldLooked && !worldBusy) lookWorld();
  }
  function pickCat(c: string | null) { cat = c; lookWorld(); }
  function pickLang(l: string | null) { lang = l; lookWorld(); }

  // Claiming the card is subscribing (§16.18.2): the enrolment, the bill
  // or the first issue, all arrive down the thread it opens.
  async function subscribe(r: MarketRow) {
    err = null; worldMsg = null;
    subscribing = r.card;
    try {
      await api.claimCard(r.card, null);
      worldMsg = t("desk_subscribed_note");
    } catch (e) {
      err = String(e);
    } finally {
      subscribing = null;
    }
  }

  let mine = $state<ListingRow[]>([]);
  let editing = $state<ListingDraft | null>(null);
  let editingId = $state<string | null>(null);
  let busy = $state<string | null>(null);
  let photoUrls = $state<Record<string, string>>({});

  const kinds = $derived.by(() => { void i18n.lang; return [
    { id: 1, label: t("desk_kind_place") },
    { id: 2, label: t("desk_kind_vehicle") },
    { id: 4, label: t("board_chip_gear") },
    { id: 3, label: t("board_chip_sale") },
    { id: 5, label: t("desk_kind_skill") },
  ]; });

  async function refresh() {
    try {
      mine = await api.listings();
      for (const l of mine) {
        for (const p of l.photos) {
          if (!photoUrls[p]) photoUrls[p] = await api.pictureDataUrl(p);
        }
      }
    } catch (e) {
      err = String(e);
    }
  }

  onMount(() => {
    refresh();
    try {
      cell = localStorage.getItem("ducat.cell") ?? "";
      cat = localStorage.getItem("ducat.market.cat") || null;
      lang = localStorage.getItem("ducat.market.lang") || null;
      if (localStorage.getItem("ducat.market.scope") === "world") scope = "world";
    } catch {}
    if (cell) paint();
    if (scope === "world") lookWorld();
  });
  $effect(() => {
    void gen.value;
    refresh();
  });

  async function paint() {
    if (cell.trim().length < 4) return;
    try {
      found = await api.browseCached(cell, kind);
    } catch {}
  }

  async function search() {
    err = null;
    if (cell.trim().length < 4) { err = t("desk_area_geohash"); return; }
    try { localStorage.setItem("ducat.cell", cell.trim()); } catch {}
    searching = true;
    await paint();
    try {
      found = await api.browse(cell, kind);
    } catch (e) {
      err = String(e);
    } finally {
      searching = false;
    }
  }

  function openListing(f: FoundRow) {
    openFound = f;
    bundle = null; bundleErr = null; bundleProgress = null; saved = null; lightbox = null;
    if (f.gallery && f.gallery_dig) loadBundle(f);
  }

  function closeListing() {
    // A fetch still in flight lands nowhere.
    bundleSeq++;
    openFound = null; bundle = null; bundleBusy = false; bundleProgress = null; lightbox = null;
  }

  // Progress is polled while the swarm moves; a fetch that fails is the
  // seller being away, which the thumbnail on the board never is.
  async function loadBundle(f: FoundRow) {
    if (!f.gallery || !f.gallery_dig) return;
    const my = ++bundleSeq;
    const share = f.gallery;
    bundleBusy = true; bundleErr = null; bundleProgress = null;
    const tick = setInterval(async () => {
      try {
        const p = await api.fetchProgress(share);
        if (my === bundleSeq && p.pieces_total > 0) bundleProgress = { done: p.pieces_done, total: p.pieces_total };
      } catch {}
    }, 800);
    try {
      const b = await api.listingBundle(f.gallery, f.gallery_dig);
      if (my === bundleSeq) bundle = b;
    } catch (e) {
      if (my === bundleSeq) bundleErr = String(e);
    } finally {
      clearInterval(tick);
      if (my === bundleSeq) { bundleBusy = false; bundleProgress = null; }
    }
  }

  function pictureAt(path: string): string | null {
    return bundle?.pictures.find((p) => p.path === path)?.data_url ?? null;
  }

  // A link in the description goes to the desk's own flows or to a
  // picture in the bundle; nothing else was allowed past the check.
  function followLink(target: string) {
    if (target.startsWith("ducat:")) { window.dispatchEvent(new CustomEvent("ducat-link", { detail: target })); return; }
    const i = bundle?.pictures.findIndex((p) => p.path === target) ?? -1;
    if (i >= 0) lightbox = i;
  }

  function stepLightbox(by: number) {
    if (lightbox === null || !bundle?.pictures.length) return;
    lightbox = (lightbox + by + bundle.pictures.length) % bundle.pictures.length;
  }

  async function saveBundleFile(f: ListingAttachment) {
    err = null; saved = null;
    const dest = await api.pickSavePath(f.name);
    if (!dest) return;
    try {
      const n = await api.saveListingFile(f.path, dest);
      saved = t("desk_written_to", n, dest);
    } catch (e) {
      err = String(e);
    }
  }

  function specLabel(k: string): string {
    switch (k) {
      case "make": return t("rent_make");
      case "model": return t("rent_model");
      case "year": return t("rent_year");
      case "color": return t("rent_color");
      case "seats": return t("rent_seats");
      case "trim": return t("rent_trim");
      case "rooms": return t("rent_rooms");
      case "sleeps": return t("rent_sleeps");
      case "size_m2": return t("rent_size");
      default: return k;
    }
  }

  // The notice's own fields first; the bundle's table adds to them and
  // never replaces them (§16.18.3).
  const details = $derived.by((): [string, string][] => {
    void i18n.lang;
    const f = openFound;
    if (!f) return [];
    const rows: [string, string][] = [];
    for (const [k, v] of Object.entries(f.specs)) if (v !== null && v !== "" && k !== "features" && k !== "subtype") rows.push([specLabel(k), String(v)]);
    if (f.features?.length) rows.push([t("desk_features"), f.features.join(", ")]);
    for (const [k, v] of Object.entries(bundle?.doc?.specs ?? {})) rows.push([k, v]);
    return rows;
  });

  async function ask() {
    if (!openFound) return;
    err = null;
    asking = true;
    try {
      const r = await api.claimCard(openFound.card, null);
      await api.sendText(r.contact.persona_hex, `Hello — is "${openFound.title}" still available?`);
      err = null;
      openFound = null;
      alert(t("desk_asked_see_chat"));
    } catch (e) {
      err = String(e);
    } finally {
      asking = false;
    }
  }

  function newDraft(k: number) {
    editingId = null;
    editing = { id: null, kind: k, title: "", area: "", cell: cell || "", price_text: "", price_is_fiat: true, specs: {}, private_details: "", description: "", quantity: 1 };
  }

  function editListing(l: ListingRow) {
    editingId = l.id;
    editing = {
      id: l.id, kind: l.kind, title: l.title, area: l.area, cell: l.cell,
      price_text: l.price_typed ?? (l.price_pxmr / 1e12).toString(), price_is_fiat: !!l.price_typed,
      specs: { ...l.specs }, private_details: l.private_details, description: l.description, quantity: l.quantity,
    };
  }

  async function saveDraft() {
    if (!editing) return;
    err = null;
    busy = "save";
    try {
      const saved = await api.saveListing(editing);
      editingId = saved.id;
      editing = { ...editing, id: saved.id };
      await refresh();
    } catch (e) {
      err = String(e);
    } finally {
      busy = null;
    }
  }

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

  async function addPhoto() {
    if (!editingId) { err = t("desk_save_first"); return; }
    const p = await api.pickFile();
    if (!p) return;
    await act("photo", () => api.addListingPhoto(editingId!, p));
  }

  async function addFile() {
    if (!editingId) { err = t("desk_save_first"); return; }
    const p = await api.pickFile();
    if (!p) return;
    await act("file", () => api.addListingFile(editingId!, p));
  }
</script>

<div class="page-head">
  <h1 class="page-title">{t("desk_nav_market")}</h1>
  <div class="tabs" style="margin: 0">
    <button class="tab" class:active={mode === "browse"} onclick={() => (mode = "browse")}>{t("desk_browse")}</button>
    <button class="tab" class:active={mode === "mine"} onclick={() => (mode = "mine")}>{t("desk_my_listings")}</button>
  </div>
</div>

{#if mode === "browse"}
  <div class="tabs" style="margin: 0 0 10px">
    <button class="tab" class:active={scope === "near"} onclick={() => pickScope("near")}>{t("market_near_me")}</button>
    <button class="tab" class:active={scope === "world"} onclick={() => pickScope("world")}>{t("market_worldwide")}</button>
  </div>
  {#if scope === "near"}
  <div class="card">
    <div class="field">
      <label for="cell">{t("rent_area")}</label>
      <input id="cell" class="input narrow" placeholder={t("desk_geohash_hint")} bind:value={cell} onkeydown={(e) => e.key === "Enter" && search()} />
      <select class="input narrow" bind:value={kind}>
        <option value={null}>{t("market_what_all")}</option>
        {#each kinds as k}<option value={k.id}>{k.label}</option>{/each}
      </select>
      <button class="btn primary" disabled={searching} onclick={search}>{searching ? t("desk_looking") : t("desk_look")}</button>
    </div>
    <p class="note">{t("desk_browse_note")}</p>
    {#if err}<p class="err">{err}</p>{/if}
  </div>
  {#if openFound}
    <div class="card">
      <div class="page-head" style="margin-bottom: 8px"><h3 style="margin: 0">{openFound.title}</h3><button class="btn small" onclick={closeListing}>{t("main_back")}</button></div>
      <div class="found-detail">
        {#if openFound.thumb_data_url && !bundle?.pictures.length}<img class="thumb big" src={openFound.thumb_data_url} alt="" />{/if}
        <div class="grow">
          <div class="balance-big" style="font-size: 22px">{openFound.shown.primary}</div>
          <div class="meta">{openFound.kind_name} · {openFound.area}{openFound.cell ? ` · ${openFound.cell}` : ""} · {t("desk_until", fmtTime(openFound.expiry))}{openFound.quantity > 1 ? ` · ${t("rent_n_available", openFound.quantity)}` : ""}</div>
          {#if openFound.deposit_pxmr}<div class="meta">{t("desk_deposit_x", fmtXmr(openFound.deposit_pxmr))}</div>{/if}
          <div class="actions">
            {#if !openFound.mine}<button class="btn primary" disabled={asking} onclick={ask}>{asking ? t("desk_asking") : t("rent_ask_about_it")}</button>{:else}<span class="meta">{t("desk_this_is_yours")}</span>{/if}
            {#if openFound.gallery && !bundle && !bundleBusy}<button class="btn" onclick={() => loadBundle(openFound!)}>{bundleErr ? t("rent_search_retry") : t("desk_see_pictures")}</button>{/if}
          </div>
          {#if bundleBusy}<p class="meta">{t("desk_fetching_pictures")}{bundleProgress ? ` ${bundleProgress.done} / ${bundleProgress.total}` : ""}</p>{/if}
          {#if bundleErr}<p class="note">{t("desk_seller_away")}</p><p class="err">{bundleErr}</p>{/if}
          <p class="note">{t("desk_ask_note")}</p>
        </div>
      </div>
      {#if bundle?.pictures.length}
        <div class="pics">
          {#each bundle.pictures as p, i (p.path)}<button class="pic" title={p.caption} onclick={() => (lightbox = i)}><img src={p.data_url} alt={p.caption} /></button>{/each}
        </div>
      {/if}
      {#if bundle?.blocks.length}
        <div class="post-body listing-words">
          {#each bundle.blocks as b}
            {#if b.kind === "Paragraph"}
              <p>{#each b.spans as s}{#if s.link}<a href={s.link} onclick={(e) => { e.preventDefault(); followLink(s.link!); }}>{s.text}</a>{:else if s.bold && s.italic}<strong><em>{s.text}</em></strong>{:else if s.bold}<strong>{s.text}</strong>{:else if s.italic}<em>{s.text}</em>{:else}{s.text}{/if}{/each}</p>
            {:else if b.kind === "Image"}
              {@const src = pictureAt(b.path)}
              {#if src}<img class="post-img" {src} alt={b.alt} />{/if}
            {/if}
          {/each}
        </div>
      {/if}
      {#if details.length}
        <div class="list-head"><span>{t("desk_details")}</span></div>
        <table class="details"><tbody>{#each details as [k, v]}<tr><th>{k}</th><td>{v}</td></tr>{/each}</tbody></table>
      {/if}
      {#if bundle?.files.length}
        <div class="list-head"><span>{t("desk_attachments")}</span></div>
        <div class="post-files">
          {#each bundle.files as f (f.path)}<span class="chip">{@html icons.files} {f.name} · {fmtBytes(f.bytes)} <button class="linkish" onclick={() => saveBundleFile(f)}>{t("desk_save_as")}</button></span>{/each}
        </div>
        {#if saved}<p class="note ok-text">{saved}</p>{/if}
      {/if}
    </div>
    {#if lightbox !== null && bundle?.pictures[lightbox]}
      <button class="lightbox" title={t("chat_close")} onclick={() => (lightbox = null)} onkeydown={(e) => { if (e.key === "ArrowRight") { e.preventDefault(); stepLightbox(1); } else if (e.key === "ArrowLeft") { e.preventDefault(); stepLightbox(-1); } }}>
        <img src={bundle.pictures[lightbox].data_url} alt={bundle.pictures[lightbox].caption} />
        {#if bundle.pictures.length > 1}<span class="meta">{lightbox + 1} / {bundle.pictures.length}</span>{/if}
      </button>
    {/if}
  {:else}
    <div class="found-grid">
      {#each found as f (f.card)}
        <button class="found" onclick={() => openListing(f)}>
          {#if f.thumb_data_url}<img class="thumb" src={f.thumb_data_url} alt="" />{:else}<div class="thumb none">{f.kind_name}</div>{/if}
          <div class="found-text">
            <div class="title">{f.title}</div>
            <div class="meta">{f.shown.primary} · {f.kind_name}{f.mine ? ` · ${t("desk_yours")}` : ""}</div>
          </div>
        </button>
      {/each}
    </div>
    {#if found.length === 0 && !searching}<p class="empty">{t("desk_nothing_found")}</p>{/if}
  {/if}
  {:else}
  <div class="card">
    <div class="chips">
      <button class="chip" class:on={cat === null} onclick={() => pickCat(null)}>{t("market_what_all")}</button>
      {#each MARKET_CATEGORIES as c (c)}<button class="chip" class:on={cat === c} onclick={() => pickCat(c)}>{catLabel(c)}</button>{/each}
    </div>
    <div class="chips">
      <button class="chip" class:on={lang === null} onclick={() => pickLang(null)}>{t("desk_any_language")}</button>
      <button class="chip" class:on={lang === deskLang.code} onclick={() => pickLang(deskLang.code)}>{deskLang.name}</button>
      <select class="input narrow lang" value={lang && lang !== deskLang.code ? lang : ""} onchange={(e) => { const v = (e.target as HTMLSelectElement).value; if (v) pickLang(v); }}>
        <option value="">{t("desk_other_language")}</option>
        {#each LANGS.filter((l) => l.code !== deskLang.code) as l (l.code)}<option value={l.code}>{l.name}</option>{/each}
      </select>
      <button class="btn small" disabled={worldBusy} onclick={lookWorld}>{worldBusy ? t("desk_looking") : t("desk_refresh")}</button>
    </div>
    <p class="note">{t("desk_market_world_note")}</p>
    {#if worldMsg}<p class="note ok-text">{worldMsg}</p>{/if}
    {#if err}<p class="err">{err}</p>{/if}
  </div>
  {#if worldBusy}<p class="meta">{world.length ? t("market_refreshing") : t("market_looking", catLabel(cat))}</p>{/if}
  <div class="shelf">
    {#each world as r (r.board + ":" + r.subkey + ":" + r.card)}
      <div class="shelf-row">
        {#if r.cover_data_url}<img class="cover" src={r.cover_data_url} alt="" />{:else}<div class="cover none">{catLabel(r.category)}</div>{/if}
        <div class="shelf-text">
          <div class="title">{r.title}{#if r.mine}<span class="meta"> · {t("desk_yours")}</span>{/if}</div>
          {#if r.blurb}<div class="meta">{r.blurb}</div>{/if}
          <div class="meta">{r.shown ? t("market_per_period", r.shown.primary) : t("market_free")} · {catLabel(r.category)} · {t("desk_until", fmtTime(r.expiry))}</div>
        </div>
        <div class="actions nowrap">
          {#if !r.mine}<button class="btn small primary" disabled={subscribing === r.card} onclick={() => subscribe(r)}>{subscribing === r.card ? t("desk_subscribing") : t("market_subscribe")}</button>{/if}
        </div>
      </div>
    {/each}
  </div>
  {#if worldLooked && !worldBusy && world.length === 0}<p class="empty">{t("market_empty")}</p>{/if}
  {/if}

{:else}
  <div class="till-grid">
    <div class="card">
      <h3>{t("desk_your_listings")}</h3>
      {#each mine as l (l.id)}
        <button class="thread-row" class:active={editingId === l.id} onclick={() => editListing(l)}>
          {#if l.thumb_data_url}<img class="thumb small" src={l.thumb_data_url} alt="" />{/if}
          <div class="thread-text">
            <div class="thread-top"><span class="thread-name">{l.title || t("desk_untitled")}</span><span class="thread-when">{l.shown.primary}</span></div>
            <div class="thread-last">{l.kind_name} · {l.posted ? t("desk_on_board_since", fmtTime(l.posted_at)) : l.wanted ? t("rent_waiting_board") : t("rent_not_posted")}</div>
          </div>
        </button>
      {/each}
      <div class="chips">
        {#each kinds as k}<button class="chip" onclick={() => newDraft(k.id)}>+ {k.label}</button>{/each}
      </div>
    </div>
    <div class="card">
      {#if editing}
        <h3>{editingId ? t("desk_edit") : t("desk_new")} · {kinds.find((k) => k.id === editing!.kind)?.label}</h3>
        <div class="field"><label for="t">{t("desk_title")}</label><input id="t" class="input" bind:value={editing.title} /></div>
        <div class="field"><label for="ar">{t("rent_area")}</label><input id="ar" class="input" placeholder={t("rent_area_hint")} bind:value={editing.area} /></div>
        <div class="field"><label for="ce">{t("desk_cell")}</label><input id="ce" class="input narrow" placeholder={t("desk_geohash_hint")} bind:value={editing.cell} /></div>
        <div class="field">
          <label for="pr">{t("desk_price")}</label>
          <input id="pr" class="input narrow" bind:value={editing.price_text} />
          <select class="input narrow" bind:value={editing.price_is_fiat}><option value={true}>{t("desk_in_your_currency").toLowerCase()}</option><option value={false}>{t("desk_in_xmr")}</option></select>
        </div>
        {#if editing.kind !== 5}<div class="field"><label for="q">{t("rent_how_many")}</label><input id="q" class="input narrow" type="number" min="1" max="999" bind:value={editing.quantity} /></div>{/if}
        {#if editing.kind === 2}
          <div class="field"><span class="meta">{t("desk_kind_vehicle")}</span>
            <input class="input narrow" placeholder={t("rent_make")} value={String(editing.specs.make ?? "")} oninput={(e) => (editing!.specs.make = (e.target as HTMLInputElement).value)} />
            <input class="input narrow" placeholder={t("rent_model")} value={String(editing.specs.model ?? "")} oninput={(e) => (editing!.specs.model = (e.target as HTMLInputElement).value)} />
            <input class="input narrow" placeholder={t("rent_year")} value={String(editing.specs.year ?? "")} oninput={(e) => (editing!.specs.year = Number((e.target as HTMLInputElement).value) || undefined)} />
          </div>
        {:else if editing.kind === 1}
          <div class="field"><span class="meta">{t("desk_kind_place")}</span>
            <input class="input narrow" placeholder={t("rent_rooms")} value={String(editing.specs.rooms ?? "")} oninput={(e) => (editing!.specs.rooms = Number((e.target as HTMLInputElement).value) || undefined)} />
            <input class="input narrow" placeholder={t("rent_sleeps")} value={String(editing.specs.sleeps ?? "")} oninput={(e) => (editing!.specs.sleeps = Number((e.target as HTMLInputElement).value) || undefined)} />
            <input class="input narrow" placeholder={t("rent_size")} value={String(editing.specs.size_m2 ?? "")} oninput={(e) => (editing!.specs.size_m2 = Number((e.target as HTMLInputElement).value) || undefined)} />
          </div>
        {/if}
        <div class="field"><label for="ft">{t("desk_features")}</label><input id="ft" class="input" placeholder={t("rent_tags_hint")} value={((editing.specs.features as string[] | undefined) ?? []).join(", ")} oninput={(e) => (editing!.specs.features = (e.target as HTMLInputElement).value.split(",").map((s) => s.trim()).filter(Boolean))} /></div>
        <div class="field top">
          <label for="ds">{t("desk_description")}</label>
          <div class="grow">
            <textarea id="ds" class="input compose" rows="5" maxlength={LISTING_DESCRIPTION_MAX} placeholder={t("desk_description_hint")} bind:value={editing.description}></textarea>
            <div class="meta counter">{t("desk_chars_used", editing.description.length, LISTING_DESCRIPTION_MAX)}</div>
          </div>
        </div>
        <div class="field"><label for="pv">{t("desk_private")}</label><input id="pv" class="input" placeholder={t("rent_private_label")} bind:value={editing.private_details} /></div>
        <div class="actions">
          <button class="btn primary" disabled={busy === "save"} onclick={saveDraft}>{busy === "save" ? t("desk_saving") : t("myprofile_save")}</button>
          {#if editingId}
            <button class="btn" onclick={addPhoto}>{t("rent_photo_add")}…</button>
            <button class="btn" disabled={busy === "file"} onclick={addFile}>{t("desk_attach_file")}…</button>
            {#if drive.on}<input id="ppath" class="input narrow" hidden placeholder="/path/to/picture" onchange={(e) => act("photo", () => api.addListingPhoto(editingId!, (e.target as HTMLInputElement).value))} />{/if}
            {#if drive.on}<input id="fpath" class="input narrow" hidden placeholder="/path/to/file" onchange={(e) => act("file", () => api.addListingFile(editingId!, (e.target as HTMLInputElement).value))} />{/if}
            {#if mine.find((l) => l.id === editingId)?.posted}
              <button class="btn" disabled={busy === "post"} onclick={() => act("post", () => api.postListing(editingId!))}>{t("desk_refresh_board")}</button>
              <button class="btn danger" onclick={() => act("unpost", () => api.unpostListing(editingId!))}>{t("rent_take_down")}</button>
            {:else}
              <button class="btn" disabled={busy === "post"} onclick={() => act("post", () => api.postListing(editingId!))}>{busy === "post" ? t("desk_posting") : t("rent_post_it")}</button>
            {/if}
            <button class="btn danger" onclick={async () => { if (!(await confirmDanger(t("desk_confirm_delete_listing")))) return; act("rm", async () => { await api.removeListing(editingId!); editing = null; editingId = null; }); }}>{t("rent_delete")}</button>
          {/if}
        </div>
        {#if editingId}
          {@const l = mine.find((x) => x.id === editingId)}
          {#if l && l.photos.length}
            <div class="gallery">
              {#each l.photos as p, i}
                <div class="shot">
                  <img src={photoUrls[p] ?? ""} alt="" />
                  <div class="actions"><button class="linkish" onclick={() => act("cover", () => api.setListingCover(l.id, i))}>{t("desk_cover")}</button><button class="linkish" onclick={() => act("rmp", () => api.removeListingPhoto(l.id, i))}>{@html icons.close}</button></div>
                </div>
              {/each}
            </div>
            <p class="note">{t("desk_cover_note")}</p>
          {/if}
          {#if l && l.files.length}
            <div class="list-head"><span>{t("desk_attachments")}</span></div>
            <div class="post-files">
              {#each l.files as f, i (f.path)}<span class="chip">{@html icons.files} {f.name} · {fmtBytes(f.bytes)} <button class="linkish" title={t("rent_photo_remove")} onclick={() => act("rmf", () => api.removeListingFile(l.id, i))}>{@html icons.close}</button></span>{/each}
            </div>
          {/if}
          <p class="note">{t("desk_files_note")}</p>
        {/if}
        {#if err}<p class="err">{err}</p>{/if}
      {:else}
        <p class="empty">{t("desk_pick_listing")}</p>
        {#if err}<p class="err">{err}</p>{/if}
      {/if}
    </div>
  </div>
{/if}
