//! Paying: the customer's side of §16.13. Money goes out of the wallet
//! first; then the thread is told, with the transaction it should look
//! for — a payment notice a receipt can point back at. The phone's Pay.kt,
//! without the screen.

use crate::mailbox::Outgoing;
use crate::{log, App, Error};

const TAG: &str = "Pay";

/// The address a bill should be paid to: the one it named, else the one
/// the contact's card or messages settled on. A held (pending) address is
/// never paid — that is the whole point of holding it.
pub fn pay_to(bill_payto: Option<&str>, their_address: Option<&str>) -> Option<String> {
    bill_payto
        .filter(|p| !p.trim().is_empty())
        .or(their_address.filter(|p| !p.trim().is_empty()))
        .map(String::from)
}

impl App {
    /// Pay a contact: `answers_seq` is the bill (their kind-1 row) this
    /// pays, or None for an unprompted payment — a donation when the
    /// thread was born from a `donate` card. Returns the transaction id.
    pub fn pay_bill(
        &self,
        persona_hex: &str,
        answers_seq: Option<u64>,
        amount_pxmr: u64,
        memo: Option<&str>,
        priority: u32,
    ) -> Result<String, Error> {
        let contact = self.contact(persona_hex).ok_or_else(|| Error::Refused("no such contact".into()))?;
        let thread = self.thread(persona_hex);
        let bill = answers_seq.and_then(|s| thread.iter().find(|m| !m.outgoing && m.kind == 1 && m.seq == s));
        if answers_seq.is_some() && bill.is_none() {
            return Err(Error::Refused("that bill is not in the thread".into()));
        }
        let to = pay_to(bill.and_then(|b| b.payto.as_deref()), contact.their_address.as_deref())
            .ok_or_else(|| Error::Refused("they have not given a payment address yet".into()))?;
        let is_donation = answers_seq.is_none() && contact.card_purpose.as_deref() == Some("donate");
        let memo = memo.map(str::trim).filter(|m| !m.is_empty());
        let r = self.send_xmr(&to, amount_pxmr, Some(persona_hex), memo, priority, is_donation)?;
        let body = memo.map(String::from).unwrap_or_else(|| if is_donation { "A donation".into() } else { "Payment".into() });
        let out = Outgoing {
            body,
            kind: 2,
            amount_pxmr: Some(amount_pxmr),
            re_seq: answers_seq,
            re_own: false,
            txid_hex: Some(r.txid_hex.clone()),
            ..Default::default()
        };
        if let Err(e) = self.send(&contact, out) {
            // The money went; the word can follow on a retry, and the
            // receipt loop on their side finds the output regardless.
            log::warn(TAG, format!("sent, but could not tell {}: {e}", contact.display_name()));
        }
        Ok(r.txid_hex)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bill_names_the_address_or_the_contact_does() {
        assert_eq!(pay_to(Some("4bill"), Some("4contact")).as_deref(), Some("4bill"));
        assert_eq!(pay_to(None, Some("4contact")).as_deref(), Some("4contact"));
        assert_eq!(pay_to(Some(" "), Some("4contact")).as_deref(), Some("4contact"));
        assert_eq!(pay_to(None, None), None);
    }
}
