// One ticker for "did anything change": the app bumps a generation on
// every write, and screens re-read when it moves. Polled, not pushed — a
// counter cannot be missed and costs nothing to compare.
import { api } from "./api";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";

export const gen = $state({ value: 0 });
// Set by a debug build's drive thread once it is watching; screens that
// would open a file picker also take a typed path while it is on.
export const drive = $state({ on: false });

let timer: ReturnType<typeof setInterval> | null = null;

// Notices the app queued — a message, money, a call — shown the way the
// platform shows things. Asked for once; refused stays refused.
let notifyOk: boolean | null = null;
export const notices = $state({ latest: null as { title: string; body: string; open_thread: string | null } | null });

async function showNotices() {
  let got: { title: string; body: string; open_thread: string | null; at_ms: number }[] = [];
  try { got = await api.takeNotices(); } catch { return; }
  if (!got.length) return;
  notices.latest = got[got.length - 1];
  if (notifyOk === null) {
    try {
      notifyOk = (await isPermissionGranted()) || (await requestPermission()) === "granted";
    } catch { notifyOk = false; }
  }
  if (!notifyOk || document.hasFocus()) return;
  for (const n of got.slice(-3)) {
    try { sendNotification({ title: n.title, body: n.body }); } catch {}
  }
}

export function startTicker() {
  if (timer) return;
  const tick = async () => {
    if ((window as any).__DUCAT_DRIVE && !drive.on) drive.on = true;
    try {
      const g = await api.generation();
      if (g !== gen.value) {
        gen.value = g;
        await showNotices();
      }
    } catch {
      // The app is not open yet; the next tick will find it.
    }
  };
  tick();
  timer = setInterval(tick, 1000);
}
