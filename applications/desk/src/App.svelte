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
  import cat from "./assets/ducat-cat.png";
  import { icons } from "./lib/icons";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  type Page = "chat" | "wallet" | "till" | "kiosk" | "activity" | "library" | "market" | "files" | "sites" | "me" | "status";
  let page = $state<Page>("chat");
  let unread = $state(0);

  // The window's title carries what is waiting, so a minimized desk
  // still says so in the task bar.
  $effect(() => {
    const title = unread > 0 ? `(${unread}) DUCAT` : "DUCAT";
    document.title = title;
    getCurrentWindow().setTitle(title).catch(() => {});
  });

  // Keyboard: Ctrl+1…9,0 walks the sidebar in order; Ctrl+K goes to the
  // find box on the page that has one (Chat, otherwise).
  function onKey(e: KeyboardEvent) {
    if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
    if (e.key >= "0" && e.key <= "9") {
      const i = e.key === "0" ? 9 : Number(e.key) - 1;
      const n = nav[i];
      if (n) { e.preventDefault(); page = n.id; }
    } else if (e.key === "k" || e.key === "K") {
      e.preventDefault();
      const box = document.querySelector<HTMLInputElement>("input.find");
      if (box) { box.focus(); return; }
      // The chat's box appears once its list has loaded; look for it a
      // few times rather than once.
      page = "chat";
      let tries = 0;
      const look = () => {
        const b = document.querySelector<HTMLInputElement>("input.find");
        if (b) b.focus();
        else if (tries++ < 20) setTimeout(look, 100);
      };
      setTimeout(look, 100);
    }
  }
  let status = $state<Status | null>(null);

  // Labels re-read when the language changes; the keys are the phone's
  // where it has the word, the desk's own where it does not.
  const nav = $derived.by((): { id: Page; label: string }[] => {
    void i18n.lang;
    return [
      { id: "chat", label: t("tab_chat") },
      { id: "wallet", label: t("monero_wallet_title") },
      { id: "till", label: t("desk_nav_till") },
      { id: "kiosk", label: t("kiosk_mode_title") },
      { id: "activity", label: t("tab_activity") },
      { id: "library", label: t("section_library") },
      { id: "market", label: t("desk_nav_market") },
      { id: "files", label: t("releases_title") },
      { id: "sites", label: t("section_sites") },
      { id: "me", label: t("desk_nav_me") },
      { id: "status", label: t("section_status") },
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

<svelte:window onkeydown={onKey} />

<div class="shell">
  <aside class="sidebar">
    <div class="brand"><img src={cat} alt="" /><span>DUCAT</span></div>
    {#each nav as n}
      <button class="nav-item" class:active={page === n.id} title={n.label} onclick={() => (page = n.id)}>
        <span class="glyph">{@html icons[n.id]}</span><span class="label">{n.label}</span>
        {#if n.id === "chat" && unread > 0}<span class="badge">{unread}</span>{/if}
      </button>
    {/each}
    <div class="spacer"></div>
    <div class="node-pill">
      <span class="dot" class:ok={status?.ready} class:warn={status?.attached && !status?.ready}></span>
      <span class="word">
        {#if status}
          {status.ready ? `${t("net_line_attached")} · ${status.peers} ${t("net_line_peers")}` : status.attached ? t("desk_attaching") : t("net_starting")}
        {:else}
          {t("net_starting")}
        {/if}
      </span>
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
