//! The catalogue: what a till sells, priced in the reader's currency and
//! converted at the moment of sale — the phone's `Catalogue.kt`. Prices
//! are kept as typed, in fiat, because a menu is written in the money the
//! customers think in; the rate turns them into XMR at the counter.

use serde::{Deserialize, Serialize};

use crate::contacts::bump;
use crate::{App, Error};

const STORE: &str = "ducat_catalogue";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub name: String,
    /// As typed, e.g. "4.50".
    pub price: String,
    #[serde(default)]
    pub currency: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub archived: bool,
    #[serde(rename = "soldout", default)]
    pub sold_out: bool,
    #[serde(default)]
    pub sort: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Snag {
    NoRate,
    WrongCurrency,
    Unpriceable,
}

#[derive(Clone, Debug, Serialize)]
pub struct Priced {
    pub pxmr: u64,
    pub stale_secs: u64,
}

/// "4.50" → minor units at `exponent` decimals, or None.
pub fn parse_money(s: &str) -> Option<(u64, u32)> {
    let t = s.trim().replace(',', ".");
    let (whole, frac) = match t.split_once('.') {
        Some((w, f)) => (w, f),
        None => (t.as_str(), ""),
    };
    if whole.is_empty() && frac.is_empty() || !whole.chars().all(|c| c.is_ascii_digit()) || !frac.chars().all(|c| c.is_ascii_digit()) || frac.len() > 8 {
        return None;
    }
    let w: u64 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
    let f: u64 = if frac.is_empty() { 0 } else { frac.parse().ok()? };
    let exp = frac.len() as u32;
    Some((w * 10u64.pow(exp) + f, exp))
}

impl App {
    pub fn catalogue(&self) -> Vec<Item> {
        let mut items: Vec<Item> = self.store(STORE).get("items").unwrap_or_default();
        items.retain(|i| !i.id.is_empty() && !i.name.is_empty());
        items.sort_by(|a, b| a.sort.cmp(&b.sort).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        items
    }

    pub fn catalogue_live(&self) -> Vec<Item> {
        self.catalogue().into_iter().filter(|i| !i.archived).collect()
    }

    pub fn put_item(&self, item: Item) -> Result<(), Error> {
        let mut items: Vec<Item> = self.catalogue().into_iter().filter(|i| i.id != item.id).collect();
        items.push(item);
        self.store(STORE).put("items", &items)?;
        bump();
        Ok(())
    }

    pub fn remove_item(&self, id: &str) -> Result<(), Error> {
        let items: Vec<Item> = self.catalogue().into_iter().filter(|i| i.id != id).collect();
        self.store(STORE).put("items", &items)?;
        bump();
        Ok(())
    }

    pub fn draft_item(&self, name: &str, price: &str) -> Item {
        Item {
            id: format!("{:x}", crate::contacts::now_ms()),
            name: ducat_mobile::contacts::clean_display_text(name.trim().to_string()),
            price: price.trim().to_string(),
            currency: self.rate_currency(),
            category: String::new(),
            archived: false,
            sold_out: false,
            sort: crate::contacts::now_ms(),
        }
    }

    /// An item's price in XMR at the cached rate, rounded down.
    pub fn price_item(&self, item: &Item) -> Result<Priced, Snag> {
        if !item.currency.is_empty() && item.currency != self.rate_currency() {
            return Err(Snag::WrongCurrency);
        }
        let (rate, at) = self.rate_cached_pair().ok_or(Snag::NoRate)?;
        if rate <= 0.0 {
            return Err(Snag::NoRate);
        }
        let pxmr = self.fiat_to_pxmr(&item.price, rate).ok_or(Snag::Unpriceable)?;
        Ok(Priced { pxmr, stale_secs: App::now().saturating_sub(at) })
    }

    /// Fiat text to pXMR at `rate` (fiat per XMR), rounded down; None when
    /// it does not parse or comes to nothing.
    pub fn fiat_to_pxmr(&self, text: &str, rate: f64) -> Option<u64> {
        let (minor, exp) = parse_money(text)?;
        let fiat = minor as f64 / 10f64.powi(exp as i32);
        let xmr = fiat / rate;
        let pxmr = (xmr * 1e12).floor();
        if !pxmr.is_finite() || pxmr <= 0.0 {
            return None;
        }
        Some(pxmr as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_parses_as_typed() {
        assert_eq!(parse_money("4.50"), Some((450, 2)));
        assert_eq!(parse_money("4"), Some((4, 0)));
        assert_eq!(parse_money("1,25"), Some((125, 2)));
        assert_eq!(parse_money(""), None);
        assert_eq!(parse_money("x"), None);
    }

    #[test]
    fn a_fiat_price_becomes_xmr_at_the_rate() {
        let dir = std::env::temp_dir().join(format!("ducat-catalogue-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = App::open(&dir).unwrap();
        assert_eq!(app.fiat_to_pxmr("150", 150.0), Some(1_000_000_000_000));
        assert_eq!(app.fiat_to_pxmr("1.50", 150.0), Some(10_000_000_000));
        assert_eq!(app.fiat_to_pxmr("0", 150.0), None);
        let it = app.draft_item("Coffee", "4.50");
        app.put_item(it.clone()).unwrap();
        assert_eq!(app.catalogue_live().len(), 1);
        assert!(matches!(app.price_item(&it), Err(Snag::NoRate)));
    }
}
