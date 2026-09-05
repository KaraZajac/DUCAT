// The desk's commands, typed once. Every screen imports from here and
// nowhere else, so a renamed command breaks one file.

import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
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
  att_hash: string | null;
  att_on_swarm: boolean;
  att_here: boolean;
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
  react_mine: string | null;
  react_theirs: string | null;
  unsent: boolean;
  withdrawn: boolean;
  refused: boolean;
  quote: string | null;
  bill_answered: boolean;
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

export interface GroupRow {
  id_hex: string;
  name: string;
  members: ContactRow[];
  missing: string[];
  mine: string;
  unread: boolean;
  last_body: string | null;
  last_at: number;
}

export interface GroupMessage {
  sender_hex: string;
  sender_name: string;
  mine: boolean;
  gseq: number;
  re_sender_hex: string | null;
  re_seq: number | null;
  quote: string | null;
  reactions: [string, string][];
  unsent: boolean;
  message: MessageRow;
}

export interface Restored {
  contacts: number;
  personas: number;
  restore_height: number;
  display_name: string | null;
}

export interface ListingRow {
  id: string;
  kind: number;
  kind_name: string;
  title: string;
  area: string;
  cell: string;
  price_pxmr: number;
  deposit_pxmr: number;
  specs: Record<string, unknown>;
  private_details: string;
  quantity: number;
  thumb_data_url: string | null;
  photos: string[];
  posted: boolean;
  board: string | null;
  posted_at: number;
  wanted: boolean;
  price_typed: string | null;
  price_currency: string | null;
  shown: Shown;
}

export interface ListingDraft {
  id: string | null;
  kind: number;
  title: string;
  area: string;
  cell: string;
  price_text: string;
  price_is_fiat: boolean;
  specs: Record<string, unknown>;
  private_details: string;
  quantity: number;
}

export interface FoundRow {
  card: string;
  poster: string;
  kind: number;
  kind_name: string;
  title: string;
  area: string;
  cell: string | null;
  price_pxmr: number;
  deposit_pxmr: number;
  expiry: number;
  specs: Record<string, unknown>;
  features: string[];
  quantity: number;
  thumb_data_url: string | null;
  gallery: string | null;
  gallery_dig: string | null;
  mine: boolean;
  shown: Shown;
}

export interface Enquiry {
  title: string;
  price: number;
  deposit: number;
  kind: number;
  listing: string;
}

export interface LedgerEvent {
  txid: string;
  height: number;
  timestamp: number;
  direction: "Received" | "Sent";
  amount_pxmr: number;
  fee_pxmr: number;
  net_pxmr: number;
  balance_after_pxmr: number;
  counterparty: string | null;
  address: string | null;
  donation: boolean;
  source: "Notice" | "OurRecord" | "Unknown";
  note: string | null;
  pending: boolean;
  locked: boolean;
  unlocks_in_blocks: number;
  unexplained: boolean;
  provisional: boolean;
  items: { d: string; a: number }[];
  tax_pxmr: number | null;
  receipted: boolean;
  contact_hex: string | null;
  receipt_by: string | null;
  receipt_at: number;
}

export interface LedgerSummary {
  in_pxmr: number;
  out_pxmr: number;
  net_pxmr: number;
  fees_pxmr: number;
  in_count: number;
  out_count: number;
  tax_collected_pxmr: number;
  donations_pxmr: number;
}

export interface BusinessSummary {
  by_origin: [string, { count: number; take_pxmr: number; tip_pxmr: number }][];
  tax_collected_pxmr: number;
  outstanding_count: number;
  outstanding_pxmr: number;
  sales_count: number;
  sales_pxmr: number;
}

export interface StandingRow {
  id: string;
  persona_hex: string;
  name: string;
  amount_pxmr: number;
  note: string;
  monthly: boolean;
  next_at: number;
}

export type CallState =
  | { kind: "Idle" }
  | { kind: "Outgoing"; contact_hex: string }
  | { kind: "NoAnswer"; contact_hex: string; why: "RangOut" | "Unreached" | "NeverConnected" }
  | { kind: "Incoming"; contact_hex: string; offer_seq: number; call_id: string }
  | { kind: "Answering"; contact_hex: string; offer_seq: number; call_id: string; door: string }
  | { kind: "Active"; contact_hex: string; since_ms: number };

