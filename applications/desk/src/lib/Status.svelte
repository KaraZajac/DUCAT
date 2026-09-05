<script lang="ts">
  import { onMount } from "svelte";
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

<h1 class="page-title">Status</h1>
<p class="page-lede">This desk's node on the network, and what it has been doing.</p>

<div class="card">
  {#if status}
    <div class="row">
      <div>
        <div class="title">
          <span class="dot" class:ok={status.ready} class:warn={status.attached && !status.ready}></span>
          {status.ready ? "Attached" : status.attached ? "Attaching" : status.running ? "Starting" : "Stopped"}
        </div>
        <div class="meta">{status.state} · {status.peers} peers, {status.reliable_peers} reliable</div>
      </div>
      <div class="actions"><span class="meta">{status.data_dir}</span></div>
    </div>
    {#if status.error}<p class="err">{status.error}</p>{/if}
  {:else}
    <p class="empty">Waiting for the node…</p>
  {/if}
</div>

<div class="card">
  <h3>Log</h3>
  <div class="log">{#each log as line}<div class={cls(line)}>{pretty(line)}</div>{/each}</div>
</div>
