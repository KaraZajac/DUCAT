<script lang="ts">
  // The market: what is offered around a place, and what you offer. A
  // notice on a board is a day long and a card wide; a desk with no GPS
  // takes the place as a geohash cell.
  import { onMount } from "svelte";
  import { api, copy, fmtXmr, fmtTime, type FoundRow, type ListingDraft, type ListingRow } from "./api";
  import { gen, drive } from "./state.svelte";

  let mode = $state<"browse" | "mine">("browse");
  let err = $state<string | null>(null);
  let cell = $state("");
  let kind = $state<number | null>(null);
  let found = $state<FoundRow[]>([]);
  let searching = $state(false);
  let openFound = $state<FoundRow | null>(null);
  let gallery = $state<string[]>([]);
  let galleryBusy = $state(false);
  let asking = $state(false);

  let mine = $state<ListingRow[]>([]);
  let editing = $state<ListingDraft | null>(null);
  let editingId = $state<string | null>(null);
  let busy = $state<string | null>(null);
  let photoUrls = $state<Record<string, string>>({});

  const kinds = [
    { id: 1, label: "Place" },
    { id: 2, label: "Vehicle" },
    { id: 4, label: "Gear" },
    { id: 3, label: "For sale" },
    { id: 5, label: "Skill" },
  ];

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
    try { cell = localStorage.getItem("ducat.cell") ?? ""; } catch {}
    if (cell) paint();
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
    if (cell.trim().length < 4) { err = "The area is a geohash cell — five characters, like dqche."; return; }
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

  async function openListing(f: FoundRow) {
    openFound = f;
    gallery = [];
  }

  async function loadGallery() {
    if (!openFound?.gallery || !openFound.gallery_dig) return;
    galleryBusy = true;
    try {
      const files = await api.fetchGallery(openFound.gallery, openFound.gallery_dig);
      gallery = await Promise.all(files.map((p) => api.pictureDataUrl(p)));
    } catch (e) {
      err = String(e);
    } finally {
      galleryBusy = false;
    }
  }

  async function ask() {
    if (!openFound) return;
    err = null;
    asking = true;
    try {
      const r = await api.claimCard(openFound.card, null);
      await api.sendText(r.contact.persona_hex, `Hello — is "${openFound.title}" still available?`);
      err = null;
      openFound = null;
      alert("Asked. The conversation is in Chat.");
    } catch (e) {
      err = String(e);
    } finally {
      asking = false;
    }
  }

  function newDraft(k: number) {
    editingId = null;
    editing = { id: null, kind: k, title: "", area: "", cell: cell || "", price_text: "", price_is_fiat: true, specs: {}, private_details: "", quantity: 1 };
  }

  function editListing(l: ListingRow) {
    editingId = l.id;
    editing = {
      id: l.id, kind: l.kind, title: l.title, area: l.area, cell: l.cell,
      price_text: l.price_typed ?? (l.price_pxmr / 1e12).toString(), price_is_fiat: !!l.price_typed,
      specs: { ...l.specs }, private_details: l.private_details, quantity: l.quantity,
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
    if (!editingId) { err = "Save the listing first, then add pictures."; return; }
    const p = await api.pickFile();
    if (!p) return;
    await act("photo", () => api.addListingPhoto(editingId!, p));
  }

  function specText(f: { specs: Record<string, unknown>; features: string[] }): string {
    const parts: string[] = [];
    for (const [k, v] of Object.entries(f.specs)) if (v !== null && v !== "" && k !== "features" && k !== "subtype") parts.push(`${k}: ${v}`);
    if (f.features?.length) parts.push(f.features.join(", "));
    return parts.join(" · ");
  }
</script>

<div class="page-head">
  <h1 class="page-title">Market</h1>
  <div class="tabs" style="margin: 0">
    <button class="tab" class:active={mode === "browse"} onclick={() => (mode = "browse")}>Browse</button>
    <button class="tab" class:active={mode === "mine"} onclick={() => (mode = "mine")}>My listings</button>
  </div>
</div>

{#if mode === "browse"}
  <div class="card">
    <div class="field">
      <label for="cell">Area</label>
      <input id="cell" class="input narrow" placeholder="geohash, e.g. dqche" bind:value={cell} onkeydown={(e) => e.key === "Enter" && search()} />
      <select class="input narrow" bind:value={kind}>
        <option value={null}>Everything</option>
        {#each kinds as k}<option value={k.id}>{k.label}</option>{/each}
      </select>
      <button class="btn primary" disabled={searching} onclick={search}>{searching ? "Looking…" : "Look"}</button>
    </div>
    <p class="note">The home cell and its eight neighbours are read — about 5 km across at five characters. What was seen before paints first.</p>
    {#if err}<p class="err">{err}</p>{/if}
  </div>
  {#if openFound}
    <div class="card">
      <div class="page-head" style="margin-bottom: 8px"><h3 style="margin: 0">{openFound.title}</h3><button class="btn small" onclick={() => (openFound = null)}>Back</button></div>
      <div class="found-detail">
        {#if openFound.thumb_data_url}<img class="thumb big" src={openFound.thumb_data_url} alt="" />{/if}
        <div>
          <div class="balance-big" style="font-size: 22px">{openFound.shown.primary}</div>
          <div class="meta">{openFound.kind_name} · {openFound.area}{openFound.cell ? ` · ${openFound.cell}` : ""} · until {fmtTime(openFound.expiry)}{openFound.quantity > 1 ? ` · ${openFound.quantity} available` : ""}</div>
          {#if openFound.deposit_pxmr}<div class="meta">deposit {fmtXmr(openFound.deposit_pxmr)}</div>{/if}
          <p>{specText(openFound)}</p>
          <div class="actions">
            {#if !openFound.mine}<button class="btn primary" disabled={asking} onclick={ask}>{asking ? "Asking…" : "Ask about it"}</button>{:else}<span class="meta">This is yours.</span>{/if}
            {#if openFound.gallery && !gallery.length}<button class="btn" disabled={galleryBusy} onclick={loadGallery}>{galleryBusy ? "Fetching pictures…" : "See the pictures"}</button>{/if}
          </div>
          <p class="note">Asking answers the seller's card and opens a conversation; fetching the pictures tells the seller somebody looked.</p>
        </div>
      </div>
      {#if gallery.length}<div class="gallery">{#each gallery as g}<img src={g} alt="" />{/each}</div>{/if}
    </div>
  {:else}
    <div class="found-grid">
      {#each found as f (f.card)}
        <button class="found" onclick={() => openListing(f)}>
          {#if f.thumb_data_url}<img class="thumb" src={f.thumb_data_url} alt="" />{:else}<div class="thumb none">{f.kind_name}</div>{/if}
          <div class="found-text">
            <div class="title">{f.title}</div>
            <div class="meta">{f.shown.primary} · {f.kind_name}{f.mine ? " · yours" : ""}</div>
          </div>
        </button>
      {/each}
    </div>
    {#if found.length === 0 && !searching}<p class="empty">Nothing found here yet — or nothing looked for. Type an area and press Look.</p>{/if}
  {/if}

{:else}
  <div class="till-grid">
    <div class="card">
      <h3>Your listings</h3>
      {#each mine as l (l.id)}
        <button class="thread-row" class:active={editingId === l.id} onclick={() => editListing(l)}>
          {#if l.thumb_data_url}<img class="thumb small" src={l.thumb_data_url} alt="" />{/if}
          <div class="thread-text">
            <div class="thread-top"><span class="thread-name">{l.title || "(untitled)"}</span><span class="thread-when">{l.shown.primary}</span></div>
            <div class="thread-last">{l.kind_name} · {l.posted ? `on the board since ${fmtTime(l.posted_at)}` : l.wanted ? "waiting for a slot" : "not posted"}</div>
          </div>
        </button>
      {/each}
      <div class="chips">
        {#each kinds as k}<button class="chip" onclick={() => newDraft(k.id)}>+ {k.label}</button>{/each}
      </div>
    </div>
    <div class="card">
      {#if editing}
        <h3>{editingId ? "Edit" : "New"} {kinds.find((k) => k.id === editing!.kind)?.label.toLowerCase()}</h3>
        <div class="field"><label for="t">Title</label><input id="t" class="input" bind:value={editing.title} /></div>
        <div class="field"><label for="ar">Area</label><input id="ar" class="input" placeholder="What you would tell a friend" bind:value={editing.area} /></div>
        <div class="field"><label for="ce">Cell</label><input id="ce" class="input narrow" placeholder="geohash, e.g. dqche" bind:value={editing.cell} /></div>
        <div class="field">
          <label for="pr">Price</label>
          <input id="pr" class="input narrow" bind:value={editing.price_text} />
          <select class="input narrow" bind:value={editing.price_is_fiat}><option value={true}>in your currency</option><option value={false}>in XMR</option></select>
        </div>
        {#if editing.kind !== 5}<div class="field"><label for="q">How many</label><input id="q" class="input narrow" type="number" min="1" max="999" bind:value={editing.quantity} /></div>{/if}
        {#if editing.kind === 2}
          <div class="field"><span class="meta">Vehicle</span>
            <input class="input narrow" placeholder="Make" value={String(editing.specs.make ?? "")} oninput={(e) => (editing!.specs.make = (e.target as HTMLInputElement).value)} />
            <input class="input narrow" placeholder="Model" value={String(editing.specs.model ?? "")} oninput={(e) => (editing!.specs.model = (e.target as HTMLInputElement).value)} />
            <input class="input narrow" placeholder="Year" value={String(editing.specs.year ?? "")} oninput={(e) => (editing!.specs.year = Number((e.target as HTMLInputElement).value) || undefined)} />
          </div>
        {:else if editing.kind === 1}
          <div class="field"><span class="meta">Place</span>
            <input class="input narrow" placeholder="Rooms" value={String(editing.specs.rooms ?? "")} oninput={(e) => (editing!.specs.rooms = Number((e.target as HTMLInputElement).value) || undefined)} />
            <input class="input narrow" placeholder="Sleeps" value={String(editing.specs.sleeps ?? "")} oninput={(e) => (editing!.specs.sleeps = Number((e.target as HTMLInputElement).value) || undefined)} />
            <input class="input narrow" placeholder="m²" value={String(editing.specs.size_m2 ?? "")} oninput={(e) => (editing!.specs.size_m2 = Number((e.target as HTMLInputElement).value) || undefined)} />
          </div>
        {/if}
        <div class="field"><label for="ft">Features</label><input id="ft" class="input" placeholder="comma separated" value={((editing.specs.features as string[] | undefined) ?? []).join(", ")} oninput={(e) => (editing!.specs.features = (e.target as HTMLInputElement).value.split(",").map((s) => s.trim()).filter(Boolean))} /></div>
        <div class="field"><label for="pv">Private</label><input id="pv" class="input" placeholder="Only you see this (e.g. where the keys are)" bind:value={editing.private_details} /></div>
        <div class="actions">
          <button class="btn primary" disabled={busy === "save"} onclick={saveDraft}>{busy === "save" ? "Saving…" : "Save"}</button>
          {#if editingId}
            <button class="btn" onclick={addPhoto}>Add a picture…</button>
            {#if drive.on}<input id="ppath" class="input narrow" placeholder="/path/to/picture" onchange={(e) => act("photo", () => api.addListingPhoto(editingId!, (e.target as HTMLInputElement).value))} />{/if}
            {#if mine.find((l) => l.id === editingId)?.posted}
              <button class="btn" disabled={busy === "post"} onclick={() => act("post", () => api.postListing(editingId!))}>Refresh on the board</button>
              <button class="btn danger" onclick={() => act("unpost", () => api.unpostListing(editingId!))}>Take down</button>
            {:else}
              <button class="btn" disabled={busy === "post"} onclick={() => act("post", () => api.postListing(editingId!))}>{busy === "post" ? "Posting…" : "Post to the board"}</button>
            {/if}
            <button class="btn danger" onclick={() => act("rm", async () => { await api.removeListing(editingId!); editing = null; editingId = null; })}>Delete</button>
          {/if}
        </div>
        {#if editingId}
          {@const l = mine.find((x) => x.id === editingId)}
          {#if l && l.photos.length}
            <div class="gallery">
              {#each l.photos as p, i}
                <div class="shot">
                  <img src={photoUrls[p] ?? ""} alt="" />
                  <div class="actions"><button class="linkish" onclick={() => act("cover", () => api.setListingCover(l.id, i))}>cover</button><button class="linkish" onclick={() => act("rmp", () => api.removeListingPhoto(l.id, i))}>✕</button></div>
                </div>
              {/each}
            </div>
            <p class="note">The cover becomes the ten-kilobyte picture on the board; the rest travel on the swarm and are fetched only when somebody opens the listing.</p>
          {/if}
        {/if}
        {#if err}<p class="err">{err}</p>{/if}
      {:else}
        <p class="empty">Pick a listing, or start one with the buttons on the left.</p>
        {#if err}<p class="err">{err}</p>{/if}
      {/if}
    </div>
  </div>
{/if}