export interface CallView {
  state: CallState;
  contact_name: string | null;
  rx_frames: number;
  tx_frames: number;
  has_audio: boolean;
}


export type OrderRow = {
  id: string;
  number: number;
  lines: [string, number][];
  total_pxmr: number;
  tax_pxmr: number | null;
  address: string;
  pay_uri: string;
  pay_svg: string;
  card: string | null;
  card_svg: string | null;
  state: "Awaiting" | "Seen" | "Confirmed" | "Abandoned";
  placed_at: number;
  ready_at: number;
  customer: string | null;
  shown: Shown;
};

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

  groups: () => invoke<GroupRow[]>("groups"),
  createGroup: (name: string, members: string[]) => invoke<GroupRow>("create_group", { name, members }),
  addToGroup: (idHex: string, personaHex: string) => invoke<void>("add_to_group", { idHex, personaHex }),
  groupThread: (idHex: string) => invoke<GroupMessage[]>("group_thread", { idHex }),
  sendGroup: (idHex: string, body: string, reSenderHex: string | null = null, reSeq: number | null = null) =>
    invoke<boolean>("send_group", { idHex, body, reSenderHex, reSeq }),
  reactInGroup: (idHex: string, senderHex: string, seq: number, emoji: string) => invoke<boolean>("react_in_group", { idHex, senderHex, seq, emoji }),
  unsendInGroup: (idHex: string, seq: number) => invoke<boolean>("unsend_in_group", { idHex, seq }),
  markGroupSeen: (idHex: string) => invoke<void>("mark_group_seen", { idHex }),

  backupInfo: () => invoke<{ exported_at: number; has_wallet: boolean }>("backup_info"),
  exportBackup: (path: string, passphrase: string) => invoke<number>("export_backup", { path, passphrase }),
  importBackup: (path: string, passphrase: string) => invoke<Restored>("import_backup", { path, passphrase }),

  listings: () => invoke<ListingRow[]>("listings"),
  saveListing: (draft: ListingDraft) => invoke<ListingRow>("save_listing", { draft }),
  removeListing: (id: string) => invoke<void>("remove_listing", { id }),
  postListing: (id: string) => invoke<boolean>("post_listing", { id }),
  unpostListing: (id: string) => invoke<void>("unpost_listing", { id }),
  addListingPhoto: (id: string, path: string) => invoke<number>("add_listing_photo", { id, path }),
  removeListingPhoto: (id: string, index: number) => invoke<void>("remove_listing_photo", { id, index }),
  setListingCover: (id: string, index: number) => invoke<boolean>("set_listing_cover", { id, index }),
  browseCached: (cell: string, kind: number | null) => invoke<FoundRow[]>("browse_cached", { cell, kind }),
  browse: (cell: string, kind: number | null) => invoke<FoundRow[]>("browse", { cell, kind }),
  fetchGallery: (share: string, digestHex: string) => invoke<string[]>("fetch_gallery", { share, digestHex }),
  pictureDataUrl: (path: string) => invoke<string>("picture_data_url", { path }),
  enquiryAbout: (personaHex: string) => invoke<Enquiry | null>("enquiry_about", { personaHex }),

  ledger: (fromTs: number, toTs: number) => invoke<{ events: LedgerEvent[]; summary: LedgerSummary; business: BusinessSummary }>("ledger", { fromTs, toTs }),
  exportLedger: (path: string, json: boolean) => invoke<number>("export_ledger", { path, json }),

  requestPayment: (personaHex: string, amountPxmr: number, note: string) => invoke<void>("request_payment", { personaHex, amountPxmr, note }),
  standingBills: () => invoke<StandingRow[]>("standing_bills"),
  addStandingBill: (personaHex: string, amountPxmr: number, note: string, monthly: boolean) => invoke<void>("add_standing_bill", { personaHex, amountPxmr, note, monthly }),
  stopStandingBill: (id: string) => invoke<void>("stop_standing_bill", { id }),
  donateCode: () => invoke<Code>("donate_code"),

  sendReply: (personaHex: string, body: string, reSeq: number, reOwn: boolean) => invoke<void>("send_reply", { personaHex, body, reSeq, reOwn }),
  declineBill: (personaHex: string, seq: number, timestamp: number) => invoke<void>("decline_bill", { personaHex, seq, timestamp }),
  cancelBill: (personaHex: string, seq: number, timestamp: number) => invoke<void>("cancel_bill", { personaHex, seq, timestamp }),
  shareIntroCard: (personaHex: string) => invoke<void>("share_intro_card", { personaHex }),
  shareContact: (personaHex: string, otherHex: string) => invoke<void>("share_contact", { personaHex, otherHex }),
  memoStart: () => invoke<void>("memo_start"),
  memoElapsedMs: () => invoke<number | null>("memo_elapsed_ms"),
  memoCancel: () => invoke<void>("memo_cancel"),
  memoStopSend: (personaHex: string) => invoke<number>("memo_stop_send", { personaHex }),
  attachmentDataUrl: (ctHashHex: string, mime: string | null) => invoke<string | null>("attachment_data_url", { ctHashHex, mime }),
  sendAttachment: (personaHex: string, path: string, caption: string | null) => invoke<void>("send_attachment", { personaHex, path, caption }),
  attachmentPath: (ctHashHex: string) => invoke<string | null>("attachment_path", { ctHashHex }),
  fetchSwarmAttachment: (personaHex: string, seq: number, outgoing: boolean) => invoke<string>("fetch_swarm_attachment", { personaHex, seq, outgoing }),

  presentSale: (lines: [string, number][], taxPxmr: number | null) => invoke<{ code: Code; tab: TabRow }>("present_sale", { lines, taxPxmr }),
  salesInProgress: () => invoke<[string, TabRow][]>("sales_in_progress"),

  callState: () => invoke<CallView>("call_state"),
  placeCall: (personaHex: string) => invoke<void>("place_call", { personaHex }),
  answerCall: () => invoke<void>("answer_call"),
  declineCall: () => invoke<void>("decline_call"),
  hangUp: () => invoke<void>("hang_up"),
  dismissCall: () => invoke<void>("dismiss_call"),
  takeNotices: () => invoke<{ title: string; body: string; open_thread: string | null; at_ms: number }[]>("take_notices"),

  // the kiosk
  orders: () => invoke<OrderRow[]>("orders"),
  placeOrder: (lines: [string, number][], taxPxmr: number | null, withCard: boolean) =>
    invoke<OrderRow>("place_order", { lines, taxPxmr, withCard }),
  orderCard: (id: string) => invoke<OrderRow>("order_card", { id }),
  abandonOrder: (id: string) => invoke<void>("abandon_order", { id }),
  sayReady: (id: string) => invoke<void>("say_ready", { id }),

  react: (personaHex: string, seq: number, reOwn: boolean, emoji: string) => invoke<void>("react", { personaHex, seq, reOwn, emoji }),
  retractMessage: (personaHex: string, seq: number) => invoke<void>("retract_message", { personaHex, seq }),
  deleteMessage: (personaHex: string, seq: number, outgoing: boolean, timestamp: number) => invoke<void>("delete_message", { personaHex, seq, outgoing, timestamp }),
  deleteThread: (personaHex: string) => invoke<void>("delete_thread", { personaHex }),
  disappearAfter: (personaHex: string) => invoke<number>("disappear_after", { personaHex }),
  setDisappearAfter: (personaHex: string, secs: number) => invoke<void>("set_disappear_after", { personaHex, secs }),
  draft: (personaHex: string) => invoke<string>("draft", { personaHex }),
  saveDraft: (personaHex: string, text: string) => invoke<void>("save_draft", { personaHex, text }),
  setChatVisible: (personaHex: string, visible: boolean) => invoke<void>("set_chat_visible", { personaHex, visible }),
  pickSavePath: async (name: string): Promise<string | null> => {
    const r = await save({ defaultPath: name });
    return typeof r === "string" ? r : null;
  },

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
