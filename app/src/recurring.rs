//! Standing bills: a request that goes out again every week or month —
//! rent, a subscription kept by hand. The phone's `Recurring.kt`.

use serde::{Deserialize, Serialize};

use crate::contacts::{bump, now_ms};
use crate::mailbox::Outgoing;
use crate::{log, App, Error};

const TAG: &str = "Recurring";
const STORE: &str = "ducat_recurring";
pub const REQUEST_NOTE: &str = "Payment request";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StandingBill {
    pub id: String,
    #[serde(rename = "who")]
    pub persona_hex: String,
    #[serde(rename = "amt")]
    pub amount_pxmr: u64,
    #[serde(default)]
    pub note: String,
    #[serde(rename = "m")]
    pub monthly: bool,
    /// Millis.
    #[serde(rename = "next")]
    pub next_at: u64,
}

/// A month on, by the calendar: the same day next month, or the last
/// day of it when that month is shorter.
pub fn advance(from_ms: u64, monthly: bool) -> u64 {
    if !monthly {
        return from_ms + 7 * 24 * 60 * 60 * 1000;
    }
    let secs = from_ms / 1000;
    let tod = secs % 86_400;
    let days = (secs / 86_400) as i64;
    let (y, m, d) = civil_from_days(days);
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    let nd = d.min(days_in_month(ny, nm));
    let ndays = days_from_civil(ny, nm, nd);
    (ndays as u64 * 86_400 + tod) * 1000
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
    }
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

impl App {
    pub fn standing_bills(&self) -> Vec<StandingBill> {
        self.store(STORE).get("bills").unwrap_or_default()
    }

    fn save_bills(&self, bills: &[StandingBill]) -> Result<(), Error> {
        self.store(STORE).put("bills", &bills)?;
        bump();
        Ok(())
    }

    pub fn add_standing_bill(&self, persona_hex: &str, amount_pxmr: u64, note: &str, monthly: bool) -> Result<StandingBill, Error> {
        let b = StandingBill {
            id: format!("{:x}-{}", now_ms(), persona_hex.chars().take(8).collect::<String>()),
            persona_hex: persona_hex.to_string(),
            amount_pxmr,
            note: ducat_mobile::contacts::clean_display_text(note.trim().to_string()),
            monthly,
            next_at: advance(now_ms(), monthly),
        };
        let mut all = self.standing_bills();
        all.push(b.clone());
        self.save_bills(&all)?;
        log::info(TAG, format!("scheduled {} bill for {}…", if monthly { "monthly" } else { "weekly" }, &persona_hex[..8.min(persona_hex.len())]));
        Ok(b)
    }

    pub fn stop_standing_bill(&self, id: &str) -> Result<(), Error> {
        let all: Vec<StandingBill> = self.standing_bills().into_iter().filter(|b| b.id != id).collect();
        self.save_bills(&all)
    }

    /// Send every bill that has come due, and move it on; a bill whose
    /// contact is gone is dropped.
    pub fn run_due_bills(&self) {
        let now = now_ms();
        let bills = self.standing_bills();
        if !bills.iter().any(|b| b.next_at <= now) {
            return;
        }
        let mut next: Vec<StandingBill> = Vec::new();
        for b in bills {
            if b.next_at > now {
                next.push(b);
                continue;
            }
            let Some(c) = self.contact(&b.persona_hex) else {
                log::warn(TAG, "recurring bill points at a forgotten contact; dropping");
                continue;
            };
            let out = Outgoing {
                body: if b.note.trim().is_empty() { REQUEST_NOTE.into() } else { b.note.clone() },
                kind: 1,
                amount_pxmr: Some(b.amount_pxmr),
                payto: self.address_for(&b.persona_hex),
                ..Default::default()
            };
            match self.send(&c, out) {
                Ok(_) => {
                    log::info(TAG, format!("standing bill sent to {}: {} XMR", c.display_name(), crate::wallet::format_xmr(b.amount_pxmr)));
                    next.push(StandingBill { next_at: advance(b.next_at, b.monthly), ..b });
                }
                Err(e) => {
                    log::warn(TAG, format!("recurring bill not sent: {e}"));
                    next.push(b);
                }
            }
        }
        let _ = self.save_bills(&next);
    }

    /// A one-off request for payment in a thread (§16.13's kind 1).
    pub fn request_payment(&self, persona_hex: &str, amount_pxmr: u64, note: &str) -> Result<(), Error> {
        let c = self.contact(persona_hex).ok_or_else(|| Error::Refused("no such contact".into()))?;
        if amount_pxmr == 0 {
            return Err(Error::Refused("a request needs an amount".into()));
        }
        self.send(
            &c,
            Outgoing {
                body: if note.trim().is_empty() { REQUEST_NOTE.into() } else { ducat_mobile::contacts::clean_display_text(note.trim().to_string()) },
                kind: 1,
                amount_pxmr: Some(amount_pxmr),
                payto: self.address_for(persona_hex),
                ..Default::default()
            },
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_month_on_is_the_same_day_or_the_last_the_month_has() {
        // 2026-01-31 12:00 UTC → 2026-02-28 12:00.
        let jan31 = (days_from_civil(2026, 1, 31) as u64 * 86_400 + 43_200) * 1000;
        let feb = advance(jan31, true);
        assert_eq!(civil_from_days((feb / 1000 / 86_400) as i64), (2026, 2, 28));
        assert_eq!((feb / 1000) % 86_400, 43_200);
        let mar15 = (days_from_civil(2026, 3, 15) as u64 * 86_400) * 1000;
        assert_eq!(civil_from_days((advance(mar15, true) / 1000 / 86_400) as i64), (2026, 4, 15));
        assert_eq!(advance(mar15, false), mar15 + 7 * 86_400 * 1000);
        assert_eq!(civil_from_days(days_from_civil(2024, 2, 29)), (2024, 2, 29));
    }
}
