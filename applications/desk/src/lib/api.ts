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

export interface PersonaRow {
  hex: string;
  name: string;
  color: number;
  primary: boolean;
  worn: boolean;
  my_name: string | null;
}

export interface Code {
  uri: string;
  inbox_key: string;
  svg: string;
}

export interface ContactRow {
  persona_hex: string;
  name: string;
  named: boolean;
  petname: string | null;
  asserted_name: string | null;
  unread: boolean;
  last_body: string | null;
  last_at: number;
  last_outgoing: boolean;
  chat_visible: boolean;
  has_keys: boolean;
  owner: string;
  their_address: string | null;
  pending_address: string | null;
  card_purpose: string | null;
  email: string | null;
  phone: string | null;
  signal: string | null;
}

export interface MessageRow {
  outgoing: boolean;
  seq: number;
  body: string;
  timestamp: number;
  kind: number;
  amount_pxmr: number;
  delivered: boolean;
  forward_secret: boolean;
  dead_letter: boolean;
  read_by_them: boolean | null;
  att_name: string | null;
  att_mime: string | null;
  att_len: number;
  items: [string, number][];
  tax_pxmr: number | null;
  payto: string | null;
  txid_hex: string | null;
  re_seq: number | null;
  re_own: boolean;
  oob: boolean;
  group_id: string | null;
  pub_wanted: string | null;
  pub_period_id: string | null;
}

export interface Balances {
  spendable_pxmr: number;
  locked_pxmr: number;
  spendable_outputs: number;
  blocks_to_unlock: number;
  scanned_to: number;
  tip: number;
  scan_rate: number;
  scan_from: number;
  error: string | null;
  syncing: boolean;
  blocks_left: number;
  progress: number;
  seconds_left: number | null;
}

export interface FiatView {
  text: string;
  notional: boolean;
  stale: boolean;
}

export interface WalletView {
  address: string | null;
  balances: Balances;
  blocker: "None" | "NoWallet" | "NoNode" | "Failing";
  node: string | null;
  own_node: string | null;
  stagenet: boolean;
  fiat_spendable: FiatView | null;
  currency: string;
  address_svg: string;
}

export interface NoteRow {
  amount_pxmr: number;
  height: number;
  spent: boolean;
  tx_hash_hex: string;
  timestamp: number;
  minor: number;
  unlocked: boolean;
  from: string | null;
}

export interface SentRow {
  txid_hex: string;
  amount_pxmr: number;
  fee_pxmr: number;
  to_address: string;
  contact: string | null;
  contact_name: string | null;
  note: string | null;
  timestamp: number;
  donation: boolean;
  recovered: boolean;
}

export interface Quote {
  amount_pxmr: number;
  fee_pxmr: number;
  notes: number;
  minutes_to_confirm: number;
  total_pxmr: number;
  remaining_pxmr: number;
  affordable: boolean;
  fee_known: boolean;
}

export interface Shown {
  primary: string;
  secondary: string | null;
  notional: boolean;
  stale: boolean;
}

export interface TabRow {
  id: string;
  origin: string;
  persona_hex: string;
  name: string;
  opened_at: number;
  lines: [string, number][];
  tax_pxmr: number | null;
  state: string;
  total_pxmr: number;
  settled_total: number;
  settled_at: number;
  paid_pxmr: number;
  tip_pxmr: number;
  seen_tx: string | null;
  receipt_owed: boolean;
  shown: Shown;
}

export interface ItemRow {
  id: string;
  name: string;
  price: string;
  currency: string;
  category: string;
  sold_out: boolean;
  pxmr: number | null;
  snag: "NoRate" | "WrongCurrency" | "Unpriceable" | null;
}

export interface IssueRow {
  period: string;
  on_shelf: boolean;
  on_swarm: boolean;
  bytes: number;
  sent: string[];
  billed: string[];
  file: string;
}

export interface PublicationRow {
  id: string;
  title: string;
  price_pxmr: number;
  subscribers: ContactRow[];
  issues: IssueRow[];
  has_shelf: boolean;
  press_code: string | null;
  created: number;
}

export interface ShelfRow {
  period: string;
  has_key: boolean;
  on_shelf: boolean;
  shelf_bytes: number;
  on_swarm: boolean;
  fetched_bytes: number | null;
  asked: boolean;
  dir: string;
}

export interface SubscriptionRow {
  publisher_hex: string;
  name: string;
  price_known: boolean;
  mirror: boolean;
  muted: boolean;
  has_shelf: boolean;
  shelf_seen_at: number;
  periods: ShelfRow[];
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

  personas: () => invoke<PersonaRow[]>("personas"),
  wear: (hex: string) => invoke<void>("wear", { hex }),
  createPersona: (name: string, color: number) => invoke<PersonaRow | null>("create_persona", { name, color }),
  setMyName: (name: string, personaHex?: string) => invoke<void>("set_my_name", { name, personaHex: personaHex ?? null }),
  profileCode: () => invoke<Code>("profile_code"),

