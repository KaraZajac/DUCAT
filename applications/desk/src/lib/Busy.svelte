<script lang="ts">
  // What the app is doing while a button waits. The app layer says which
  // phase it is in ("stamping the notice"); while the call that started it
  // is in flight this asks every half second and shows the phrase beside
  // the button, in the reader's language, in the meta voice.
  import { api } from "./api";
  import { t } from "./i18n.svelte";

  let { on }: { on: boolean } = $props();
  let raw = $state<string | null>(null);

  // The phrases the app layer says, each with a key of its own; one it
  // learns to say before this table does is shown as it came.
  function worded(phrase: string): string {
    switch (phrase) {
      case "putting the pictures on the swarm": return t("desk_busy_pictures");
      case "issuing the card": return t("desk_busy_card");
      case "stamping the notice": return t("desk_busy_stamping");
      case "reading the board": return t("desk_busy_reading_board");
      case "writing to the board": return t("desk_busy_writing_board");
      case "forming the board": return t("desk_busy_forming_board");
      case "sending the roster": return t("desk_busy_roster");
      case "publishing your home": return t("desk_busy_home");
      case "seeding on the swarm": return t("desk_busy_seeding");
      case "publishing the address": return t("desk_busy_address");
    }
    const m = /^stamping for (.+)$/.exec(phrase);
    return m ? t("desk_busy_stamping_for", m[1]) : phrase;
  }

  $effect(() => {
    if (!on) return;
    let live = true;
    const tick = async () => {
      try {
        const n = await api.busyNote();
        if (live) raw = n;
      } catch {}
    };
    tick();
    const timer = setInterval(tick, 500);
    return () => {
      live = false;
      clearInterval(timer);
      raw = null;
    };
  });
</script>

{#if on && raw}<span class="busy-note">{worded(raw)}</span>{/if}
