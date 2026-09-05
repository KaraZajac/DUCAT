// The desk's commands, typed once. Every screen imports from here and
// nowhere else, so a renamed command breaks one file.

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

export interface Status {
  running: boolean;
  attached: boolean;
  ready: boolean;
  peers: number;
  reliable_peers: number;
  state: string;
  error: string | null;
  data_dir: string;
}

export interface Progress {
  position: number;
  length: number;
  done: boolean;
  pieces_done: number;
  pieces_total: number;
  pieces: number[];
}

export interface ReleaseRow {
  share_key: string;
  digest_hex: string;
  title: string;
  added_at: number;
  bytes: number;
  keep_alive: boolean;
  mine: boolean;
  here: boolean;
  uri: string;
  dir: string;
}

export interface SiteRow {
  record_key: string;
  title: string;
  share: string;
  digest_hex: string;
  updated: number;
  added_at: number;
  keep_alive: boolean;
  mine: boolean;
  cached: boolean;
  current: boolean;
  uri: string;
  dir: string;
}

export const api = {
  status: () => invoke<Status>("status"),
  fetchProgress: (shareKey: string) => invoke<Progress>("fetch_progress", { shareKey }),

  releases: () => invoke<ReleaseRow[]>("releases"),
  shareFile: (path: string, title: string) => invoke<ReleaseRow>("share_file", { path, title }),
  addRelease: (uri: string, title = "") => invoke<ReleaseRow>("add_release", { uri, title }),
  fetchRelease: (digestHex: string) => invoke<string>("fetch_release", { digestHex }),
  setReleaseKeep: (digestHex: string, keep: boolean) => invoke<void>("set_release_keep", { digestHex, keep }),
  removeRelease: (digestHex: string) => invoke<void>("remove_release", { digestHex }),

  sites: () => invoke<SiteRow[]>("sites"),
  publishSite: (dir: string, title: string, recordKey?: string) =>
    invoke<SiteRow>("publish_site", { dir, title, recordKey: recordKey ?? null }),
  addSite: (uri: string) => invoke<SiteRow>("add_site", { uri }),
  fetchSite: (recordKey: string) => invoke<string>("fetch_site", { recordKey }),
  setSiteKeep: (recordKey: string, keep: boolean) => invoke<void>("set_site_keep", { recordKey, keep }),
  removeSite: (recordKey: string) => invoke<void>("remove_site", { recordKey }),
  lintSite: (dir: string) => invoke<string | null>("lint_site", { dir }),

  logTail: (lines = 200) => invoke<string[]>("log_tail", { lines }),

  pickFile: async (): Promise<string | null> => {
    const r = await open({ multiple: false, directory: false });
    return typeof r === "string" ? r : null;
  },
  pickFolder: async (): Promise<string | null> => {
    const r = await open({ multiple: false, directory: true });
    return typeof r === "string" ? r : null;
  },
  reveal: (path: string) => revealItemInDir(path),
};

export function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} kB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export function fmtWhen(secs: number): string {
  if (!secs) return "";
  return new Date(secs * 1000).toLocaleString();
}

export async function copy(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    // The webview may refuse without a user gesture in flight; the
    // address is selectable on screen either way.
  }
}