  contacts: () => invoke<ContactRow[]>("contacts"),
  claimCard: (uri: string, petname: string | null) => invoke<{ contact: ContactRow; known: boolean }>("claim_card", { uri, petname }),
  thread: (personaHex: string) => invoke<MessageRow[]>("thread", { personaHex }),
  sendText: (personaHex: string, body: string) => invoke<void>("send_text", { personaHex, body }),
  markSeen: (personaHex: string) => invoke<void>("mark_seen", { personaHex }),
  setPetname: (personaHex: string, name: string | null) => invoke<void>("set_petname", { personaHex, name }),
  removeContact: (personaHex: string) => invoke<void>("remove_contact", { personaHex }),
  unreadThreads: () => invoke<number>("unread_threads"),
  generation: () => invoke<number>("generation"),
  pollNow: (personaHex?: string) => invoke<number>("poll_now", { personaHex: personaHex ?? null }),

  walletStatus: () => invoke<WalletView>("wallet_status"),
  walletNotes: () => invoke<NoteRow[]>("wallet_notes"),
  walletSends: () => invoke<SentRow[]>("wallet_sends"),
  walletQuote: (amountXmr: string, priority = 1) => invoke<Quote>("wallet_quote", { amountXmr, priority }),
  walletSend: (to: string, amountXmr: string, note: string | null, priority = 1, contactHex: string | null = null) =>
    invoke<string>("wallet_send", { to, amountXmr, note, priority, contactHex }),
  walletMax: (priority = 1) => invoke<number>("wallet_max", { priority }),
  setOwnNode: (url: string | null) => invoke<void>("set_own_node", { url }),
  walletRescan: (height: number) => invoke<void>("wallet_rescan", { height }),
  walletStep: () => invoke<boolean>("wallet_step"),

  tabs: () => invoke<TabRow[]>("tabs"),
  openTab: (personaHex: string, origin: string) => invoke<TabRow>("open_tab", { personaHex, origin }),
  tabAddLine: (id: string, description: string, amountPxmr: number) => invoke<TabRow>("tab_add_line", { id, description, amountPxmr }),
  tabRemoveLine: (id: string, index: number) => invoke<TabRow>("tab_remove_line", { id, index }),
  tabSetTax: (id: string, taxPxmr: number | null) => invoke<TabRow>("tab_set_tax", { id, taxPxmr }),
  settleTab: (id: string) => invoke<TabRow>("settle_tab", { id }),
  cancelTab: (id: string) => invoke<TabRow | null>("cancel_tab", { id }),
  tabPaidOutside: (id: string) => invoke<TabRow | null>("tab_paid_outside", { id }),
  tabSendReceipt: (id: string) => invoke<TabRow | null>("tab_send_receipt", { id }),
  deleteTab: (id: string) => invoke<void>("delete_tab", { id }),
  saleCard: () => invoke<Code>("sale_card"),
  cardClaimant: (inboxKey: string) => invoke<ContactRow | null>("card_claimant", { inboxKey }),
  catalogue: () => invoke<ItemRow[]>("catalogue"),
  putItem: (id: string | null, name: string, price: string, soldOut: boolean) => invoke<void>("put_item", { id, name, price, soldOut }),
  removeItem: (id: string) => invoke<void>("remove_item", { id }),
  fiatToPxmr: (text: string) => invoke<number | null>("fiat_to_pxmr", { text }),
  showAmount: (pxmr: number) => invoke<Shown>("show_amount", { pxmr }),
  payBill: (personaHex: string, answersSeq: number | null, amountPxmr: number, memo: string | null, priority = 1) =>
    invoke<string>("pay_bill", { personaHex, answersSeq, amountPxmr, memo, priority }),

  publications: () => invoke<PublicationRow[]>("publications"),
  createPublication: (title: string) => invoke<string>("create_publication", { title }),
  deletePublication: (id: string) => invoke<void>("delete_publication", { id }),
  setPublicationPrice: (id: string, pricePxmr: number) => invoke<void>("set_publication_price", { id, pricePxmr }),
  setSubscriber: (id: string, personaHex: string, on: boolean) => invoke<void>("set_subscriber", { id, personaHex, on }),
  publishIssue: (id: string, period: string, path: string, preferSwarm: boolean, note: string) =>
    invoke<number>("publish_issue", { id, period, path, preferSwarm, note }),
  pressCode: (id: string) => invoke<Code>("press_code", { id }),
  subscriptions: () => invoke<SubscriptionRow[]>("subscriptions"),
  fetchIssue: (publisherHex: string, period: string) => invoke<string>("fetch_issue", { publisherHex, period }),
  refreshShelf: (publisherHex: string) => invoke<number>("refresh_shelf", { publisherHex }),
  askForPeriod: (publisherHex: string, period: string) => invoke<void>("ask_for_period", { publisherHex, period }),
  setMirroring: (publisherHex: string, on: boolean) => invoke<void>("set_mirroring", { publisherHex, on }),
  setMuted: (publisherHex: string, muted: boolean) => invoke<void>("set_muted", { publisherHex, muted }),

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

/// pXMR as a short XMR string: up to six decimals, trailing zeros trimmed,
/// never scientific, and "<0.000001" rather than "0" for dust.
export function fmtXmr(pxmr: number): string {
  const whole = Math.floor(pxmr / 1e12);
  const micro = Math.floor((pxmr % 1e12) / 1e6);
  if (whole === 0 && micro === 0 && pxmr > 0) return "<0.000001 XMR";
  const frac = micro.toString().padStart(6, "0").replace(/0+$/, "");
  return frac ? `${whole}.${frac} XMR` : `${whole} XMR`;
}

export function fmtTime(secs: number): string {
  if (!secs) return "";
  const d = new Date(secs * 1000);
  const today = new Date();
  const sameDay = d.toDateString() === today.toDateString();
  return sameDay ? d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : d.toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}
