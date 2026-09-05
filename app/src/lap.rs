//! The lap: what the desk does on its own, over and over, while it is
//! open — the phone's `Poller`, without the alarms. One thread, one loop:
//! answers to our cards, everyone's log, slot insurance, and once an hour
//! the things that only drift.

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
                            app.refresh_shelves();
                            app.sweep_site_orphans();
                            app.sweep_release_orphans();
                            last_hourly = Some(Instant::now());
                        }
                    }
                    std::thread::sleep(POLL_EVERY);
                }
            })
            .ok();
    }

    /// One turn of the lap: cards, then logs, then insurance. Public so a
    /// harness can turn it by hand.
    pub fn lap_once(&self) {
        let claimed = self.collect_claims(None);
        if claimed > 0 {
            log::info(TAG, format!("{claimed} card(s) answered"));
        }
        let got = self.poll();
        if got > 0 {
            log::info(TAG, format!("{got} message(s) arrived"));
        }
        self.verify_last_writes();
        self.retry_group_outbox();
        self.listings_lap();
        self.sweep_abandoned_tabs(&[]);
    }
}
