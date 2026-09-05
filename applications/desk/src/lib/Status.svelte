<script lang="ts">

  const ATTACH: Record<string, string> = {
    Detached: "desk_node_detached",
    Detaching: "desk_node_detaching",
    Attaching: "desk_node_attaching",
    AttachedWeak: "desk_node_attached_weak",
    AttachedGood: "desk_node_attached",
    AttachedStrong: "desk_node_attached_strong",
    AttachedFull: "desk_node_attached_full",
    FullyAttached: "desk_node_attached_full",
    OverAttached: "desk_node_over_attached",
  };
  const attachWord = (s: string) => (ATTACH[s] ? t(ATTACH[s]) : s);
  import { onMount } from "svelte";
  import { t, tp, i18n } from "./i18n.svelte";
  import { api, type Status } from "./api";

  let status = $state<Status | null>(null);
  let log = $state<string[]>([]);

  async function refresh() {
    try {
      status = await api.status();
      log = await api.logTail(200);
    } catch (e) {
      status = null;
    }
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
      </div>
      <div class="actions"><span class="meta">{status.data_dir}</span></div>
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
