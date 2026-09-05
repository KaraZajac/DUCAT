//! The backup bundle: the same sealed file the phone writes, so a desk can
//! restore a phone and a phone a desk. The identity, the wallet's spend
//! key, every contact's logs and counters, the prekeys, and the app state
//! the tables hold — under one passphrase. The phone's `BackupSettings`
//! and `ContactStore.backup*`, with the bundle's format in `ducat-mobile`.

use std::path::Path;

use ducat_mobile::{BackupInput, ContactBackup, PersonaBackup, PrekeyEntry, RestoredBackup};
use serde_json::{json, Map, Value};

use crate::contacts::{bump, hex, hex_to_bytes, unb64, Contact, CONTACTS};
use crate::{log, App, Error};

const TAG: &str = "Backup";

impl From<ducat_mobile::BackupError> for Error {
    fn from(e: ducat_mobile::BackupError) -> Self {
        Error::Refused(e.to_string())
    }
}

/// The keys of the contacts table a bundle carries verbatim.
fn backup_key(k: &str) -> bool {
    k.starts_with("thread_") || k.starts_with("disappear_") || k.starts_with("usedtheirs_") || k.starts_with("sub_") || k.starts_with("mode_persona_")
}

const APP_STATE_KEYS: [&str; 7] = ["tabs_v1", "publish_address", "receipts_v1", "claimed_kis_v1", "issued_cards", "donation_receipted", "worn_persona"];

/// A table value as the phone keeps it: structures as JSON text.
fn as_phone(v: &Value) -> Value {
    match v {
        Value::Array(_) | Value::Object(_) => Value::from(v.to_string()),
        other => other.clone(),
    }
}

