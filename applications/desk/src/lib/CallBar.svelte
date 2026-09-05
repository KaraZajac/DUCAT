<script lang="ts">
  // The call, wherever you are in the app: a bar across the top while one
  // is ringing, connecting, or up. Polled twice a second — a state, not
  // an event, so nothing can be missed.
  import { onMount } from "svelte";
  import { api, type CallView } from "./api";

  let view = $state<CallView | null>(null);
  let now = $state(Date.now());
  let err = $state<string | null>(null);

  onMount(() => {
    const t = setInterval(async () => {
      try { view = await api.callState(); } catch {}
      now = Date.now();
    }, 500);
    return () => clearInterval(t);
  });

  async function act(fn: () => Promise<unknown>) {
    err = null;
    try { await fn(); } catch (e) { err = String(e); }
  }

  function clock(since: number): string {
    const s = Math.max(0, Math.floor((now - since) / 1000));
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  }
</script>

{#if view && view.state.kind !== "Idle"}
  {@const st = view.state}
  <div class="callbar" class:active={st.kind === "Active"} class:ringing={st.kind === "Incoming"}>
    <div class="callbar-who">
      <span class="avatar">{(view.contact_name ?? "?").slice(0, 1).toUpperCase()}</span>
      <div>
        <div class="title">{view.contact_name ?? "Somebody"}</div>
        <div class="meta">
          {#if st.kind === "Outgoing"}Calling…
          {:else if st.kind === "Incoming"}Incoming call
          {:else if st.kind === "Answering"}Connecting…
          {:else if st.kind === "Active"}{view.rx_frames === 0 ? "Connecting…" : clock(st.since_ms)} · {view.rx_frames} in / {view.tx_frames} out
          {:else if st.kind === "NoAnswer"}{st.why === "Unreached" ? "Could not reach them" : st.why === "NeverConnected" ? "Answered, but no sound came through" : "No answer"}
          {/if}
        </div>
      </div>
    </div>
    <div class="actions nowrap">
      {#if st.kind === "Incoming"}
        <button class="btn primary" onclick={() => act(api.answerCall)}>Answer</button>
        <button class="btn danger" onclick={() => act(api.declineCall)}>Decline</button>
      {:else if st.kind === "NoAnswer"}
        <button class="btn" onclick={() => act(api.dismissCall)}>OK</button>
      {:else}
        <button class="btn danger" onclick={() => act(api.hangUp)}>{st.kind === "Outgoing" ? "Cancel" : "Hang up"}</button>
      {/if}
    </div>
    {#if err}<span class="err">{err}</span>{/if}
  </div>
{/if}
