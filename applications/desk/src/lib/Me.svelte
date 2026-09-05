<script lang="ts">
  // Me: the name on your cards, the code somebody scans to reach you, and
  // the hats you wear.
  import { onMount } from "svelte";
  import { api, copy, type Code, type PersonaRow } from "./api";
  import { gen } from "./state.svelte";

  let personas = $state<PersonaRow[]>([]);
  let name = $state("");
  let savedName = $state<string | null>(null);
  let code = $state<Code | null>(null);
  let cutting = $state(false);
  let err = $state<string | null>(null);
  let newPersona = $state("");

  const worn = $derived(personas.find((p) => p.worn) ?? null);

  async function refresh() {
    try {
      personas = await api.personas();
      const w = personas.find((p) => p.worn);
      savedName = w?.my_name ?? null;
      if (!name) name = savedName ?? "";
    } catch (e) {
      err = String(e);
    }
  }

  onMount(refresh);
  $effect(() => {
    void gen.value;
    refresh();
  });

  async function saveName() {
    err = null;
    try {
      await api.setMyName(name.trim());
      await refresh();
      // The code carries the name; a new name means a new code next time.
      code = null;
    } catch (e) {
      err = String(e);
    }
  }

  async function showCode() {
    err = null;
    cutting = true;
    try {
      code = await api.profileCode();
    } catch (e) {
      err = String(e);
    } finally {
      cutting = false;
    }
  }

  async function wear(hex: string) {
    await api.wear(hex);
    code = null;
    name = "";
    await refresh();
  }

  async function addPersona() {
    const n = newPersona.trim();
    if (!n) return;
    const p = await api.createPersona(n, 0);
    if (!p) err = "Four is the most — compartments only work when they fit on one hand.";
    newPersona = "";
    await refresh();
  }
</script>

<h1 class="page-title">Me</h1>
<p class="page-lede">What your cards say about you, and the code somebody answers to reach you.</p>

<div class="card">
  <h3>Your name</h3>
  <div class="field">
    <label for="myname">On your cards</label>
    <input id="myname" class="input" bind:value={name} placeholder="How you want to be known" onkeydown={(e) => e.key === "Enter" && saveName()} />
    <button class="btn" onclick={saveName} disabled={name.trim() === (savedName ?? "")}>Save</button>
  </div>
  <p class="note">Sent in the open on the card and inside the handshake, nowhere else.</p>
</div>

<div class="card">
  <h3>Your code</h3>
  {#if code}
    <div class="code-wrap">
      <div class="qr">{@html code.svg}</div>
      <div class="code-side">
        <p class="note">Somebody who scans this — or pastes it into their desk's Chat page — becomes a contact. It answers once; a fresh one is cut when it is taken.</p>
        <div class="addr">{code.uri}</div>
        <div class="actions">
          <button class="btn small" onclick={() => copy(code?.uri ?? "")}>Copy code</button>
          <button class="btn small" onclick={showCode}>Refresh</button>
        </div>
      </div>
    </div>
  {:else}
    <button class="btn primary" onclick={showCode} disabled={cutting}>{cutting ? "Cutting…" : "Show my code"}</button>
    <p class="note">Cutting a code writes a record to the network; the first one takes a few seconds.</p>
  {/if}
</div>

<div class="card">
  <h3>Personas</h3>
  <p class="note">Each is a separate identity with its own contacts. A thread stays with the persona it was born under.</p>
  {#each personas as p (p.hex)}
    <div class="row">
      <div class="lead">
        <div class="title">{p.name || (p.primary ? "Primary" : "Unnamed")} {#if p.worn}<span class="meta">· wearing</span>{/if}</div>
        <div class="meta">{p.my_name ? `cards say “${p.my_name}”` : "no name on its cards yet"} · <span class="mono">{p.hex.slice(0, 16)}…</span></div>
      </div>
      <div class="actions">
        {#if !p.worn}<button class="btn small" onclick={() => wear(p.hex)}>Wear</button>{/if}
      </div>
    </div>
  {/each}
  {#if personas.length < 4}
    <div class="field">
      <label for="newp">New persona</label>
      <input id="newp" class="input" bind:value={newPersona} placeholder="A label for it, e.g. Shop" onkeydown={(e) => e.key === "Enter" && addPersona()} />
      <button class="btn" onclick={addPersona} disabled={!newPersona.trim()}>Create</button>
    </div>
  {/if}
  {#if err}<p class="err">{err}</p>{/if}
</div>
