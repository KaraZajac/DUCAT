<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Status } from "./lib/api";
  import Chat from "./lib/Chat.svelte";
  import Me from "./lib/Me.svelte";
  import Wallet from "./lib/Wallet.svelte";
  import Till from "./lib/Till.svelte";
  import Kiosk from "./lib/Kiosk.svelte";
  import Library from "./lib/Library.svelte";
  import Market from "./lib/Market.svelte";
  import Activity from "./lib/Activity.svelte";
  import CallBar from "./lib/CallBar.svelte";
  import Files from "./lib/Files.svelte";
  import Sites from "./lib/Sites.svelte";
  import StatusView from "./lib/Status.svelte";

  import { gen, startTicker } from "./lib/state.svelte";
  import { i18n, t } from "./lib/i18n.svelte";

  type Page = "chat" | "wallet" | "till" | "kiosk" | "activity" | "library" | "market" | "files" | "sites" | "me" | "status";
  let page = $state<Page>("chat");
  let unread = $state(0);
  let status = $state<Status | null>(null);

  // Labels re-read when the language changes; the keys are the phone's
  // where it has the word, the desk's own where it does not.
  const nav = $derived.by((): { id: Page; label: string; glyph: string }[] => {
    void i18n.lang;
    return [
      { id: "chat", label: t("tab_chat"), glyph: "✉" },
      { id: "wallet", label: t("monero_wallet_title"), glyph: "◈" },
      { id: "till", label: t("desk_nav_till"), glyph: "▣" },
      { id: "kiosk", label: t("kiosk_mode_title"), glyph: "▨" },
      { id: "activity", label: t("tab_activity"), glyph: "≣" },
      { id: "library", label: t("section_library"), glyph: "▥" },
      { id: "market", label: t("desk_nav_market"), glyph: "◫" },
      { id: "files", label: t("releases_title"), glyph: "▤" },
      { id: "sites", label: t("section_sites"), glyph: "▦" },
      { id: "me", label: t("desk_nav_me"), glyph: "◐" },
      { id: "status", label: t("section_status"), glyph: "◉" },
    ];
  });

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
        {status.ready ? `${t("net_line_attached")} · ${status.peers} ${t("net_line_peers")}` : status.attached ? t("desk_attaching") : t("net_starting")}
      {:else}
        {t("net_starting")}
      {/if}
    </div>
  </aside>
  <main class="main">
    <CallBar />
    {#if page === "chat"}
      <Chat />
    {:else if page === "wallet"}
      <Wallet />
    {:else if page === "till"}
      <Till />
    {:else if page === "kiosk"}
      <Kiosk />
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
