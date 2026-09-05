<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Status } from "./lib/api";
  import Chat from "./lib/Chat.svelte";
  import Me from "./lib/Me.svelte";
  import Wallet from "./lib/Wallet.svelte";
  import Till from "./lib/Till.svelte";
  import Library from "./lib/Library.svelte";
  import Market from "./lib/Market.svelte";
  import Activity from "./lib/Activity.svelte";
  import Files from "./lib/Files.svelte";
  import Sites from "./lib/Sites.svelte";
  import StatusView from "./lib/Status.svelte";

  import { gen, startTicker } from "./lib/state.svelte";

  type Page = "chat" | "wallet" | "till" | "activity" | "library" | "market" | "files" | "sites" | "me" | "status";
  let page = $state<Page>("chat");
  let unread = $state(0);
  let status = $state<Status | null>(null);

  const nav: { id: Page; label: string; glyph: string }[] = [
    { id: "chat", label: "Chat", glyph: "✉" },
    { id: "wallet", label: "Wallet", glyph: "◈" },
    { id: "till", label: "Till", glyph: "▣" },
    { id: "activity", label: "Activity", glyph: "≣" },
    { id: "library", label: "Library", glyph: "▥" },
    { id: "market", label: "Market", glyph: "◫" },
    { id: "files", label: "Files", glyph: "▤" },
    { id: "sites", label: "Sites", glyph: "▦" },
    { id: "me", label: "Me", glyph: "◐" },
    { id: "status", label: "Status", glyph: "◉" },
  ];

  onMount(() => {
    const tick = async () => {
      try { status = await api.status(); } catch { status = null; }
    };
    tick();
    startTicker();
    const t = setInterval(tick, 3000);
    return () => clearInterval(t);
  });

  $effect(() => {
    void gen.value;
    api.unreadThreads().then((n) => (unread = n)).catch(() => {});
  });
</script>

<div class="shell">
  <aside class="sidebar">
    <div class="brand">DUCAT</div>
    {#each nav as n}
      <button class="nav-item" class:active={page === n.id} onclick={() => (page = n.id)}>
        <span class="glyph">{n.glyph}</span>{n.label}
        {#if n.id === "chat" && unread > 0}<span class="badge">{unread}</span>{/if}
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
    {#if page === "chat"}
      <Chat />
    {:else if page === "wallet"}
      <Wallet />
    {:else if page === "till"}
      <Till />
    {:else if page === "activity"}
      <Activity />
    {:else if page === "library"}
      <Library />
    {:else if page === "market"}
      <Market />
    {:else if page === "me"}
      <Me />
    {:else if page === "files"}
      <Files />
    {:else if page === "sites"}
      <Sites />
    {:else}
      <StatusView />
    {/if}
  </main>
</div>
