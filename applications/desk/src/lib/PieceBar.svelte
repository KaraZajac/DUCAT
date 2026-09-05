<script lang="ts">
  // The pieces, not a fraction: a bundle arrives scattered across the
  // swarm, and which parts have landed is what a reader can act on. A bar
  // creeping from the left would describe an order the fetcher does not
  // use. At most 40 cells; each cell fills with the share of its pieces.
  import type { Progress } from "./api";

  let { progress }: { progress: Progress | null } = $props();

  const CELLS = 40;

  let cells = $derived.by(() => {
    if (!progress || progress.pieces_total === 0) return [] as number[];
    const total = progress.pieces_total;
    const n = Math.min(CELLS, total);
    const out: number[] = [];
    for (let c = 0; c < n; c++) {
      const lo = Math.floor((c * total) / n);
      const hi = Math.floor(((c + 1) * total) / n);
      let on = 0;
      for (let i = lo; i < hi; i++) if (progress.pieces[i]) on++;
      out.push(hi > lo ? on / (hi - lo) : 0);
    }
    return out;
  });
</script>

{#if progress && progress.pieces_total > 0}
  <div class="pieces" title={`${progress.pieces_done} of ${progress.pieces_total} pieces`}>
    {#each cells as fill}
      <span class:on={fill >= 0.999} style={fill > 0 && fill < 0.999 ? `background: color-mix(in srgb, var(--primary) ${Math.round(fill * 100)}%, var(--surface-2))` : ""}></span>
    {/each}
  </div>
{:else if progress && !progress.done}
  <div class="pieces"><span style="background: var(--primary-soft)"></span></div>
{/if}
