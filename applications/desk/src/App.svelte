<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Status } from "./lib/api";
  import Files from "./lib/Files.svelte";
  import Sites from "./lib/Sites.svelte";
  import StatusView from "./lib/Status.svelte";

  type Page = "files" | "sites" | "status";
  let page = $state<Page>("files");
  let status = $state<Status | null>(null);

  const nav: { id: Page; label: string; glyph: string }[] = [
    { id: "files", label: "Files", glyph: "▤" },
    { id: "sites", label: "Sites", glyph: "▦" },
    { id: "status", label: "Status", glyph: "◉" },
  ];

  onMount(() => {
    const tick = async () => {
      try { status = await api.status(); } catch { status = null; }
    };
    tick();
    const t = setInterval(tick, 3000);
    return () => clearInterval(t);
  });
</script>

<div class="shell">
  <aside class="sidebar">
    <div class="brand">DUCAT</div>
    {#each nav as n}
      <button class="nav-item" class:active={page === n.id} onclick={() => (page = n.id)}>
        <span class="glyph">{n.glyph}</span>{n.label}
      </button>
    {/each}
    <div class="spacer"></div>
    <div class="node-pill">
      <span class="dot" class:ok={status?.ready} class:warn={status?.attached && !status?.ready}></span>
      {#if status}
        {status.ready ? `Attached · ${status.peers} peers` : status.attached ? "Attaching…" : "Starting…"}
      {:else}
        Starting…
      {/if}
    </div>
  </aside>
  <main class="main">
    {#if page === "files"}
      <Files />
    {:else if page === "sites"}
      <Sites />
    {:else}
      <StatusView />
    {/if}
  </main>
</div>
