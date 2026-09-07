# Application images

Screenshots of DUCAT running on a phone — captured from the live app, not
mocked up. This directory is the website's gallery; the README's own set
lives in `docs/screenshots/` and is a subset of the same captures.

It stays at the repo root because the README cites it from here. The site
lives in `docs/` and is published from `docs/dist/`, which `docs/package.json`
assembles by copying this directory in — so the page serves these at
`/images/…` without a second copy in git. New captures go here; nothing else
needs changing.

Phone shots are 1080×2400; the `desk-*.png` shots are the desktop client's
window, cropped to the window. Names say what the screen is; the theme is the
light default unless the name says otherwise. Refreshed 2026-09-06.

| File | What it shows |
|---|---|
| `onboarding-1.png` | First run, step 1: create your identity — a keypair, no account |
| `onboarding-2.png` | Step 3: the wallet, generated and held on the phone |
| `onboarding-3.png` | Step 4: the PIN, with the no-reset warning stated plainly |
| `onboarding-4.png` | Step 5: how strangers trust each other — both stake, finishing returns it |
| `onboarding-5.png` | Step 6: the backup — created, and honestly "not a backup yet" until it leaves the phone |
| `home.png` | Personal home: balance in USD and XMR, sync state, the running-low-on-notes card with Top up, the mode tiles |
| `home-dark.png` | The same home in the Mocha (dark) theme |
| `home-arabic.png` | The same home in Arabic — full RTL mirror, Eastern Arabic numerals |
| `chat-list.png` | The Chat tab: message search, groups above the pairwise threads, previews with receipt icons |
| `chat-thread.png` | A conversation carrying money: a payment, an itemised bill marked paid, the receipt |
| `chat-dark.png` | A money-heavy thread in the dark theme |
| `chat-search.png` | Searching what people said — name hits ranked above message hits |
| `group-chat.png` | The same group a moment later: a reaction, and a reply quoting the message it answers |
| `activity.png` | The statement: running balance, receipt search, CSV export |
| `pay.png` | Paying or requesting: amount in fiat or XMR, fee arithmetic, what is left after |
| `send-receive.png` | Picking who to pay: contacts with their key fingerprints |
| `my-card.png` | Your card: claim-once QR that opens a conversation |
| `settings.png` | Settings: language, appearance, distance, prices |
| `settings-business.png` | The business half: fares region, sales-tax rate, privacy, backup |
| `profile.png` | Profiles: who this phone can be — each its own key and contacts — and the one being worn: picture, name, pronouns, ways to reach you, all of it optional |
| `profile-privacy.png` | The linkability cost of direct payment, stated where you choose it |
| `modes.png` | Operating modes: the whole app hands over to a till, a bar, a taxi, a kiosk |
| `pos.png` | Point of sale: items rung up, the computed tax line |
| `pos-charge.png` | The one code a customer scans; the itemised bill lands on their phone |
| `bar-tab.png` | Bar tabs: open tabs, billed-and-waiting, one bill at close |
| `kiosk.png` | Kiosk mode, customer side: tap what you want, tax shown before you commit |
| `kiosk-staff.png` | Kiosk staff panel behind the PIN: the orders, two of them walked away |
| `donations.png` | A standing donation code, its linkability cost stated on screen |
| `hail.png` | Hailing a ride: route, distance, time, fare against a rideshare's price |
| `taxi-drive.png` | The driver's day: watch a stand or an area for hails |
| `taxi-meter.png` | The meter: rate typed once, a pickup code, terms land in the chat at start |
| `marketplace.png` | Marketplace browse: a listing found on the board, price and stakes, "Ask about it" |
| `hire-help.png` | Hire help browse: a skill found nearby, priced per hour |
| `rent-search.png` | Finding gear nearby: a live listing with price per day and stakes |
| `renting-form.png` | Renting out a vehicle: the public half of the form — one line, area in human words, price per day with the XMR conversion, make and model |
| `renting-private.png` | The same form further down: the part that never goes on the board — where to meet and what they need to know is sent in the conversation once something is agreed |
| `onboarding-name.png` | Step 2: what people should call you, and whether contacts may pay you directly — the linkability cost stated where you choose |
| `marketplace-listing.png` | A listing opened: its bundle's pictures, the seller's description, the price they typed beside today's conversion |
| `marketplace-worldwide.png` | Worldwide: publications on topic boards, "Everything" reading every shelf at once |
| `group-board.png` | A group of three on a shared board (§16.24): one record per generation, a question and its answer from another phone |
| `status.png` | Status: the node, its peers, the routing table's live and dead counts, Reconnect |
| `library.png` | The Library: issues filed by publisher, saved out rather than opened |
| `feed.png` | The Feed: posts from people you keep, your own home |
| `desk-chat.png` | The desktop client (1536×1338 window): Chat, groups on boards above the pairwise threads |
| `desk-feed.png` | Desk: the Feed with a post carrying a picture and a file |
| `desk-wallet.png` | Desk: the wallet — balance, sends, notes, the node it asks |
| `desk-till.png` | Desk: the till — a sale rung up, bar tabs |
| `desk-kiosk.png` | Desk: the kiosk — an order with a code any Monero wallet can pay, and a card for a DUCAT customer |
| `desk-activity.png` | Desk: the statement |
| `desk-library.png` | Desk: the Library's reading side |
| `desk-press.png` | Desk: the Library's press side — publications, issues, the room code |
| `desk-market.png` | Desk: the Market near a cell, thumbnails on the notices |
| `desk-market-world.png` | Desk: the worldwide market's shelves |
| `desk-files.png` | Desk: files kept and served |
| `desk-sites.png` | Desk: sites kept and served, the sealed room |
| `desk-me.png` | Desk: the Me page — name, picture, ways to reach you, the car |
| `desk-status.png` | Desk: the Status page — node, routing table, Reconnect, log |
