<script lang="ts">

  // Literal keys, one per state, so the string checker can see each is used.
  function attachWord(s: string): string {
    switch (s) {
      case "Detached": return t("desk_node_detached");
      case "Detaching": return t("desk_node_detaching");
      case "Attaching": return t("desk_node_attaching");
      case "AttachedWeak": return t("desk_node_attached_weak");
      case "AttachedGood": return t("desk_node_attached");
      case "AttachedStrong": return t("desk_node_attached_strong");
      case "AttachedFull":
      case "FullyAttached": return t("desk_node_attached_full");
      case "OverAttached": return t("desk_node_over_attached");
      default: return s;
    }
  }
  import { onMount } from "svelte";
  import { t, tp, i18n } from "./i18n.svelte";
  import { api, type Status, type RoutingHealth } from "./api";

  let status = $state<Status | null>(null);
  let health = $state<RoutingHealth | null>(null);
  let log = $state<string[]>([]);
  let reconnecting = $state(false);
  let reconnectSaid = $state("");

  async function refresh() {
    try {
      status = await api.status();
      log = await api.logTail(200);
    } catch (e) {
      status = null;
    }
    // The table's figures come from the node's own debug text, which is
    // a few kilobytes to build; every third refresh is plenty.
    try {
      health = status?.running ? await api.routingHealth() : null;
    } catch (e) {
      health = null;
    }
  }

  // Stop and start, table purged: what a person does by hand when the
  // network has been slow for an hour, as one button.
  async function reconnect() {
    if (reconnecting) return;
    reconnecting = true;
    reconnectSaid = "";
    try {
      await api.reconnect();
    } catch (e) {
      reconnectSaid = String(e);
    }
    reconnecting = false;
    refresh();
  }

  onMount(() => {
    refresh();
    const t = setInterval(refresh, 3000);
    return () => clearInterval(t);
  });

  function cls(line: string): string {
    const lv = line.split("|")[1];
    return lv === "E" ? "e" : lv === "W" ? "w" : "";
  }

  function pretty(line: string): string {
    const parts = line.split("|");
    if (parts.length < 4) return line;
    const t = new Date(Number(parts[0])).toLocaleTimeString();
    return `${t} ${parts[1]} ${parts[2]}: ${parts.slice(3).join("|")}`;
  }
</script>

<h1 class="page-title">{t("section_status")}</h1>
<p class="page-lede">{t("desk_status_lede")}</p>

<div class="card">
  {#if status}
    <div class="row">
      <div>
        <div class="title">
          <span class="dot" class:ok={status.ready} class:warn={status.attached && !status.ready}></span>
          {status.ready ? t("net_line_attached") : status.attached ? t("desk_node_attaching") : status.running ? t("net_starting") : t("net_stopped")}
        </div>
        <div class="meta">{attachWord(status.state)} · {t("net_line_peers")}: {t("net_peers_value", status.peers, status.reliable_peers)}</div>
        {#if health && health.total > 0}
          <div class="meta">{t("net_line_table")}: {t("net_table_value", health.live, health.dead, health.total)}</div>
          {#if health.slow}
            <div class="meta hint">{t("net_slow_hint")}</div>
          {/if}
        {/if}
      </div>
      <div class="actions">
        <span class="meta">{status.data_dir}</span>
        {#if status.running}
          <button class="btn" onclick={reconnect} disabled={reconnecting}>{reconnecting ? t("net_reconnecting") : t("net_reconnect")}</button>
        {/if}
      </div>
      {#if reconnectSaid}<p class="err">{reconnectSaid}</p>{/if}
      {#if status.node_id}
        <div class="meta mono">{t("net_line_node_id")}: {status.node_id}</div>
      {/if}
      <div class="meta">{t("desk_version_line", status.version, status.platform)}</div>
    </div>
    {#if status.error}<p class="err">{status.error}</p>{/if}
  {:else}
    <p class="empty">{t("desk_waiting_node")}</p>
  {/if}
</div>

<div class="card">
  <h3>{t("desk_log")}</h3>
  <div class="meta" style="margin-bottom: 6px">{t("desk_newest_first")}</div>
  <div class="log">{#each [...log].reverse() as line}<div class={cls(line)}>{pretty(line)}</div>{/each}</div>
</div>

<style>
  .hint { max-width: 62ch; margin-top: 4px; }
</style>