/// A raw value from a bundle as this desk keeps it: JSON text parsed.
fn from_phone(v: Value) -> Value {
    match v {
        Value::String(s) => serde_json::from_str::<Value>(&s).ok().filter(|p| p.is_array() || p.is_object()).unwrap_or(Value::String(s)),
        other => other,
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Restored {
    pub contacts: usize,
    pub personas: usize,
    pub restore_height: u64,
    pub display_name: Option<String>,
}

impl App {
    fn app_state_blob(&self) -> Vec<u8> {
        let mut o = Map::new();
        let mut kv = Map::new();
        let contacts_table = self.store(CONTACTS).view(|m| m.clone());
        for (k, v) in contacts_table.iter() {
            if backup_key(k) {
                kv.insert(k.clone(), as_phone(v));
            }
        }
        o.insert("kv".into(), Value::Object(kv));
        for k in APP_STATE_KEYS {
            if let Some(v) = contacts_table.get(k) {
                // A set of key images travels as an array; everything else as
                // the phone would have stored it.
                o.insert(k.into(), if k == "claimed_kis_v1" { v.clone() } else { as_phone(v) });
            }
        }
        if let Some(v) = contacts_table.get("contacts") {
            o.insert("contacts_raw".into(), as_phone(v));
        }
        let raw = |store: &str, key: &str| self.store(store).get::<Value>(key).map(|v| as_phone(&v));
        for (name, store, key) in [
            ("listings_raw", "ducat_listings", "listings"),
            ("publications_raw", "ducat_publications", "pubs"),
            ("subscriptions_raw", "ducat_publications", "subs"),
            ("catalogue_raw", "ducat_catalogue", "items"),
            ("groups_raw", "ducat_groups", "groups"),
            ("subcards_raw", "ducat_publications", "subcards"),
            ("recurring_raw", "ducat_recurring", "bills"),
            ("sites_raw", "ducat_sites", "sites"),
        ] {
            if let Some(v) = raw(store, key) {
                o.insert(name.into(), v);
            }
        }
        serde_json::to_vec(&Value::Object(o)).unwrap_or_default()
    }

    fn contacts_backup(&self) -> Vec<ContactBackup> {
        self.contacts()
            .into_iter()
            .map(|c| ContactBackup {
                persona: hex_to_bytes(&c.persona_hex).unwrap_or_default(),
                my_outbox_key: c.my_outbox,
                my_outbox_owner_public: c.my_outbox_owner_public,
                my_outbox_owner_secret: c.my_outbox_owner_secret,
                their_outbox_key: c.their_outbox,
                their_bundle: c.their_bundle,
                their_payto: c.their_address,
                petname: c.petname,
                asserted_name: c.asserted_name,
                in_seq: c.in_seq,
                out_seq: c.out_seq,
                in_prev: c.in_prev_link,
                out_prev: c.out_prev_link,
                owner: Some(c.owner).filter(|o| !o.is_empty()),
            })
            .collect()
    }

    fn personas_backup(&self) -> Result<Vec<PersonaBackup>, Error> {
        let mut out = Vec::new();
        for p in self.personas()? {
            let Some(secret) = self.persona_secret(&p.hex)? else { continue };
            let f = |k: &str| self.profile_field(&p.hex, k);
            out.push(PersonaBackup {
                secret,
                name: Some(p.name.clone()).filter(|n| !n.is_empty()),
                color: (p.color as u64) & 0xFFFF_FFFF,
                created: p.created_at,
                display_name: self.my_name(Some(&p.hex))?,
                avatar: f("avatar").and_then(|s| unb64(&s)),
                email: f("email"),
                phone: f("phone"),
                signal: f("signal"),
                pronouns: f("pronouns").and_then(|s| s.parse().ok()),
                car_model: f("car_model"),
                car_color: f("car_color"),
                plate: f("plate"),
                share_profile: self.share_profile(&p.hex),
            });
        }
        Ok(out)
    }

    /// The bundle, sealed under `passphrase` (eight characters at least).
    pub fn export_backup_bytes(&self, passphrase: &str) -> Result<Vec<u8>, Error> {
        let spend = self.spend_key_hex().ok_or_else(|| Error::Refused("no wallet to back up yet — it is minted once a node answers".into()))?;
        let primary = self.primary_hex()?;
        let prekeys = self.prekeys_for_backup();
        let input = BackupInput {
            spend_key_hex: spend,
            restore_height: self.restore_height(),
            display_name: self.my_name(Some(&primary))?,
            publish_payto: self.publish_address(),
            profile: self.profile_wire(&primary, Some("profile"), true),
            contacts: self.contacts_backup(),
            prekey_signed_secret: prekeys.0,
            prekey_one_time: prekeys.1,
            prekey_next_id: prekeys.2,
            app_state: Some(self.app_state_blob()),
            escrow_shares: Vec::new(),
            personas: self.personas_backup()?,
        };
        let bytes = ducat_mobile::export_backup(input, passphrase.to_string(), self.primary_secret()?)?;
        let _ = self.store(CONTACTS).put("backup_exported_at", &App::now());
        bump();
        Ok(bytes)
    }

    pub fn export_backup_to(&self, path: &Path, passphrase: &str) -> Result<u64, Error> {
        let bytes = self.export_backup_bytes(passphrase)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, &bytes)?;
        log::info(TAG, format!("exported {} bytes to {}", bytes.len(), path.display()));
        Ok(bytes.len() as u64)
    }

    pub fn backup_exported_at(&self) -> u64 {
        self.store(CONTACTS).get("backup_exported_at").unwrap_or(0)
    }

    fn prekeys_for_backup(&self) -> (Option<Vec<u8>>, Vec<PrekeyEntry>, u64) {
        let p: Value = self.store(CONTACTS).get("prekeys").unwrap_or(Value::Null);
        let signed = p.get("signed").and_then(Value::as_str).and_then(unb64);
        let one_time: Vec<PrekeyEntry> = p
            .get("one_time")
            .and_then(Value::as_object)
            .map(|o| o.iter().filter_map(|(id, sk)| Some(PrekeyEntry { id: id.parse().ok()?, secret: unb64(sk.as_str()?)? })).collect())
            .unwrap_or_default();
        let next: u64 = self.store(CONTACTS).get("prekey_next_id").unwrap_or(1);
        (signed, one_time, next)
    }

    /// Restore from a bundle. Everything the bundle carries replaces what
    /// is here; the wallet rescans from the bundle's height; every thread
    /// is marked for a bundle republish, because the network moved on
    /// while this state stood still.
    pub fn import_backup_from(&self, path: &Path, passphrase: &str) -> Result<Restored, Error> {
        let blob = std::fs::read(path)?;
        let r = ducat_mobile::import_backup(blob, passphrase.to_string())?;
        self.restore(r)
    }

    fn restore(&self, r: RestoredBackup) -> Result<Restored, Error> {
        // The roster first: contacts are owned by personas.
        if r.personas.is_empty() {
            let hex = ducat_mobile::contacts::persona_public_hex(r.persona_secret.clone())?;
            self.store(CONTACTS).update(|m| {
                m.insert("personas".into(), json!([{ "secret": crate::contacts::b64(&r.persona_secret), "name": "", "color": 0, "created": App::now() }]));
                m.insert("persona_secret".into(), Value::from(crate::contacts::b64(&r.persona_secret)));
            })?;
            log::info(TAG, format!("restored the primary persona {}…", &hex[..12]));
        } else {
            let entries: Vec<Value> = r
                .personas
                .iter()
                .map(|p| json!({ "secret": crate::contacts::b64(&p.secret), "name": p.name.clone().unwrap_or_default(), "color": p.color as i64, "created": p.created }))
                .collect();
            self.store(CONTACTS).update(|m| {
                m.insert("personas".into(), Value::Array(entries));
                m.insert("persona_secret".into(), Value::from(crate::contacts::b64(&r.personas[0].secret)));
            })?;
            for p in &r.personas {
                let hex = ducat_mobile::contacts::persona_public_hex(p.secret.clone())?;
                if let Some(n) = &p.display_name {
                    self.set_my_name(Some(&hex), n)?;
                }
                for (k, v) in [("email", &p.email), ("phone", &p.phone), ("signal", &p.signal), ("car_model", &p.car_model), ("car_color", &p.car_color), ("plate", &p.plate)] {
                    self.set_profile_field(&hex, k, v.as_deref())?;
                }
                if let Some(a) = &p.avatar {
                    self.set_profile_field(&hex, "avatar", Some(&crate::contacts::b64(a)))?;
                }
                if let Some(pr) = p.pronouns {
                    self.set_profile_field(&hex, "pronouns", Some(&pr.to_string()))?;
                }
                self.set_share_profile(&hex, p.share_profile)?;
            }
        }
        let primary = self.primary_hex()?;
        if let Some(n) = &r.display_name {
            self.set_my_name(Some(&primary), n)?;
        }
        self.store(CONTACTS).put("publish_address", &r.publish_payto)?;
        // The wallet: the spend key and where to scan from.
        let address = ducat_mobile::address_for_spend_key(r.spend_key_hex.clone(), true)?;
        self.wallet_save(&address, &r.spend_key_hex, r.restore_height, true)?;
        self.rescan_from(r.restore_height)?;
        // App state: the tables the bundle carried, converted to how this
        // desk keeps them.
        if let Some(blob) = &r.app_state {
            if let Ok(Value::Object(o)) = serde_json::from_slice::<Value>(blob) {
                self.store(CONTACTS).update(|m| {
                    if let Some(Value::Object(kv)) = o.get("kv") {
                        let mut refused = 0;
                        for (k, v) in kv {
                            if !backup_key(k) {
                                refused += 1;
                                continue;
                            }
                            m.insert(k.clone(), from_phone(v.clone()));
                        }
                        if refused > 0 {
                            log::warn(TAG, format!("refused {refused} key(s) a backup should not carry"));
                        }
                    }
                    if let Some(v) = o.get("contacts_raw") {
                        m.insert("contacts".into(), from_phone(v.clone()));
                    }
                    for k in APP_STATE_KEYS {
                        if let Some(v) = o.get(k) {
                            m.insert(k.into(), from_phone(v.clone()));
                        }
                    }
                })?;
                for (name, store, key) in [
                    ("listings_raw", "ducat_listings", "listings"),
                    ("publications_raw", "ducat_publications", "pubs"),
                    ("subscriptions_raw", "ducat_publications", "subs"),
                    ("catalogue_raw", "ducat_catalogue", "items"),
                    ("groups_raw", "ducat_groups", "groups"),
                    ("subcards_raw", "ducat_publications", "subcards"),
                    ("recurring_raw", "ducat_recurring", "bills"),
                    ("sites_raw", "ducat_sites", "sites"),
                ] {
                    if let Some(v) = o.get(name) {
                        self.store(store).put(key, &from_phone(v.clone()))?;
                    }
                }
            } else {
                log::warn(TAG, "app state in the bundle was not readable");
            }
        }
        // Contacts: the bundle's counters over whatever is here.
        for c in &r.contacts {
            let persona_hex = hex(&c.persona);
            let existing = self.contact(&persona_hex);
            let built = Contact {
                petname: c.petname.clone().or_else(|| existing.as_ref().and_then(|e| e.petname.clone())),
                asserted_name: c.asserted_name.clone().or_else(|| existing.as_ref().and_then(|e| e.asserted_name.clone())),
                my_outbox: c.my_outbox_key.clone(),
                my_outbox_owner_public: c.my_outbox_owner_public.clone(),
                my_outbox_owner_secret: c.my_outbox_owner_secret.clone(),
                their_outbox: c.their_outbox_key.clone(),
                their_bundle: c.their_bundle.clone(),
                their_address: c.their_payto.clone(),
                in_seq: c.in_seq,
                out_seq: c.out_seq,
                in_prev_link: c.in_prev.clone(),
                out_prev_link: c.out_prev.clone(),
                owner: c.owner.clone().or_else(|| existing.as_ref().map(|e| e.owner.clone())).unwrap_or_default(),
                ..existing.clone().unwrap_or_else(|| Contact {
                    persona_hex: persona_hex.clone(),
                    petname: None,
                    asserted_name: None,
                    my_outbox: String::new(),
                    my_outbox_owner_public: Vec::new(),
                    my_outbox_owner_secret: Vec::new(),
                    their_outbox: String::new(),
                    their_bundle: None,
                    their_address: None,
                    pending_address: None,
                    avatar: None,
                    email: None,
                    phone: None,
                    signal: None,
                    pronouns: None,
                    my_ring: 8,
                    car_model: None,
                    car_color: None,
                    plate: None,
                    their_read_up_to: None,
                    card_purpose: None,
                    my_card_purpose: None,
                    my_card_purpose_at: 0,
                    out_seq: 0,
                    out_prev_link: None,
                    in_seq: 0,
                    in_prev_link: None,
                    chat_visible: true,
                    owner: String::new(),
                })
            };
            self.put_contact(built)?;
        }
        // Prekeys: the bundle's secrets beside whatever bundle is
        // advertised; the counter only ever moves up.
        let one_time: Vec<(u32, Vec<u8>)> = r.prekey_one_time.iter().map(|e| (e.id as u32, e.secret.clone())).collect();
        if r.prekey_signed_secret.is_some() || !one_time.is_empty() {
            self.save_prekeys(&[], r.prekey_signed_secret.as_deref().unwrap_or(&[]), &one_time, false)?;
        }
        let next: u64 = self.store(CONTACTS).get("prekey_next_id").unwrap_or(1);
        if r.prekey_next_id > next {
            self.store(CONTACTS).put("prekey_next_id", &r.prekey_next_id)?;
        }
        // The network moved on while this state stood still.
        self.set_bundles_need_republish(true)?;
        if r.escrow_count > 0 {
            log::warn(TAG, format!("the bundle carried {} escrow share(s) — ceremonies are not on the desk yet, so they were not restored", r.escrow_count));
        }
        bump();
        log::info(TAG, format!("restored {} contact(s), {} persona(s), wallet from height {}", r.contacts.len(), r.personas.len().max(1), r.restore_height));
        Ok(Restored { contacts: r.contacts.len(), personas: r.personas.len().max(1), restore_height: r.restore_height, display_name: r.display_name })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bundle_round_trips_between_two_desks() {
        let base = std::env::temp_dir().join(format!("ducat-backup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let a = App::open(base.join("a")).unwrap();
        // A real key: the bundle derives the address from it, and a
        // made-up scalar is refused as malformed.
        let w = ducat_mobile::create_wallet(2_200_000, true);
        a.wallet_save(&w.address, &w.spend_key_hex, 2_200_000, true).unwrap();
        a.set_my_name(None, "Kara").unwrap();
        let me = a.primary_hex().unwrap();
        a.set_profile_field(&me, "email", Some("k@example.org")).unwrap();
        a.set_share_profile(&me, true).unwrap();
        let shop = a.create_persona("Shop", 7).unwrap().unwrap();
        a.put_contact(Contact {
            persona_hex: "cd".repeat(32),
            petname: Some("Pat".into()),
            asserted_name: None,
            my_outbox: "VLD0:mine".into(),
            my_outbox_owner_public: vec![1; 32],
            my_outbox_owner_secret: vec![2; 32],
            their_outbox: "VLD0:theirs".into(),
            their_bundle: Some(vec![9; 40]),
            their_address: Some("4pay".into()),
            pending_address: None,
            avatar: None,
            email: None,
            phone: None,
            signal: None,
            pronouns: None,
            my_ring: 32,
            car_model: None,
            car_color: None,
            plate: None,
            their_read_up_to: None,
            card_purpose: None,
            my_card_purpose: None,
            my_card_purpose_at: 0,
            out_seq: 3,
            out_prev_link: Some(vec![3; 32]),
            in_seq: 2,
            in_prev_link: Some(vec![4; 32]),
            chat_visible: true,
            owner: me.clone(),
        })
        .unwrap();
        let row = crate::contacts::StoredMessage { outgoing: false, seq: 0, body: "hello".into(), timestamp: 5, ..Default::default() };
        a.append_and_advance(&"cd".repeat(32), row, 1, None).unwrap();
        a.save_prekeys(b"bundle", &[7; 32], &[(5, vec![5; 32])], false).unwrap();
        let t = a.open_or_resume_tab(&"cd".repeat(32), crate::tabs::ORIGIN_BAR).unwrap();
        let id = a.create_publication("Zine").unwrap();
        let bytes = a.export_backup_bytes("correct horse battery").unwrap();
        assert!(bytes.len() > 200);
        let path = base.join("bundle.ducat");
        std::fs::write(&path, &bytes).unwrap();

        let b = App::open(base.join("b")).unwrap();
        let r = b.import_backup_from(&path, "correct horse battery").unwrap();
        assert_eq!(r.contacts, 1);
        assert_eq!(r.personas, 2);
        assert_eq!(b.primary_hex().unwrap(), me);
        assert_eq!(b.personas().unwrap()[1].hex, shop.hex);
        assert_eq!(b.my_name(Some(&me)).unwrap().as_deref(), Some("Kara"));
        assert_eq!(b.profile_field(&me, "email").as_deref(), Some("k@example.org"));
        assert_eq!(b.spend_key_hex().as_deref(), Some(w.spend_key_hex.as_str()));
        assert_eq!(b.wallet_address().as_deref(), Some(w.address.as_str()));
        assert_eq!(b.restore_height(), 2_200_000);
        let c = b.contact(&"cd".repeat(32)).unwrap();
        assert_eq!(c.petname.as_deref(), Some("Pat"));
        assert_eq!(c.out_seq, 3);
        assert_eq!(c.in_seq, 1);
        assert_eq!(c.their_bundle, Some(vec![9; 40]));
        assert_eq!(b.thread(&"cd".repeat(32)).len(), 1);
        assert_eq!(b.one_time_secret(5), Some(vec![5; 32]));
        assert_eq!(b.tab(&t.id).unwrap().persona_hex, "cd".repeat(32));
        assert_eq!(b.publication(&id).unwrap().title, "Zine");
        assert!(b.bundles_need_republish());
        assert!(b.import_backup_from(&path, "wrong passphrase").is_err());
    }
}
