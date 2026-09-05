// The sidebar's icons: one line set, drawn here so they match each other
// and take the text colour. 24-unit box, 1.7 stroke, round joins.
const wrap = (d: string) =>
  `<svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${d}</svg>`;

export const icons: Record<string, string> = {
  chat: wrap('<path d="M4 5.5A1.5 1.5 0 0 1 5.5 4h13A1.5 1.5 0 0 1 20 5.5v9a1.5 1.5 0 0 1-1.5 1.5H9l-5 4z"/>'),
  wallet: wrap('<path d="M3.5 7.5A2.5 2.5 0 0 1 6 5h10.5"/><path d="M3.5 7.5V17a2 2 0 0 0 2 2h13a1.5 1.5 0 0 0 1.5-1.5v-8A1.5 1.5 0 0 0 18.5 8H6a2.5 2.5 0 0 1-2.5-.5z"/><circle cx="16.5" cy="13.5" r="1"/>'),
  till: wrap('<path d="M4 10.5h16V19a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1z"/><path d="M7 6.5h10l1.5 4h-13z"/><path d="M8 14.5h3M13 14.5h3"/>'),
  kiosk: wrap('<path d="M3.5 9l1.7-4.5h13.6L20.5 9"/><path d="M4 9h16v10.5H4z"/><path d="M9.5 19.5v-5h5v5"/>'),
  activity: wrap('<path d="M4 6.5h16M4 12h16M4 17.5h9"/>'),
  library: wrap('<path d="M4.5 4.5h11a2 2 0 0 1 2 2V20H6.5a2 2 0 0 0-2 2z"/><path d="M4.5 4.5V22"/><path d="M17.5 7h2v13h-2"/>'),
  market: wrap('<path d="M3.5 12.5l9-9h8v8l-9 9z"/><circle cx="15.5" cy="8.5" r="1.2"/>'),
  files: wrap('<path d="M6 3.5h8l4.5 4.5v12.5H6z"/><path d="M14 3.5V8h4.5"/>'),
  sites: wrap('<circle cx="12" cy="12" r="8.5"/><path d="M3.5 12h17"/><path d="M12 3.5c3.2 3 3.2 14 0 17M12 3.5c-3.2 3-3.2 14 0 17"/>'),
  me: wrap('<circle cx="12" cy="8" r="3.8"/><path d="M4.5 20.5a7.5 7.5 0 0 1 15 0"/>'),
  // Chat controls, same pen.
  react: wrap('<circle cx="12" cy="12" r="8.5"/><path d="M8.5 14.5c1 1.3 2.2 2 3.5 2s2.5-.7 3.5-2"/><path d="M9.2 9.5h.01M14.8 9.5h.01" stroke-width="2.4"/>'),
  reply: wrap('<path d="M9.5 7L4.5 12l5 5"/><path d="M4.5 12h9a6 6 0 0 1 6 6v.5"/>'),
  copy: wrap('<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M15 9V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h3"/>'),
  unsend: wrap('<path d="M8 7L3.5 11.5 8 16"/><path d="M3.5 11.5H15a5 5 0 0 1 0 10h-3"/>'),
  close: wrap('<path d="M6.5 6.5l11 11M17.5 6.5l-11 11"/>'),
  call: wrap('<path d="M5.5 3.5h3l1.8 4.5-2.3 1.5a11 11 0 0 0 6.5 6.5l1.5-2.3 4.5 1.8v3a2 2 0 0 1-2 2A16.5 16.5 0 0 1 3.5 5.5a2 2 0 0 1 2-2z"/>'),
  attach: wrap('<path d="M20 11.5l-8.2 8.2a5.2 5.2 0 0 1-7.4-7.4l8.6-8.6a3.5 3.5 0 0 1 4.9 4.9l-8.6 8.6a1.8 1.8 0 0 1-2.5-2.5L14.6 7"/>'),
  mic: wrap('<rect x="9" y="3.5" width="6" height="11" rx="3"/><path d="M5.5 11.5a6.5 6.5 0 0 0 13 0"/><path d="M12 18v3M9 21h6"/>'),
  more: wrap('<circle cx="6" cy="12" r="1.4" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="1.4" fill="currentColor" stroke="none"/><circle cx="18" cy="12" r="1.4" fill="currentColor" stroke="none"/>'),
  send: wrap('<path d="M4 12L20 4l-4 16-4-7z"/><path d="M12 13l8-9"/>'),
  status: wrap('<path d="M3.5 12h3.5l3-7 4 14 3-7h3.5"/>'),
};
