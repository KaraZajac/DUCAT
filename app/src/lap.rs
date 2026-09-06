//! The lap: what the desk does on its own, over and over, while it is
//! open — the phone's `Poller`, without the alarms. One thread, one loop:
//! answers to our cards, everyone's log, slot insurance, and once an hour
//! the things that only drift.

use ducat_mobile::node::{node_changed_keys, node_wait_change};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::{log, App};

const TAG: &str = "Lap";
/// How often the logs are read while the window is open.
const POLL_EVERY: Duration = Duration::from_secs(15);
/// How often the seeds are put back and the drift checked.
const HOURLY: Duration = Duration::from_secs(60 * 60);
/// How often the wallet takes a scan step.
const WALLET_EVERY: Duration = Duration::from_secs(20);

static RUNNING: AtomicBool = AtomicBool::new(false);

impl App {
    /// Start the lap thread if it is not running. Safe to call twice.
    pub fn start_lap(&self) {
        if RUNNING.swap(true, Ordering::AcqRel) {
            return;
        }
        // The wallet has its own lane: a scan step is seconds against a
        // node, and the logs must not wait behind it.
        let wallet = self.clone();
        std::thread::Builder::new()
            .name("desk-wallet".into())
            .spawn(move || loop {
                if wallet.node_status().public_internet_ready || wallet.last_good_node().is_some() {
                    wallet.wallet_lap();
                }
                std::thread::sleep(WALLET_EVERY);
            })
            .ok();
        let app = self.clone();
        std::thread::Builder::new()
            .name("desk-lap".into())
            .spawn(move || {
                let mut last_hourly: Option<Instant> = None;
                loop {
                    let status = app.node_status();
                    if status.public_internet_ready {
                        app.lap_once();
                        if last_hourly.map_or(true, |t| t.elapsed() >= HOURLY) {
                            app.reseed_all_sites();
                            app.reseed_all_releases();
                            app.reseed_issues();
                            app.reseed_library();
                            app.reseed_galleries();
                            app.reseed_attachments();
                            app.sweep_attachments();
                            app.refresh_shelves();
                            app.sweep_site_orphans();
                            app.sweep_release_orphans();
                            last_hourly = Some(Instant::now());
                        }
                    }
                    // Sleep until the network rings or the lap is due. A
                    // watched log that changed wakes the lap for that
                    // contact alone; everyone else keeps their turn.
                    if node_wait_change(POLL_EVERY.as_millis() as u32) {
                        let rang = app.mark_changed(&node_changed_keys());
                        if rang > 0 {
                            log::info(TAG, format!("{rang} log(s) rang"));
                        }
                    }
                }
            })
            .ok();
    }

    /// One turn of the lap: cards, then logs, then insurance. Public so a
    /// harness can turn it by hand.
    pub fn lap_once(&self) {
        // Each phase timed, and the lap narrated when it ran long: a lap
        // that takes two minutes is a phase that took two minutes, and
        // the number says which.
        let t0 = Instant::now();
        let mut phases: Vec<(&str, u128)> = Vec::new();
        let mut phase = |name: &'static str, f: &mut dyn FnMut()| {
            let t = Instant::now();
            f();
            phases.push((name, t.elapsed().as_millis()));
        };
        phase("cards", &mut || {
            let claimed = self.collect_claims(None);
            if claimed > 0 {
                log::info(TAG, format!("{claimed} card(s) answered"));
            }
        });
        phase("logs", &mut || {
            let got = self.poll();
            if got > 0 {
                log::info(TAG, format!("{got} message(s) arrived"));
            }
        });
        phase("calls", &mut || self.calls_noticed());
        phase("verify", &mut || self.verify_last_writes());
        phase("retries", &mut || self.retry_group_outbox());
        phase("boards", &mut || {
            let on_boards = self.groups_lap();
            if on_boards > 0 {
                log::info(TAG, format!("{on_boards} group message(s) arrived"));
            }
        });
        phase("listings", &mut || self.listings_lap());
        phase("market", &mut || self.market_lap());
        phase("bills", &mut || self.run_due_bills());
        phase("attachment", &mut || {
            self.fetch_one_attachment();
        });
        phase("expiry", &mut || {
            self.expire_all();
            self.expire_orders();
            self.sweep_abandoned_tabs(&[]);
        });
        phase("feeds", &mut || self.feeds_lap());
        let total = t0.elapsed().as_millis();
        if total > 30_000 {
            let slow: Vec<String> = phases.iter().filter(|(_, ms)| *ms >= 1_000).map(|(n, ms)| format!("{n} {}s", ms / 1000)).collect();
            log::info(TAG, format!("lap took {}s — {}", total / 1000, slow.join(", ")));
        }
    }
}
