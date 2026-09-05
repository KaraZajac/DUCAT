<script lang="ts">
  // The call, wherever you are in the app: a bar across the top while one
  // is ringing, connecting, or up. Polled twice a second — a state, not
  // an event, so nothing can be missed.
  import { onMount } from "svelte";
  import { t, tp, i18n } from "./i18n.svelte";
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
        <div class="title">{view.contact_name ?? t("desk_somebody")}</div>
        <div class="meta">
          {#if st.kind === "Outgoing"}{t("call_calling")}
          {:else if st.kind === "Incoming"}{t("call_incoming")}
          {:else if st.kind === "Answering"}{t("call_connecting")}
          {:else if st.kind === "Active"}{view.rx_frames === 0 ? t("call_connecting") : clock(st.since_ms)} · {t("call_frames", view.rx_frames, view.tx_frames)}
          {:else if st.kind === "NoAnswer"}{st.why === "Unreached" ? t("call_unreached") : st.why === "NeverConnected" ? t("call_never_connected") : t("call_no_answer")}
          {/if}
        </div>
      </div>
    </div>
    <div class="actions nowrap">
      {#if st.kind === "Incoming"}
        <button class="btn primary" onclick={() => act(api.answerCall)}>{t("call_answer_btn")}</button>
        <button class="btn danger" onclick={() => act(api.declineCall)}>{t("ceremony_decline")}</button>
      {:else if st.kind === "NoAnswer"}
        <button class="btn" onclick={() => act(api.dismissCall)}>{t("main_card_link_ok")}</button>
      {:else}
        <button class="btn danger" onclick={() => act(api.hangUp)}>{st.kind === "Outgoing" ? t("common_cancel") : t("call_end_btn")}</button>
      {/if}
    </div>
    {#if err}<span class="err">{err}</span>{/if}
  </div>
{/if}
