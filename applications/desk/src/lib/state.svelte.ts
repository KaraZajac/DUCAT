// One ticker for "did anything change": the app bumps a generation on
// every write, and screens re-read when it moves. Polled, not pushed — a
// counter cannot be missed and costs nothing to compare.
import { api } from "./api";

export const gen = $state({ value: 0 });

let timer: ReturnType<typeof setInterval> | null = null;

export function startTicker() {
  if (timer) return;
  const tick = async () => {
    try {
      const g = await api.generation();
      if (g !== gen.value) gen.value = g;
    } catch {
      // The app is not open yet; the next tick will find it.
    }
  };
  tick();
  timer = setInterval(tick, 1000);
}
