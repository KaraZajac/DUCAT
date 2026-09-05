//! Who this desk is: the persona roster, the name each persona goes by,
//! and the profile it publishes — the phone's `PersonaStore`, `NameStore`
//! and `MyProfile`, kept in the contacts table under the same keys.
//!
//! The rules, stated where the code enforces them:
//! - **Few by construction.** [`MAX_PERSONAS`] is small because
//!   compartments only work when they fit on one hand.
//! - **No deletion.** A persona's contacts are bound to it at their
//!   doorway and cannot be re-homed; the roster only grows.
//! - **The primary is entry zero, for ever.** The legacy `persona_secret`
//!   key is kept in step with it so a backup finds the identity where it
//!   always was.

use std::collections::HashSet;
use std::sync::Mutex;

use ducat_mobile::contacts::{clean_display_text, persona_public_hex, Profile};
use serde::{Deserialize, Serialize};

use crate::contacts::{b64, bump, unb64, Contact, CONTACTS};
use crate::{log, App, Error};

const TAG: &str = "Identity";

/// Compartments that fit on one hand.
pub const MAX_PERSONAS: usize = 4;

#[derive(Clone, Debug, Serialize)]
pub struct Persona {
    pub hex: String,
    pub name: String,
    /// ARGB accent the bar is tinted with; 0 means the theme default.
    pub color: i64,
    pub created_at: u64,
    pub primary: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    secret: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    color: i64,
    #[serde(default)]
    created: u64,
}

/// Minting is once per lifetime; two threads touching an empty roster at
/// once must not each mint an identity.
static ROSTER: Mutex<()> = Mutex::new(());

impl App {
    /// The roster, migrating the single-persona era on first touch and
    /// minting on the very first. An empty stored roster is not a roster;
    /// a roster that fails to parse is refused rather than replaced —
    /// minting over a corrupt one would orphan every contact this desk has.
    fn roster(&self) -> Result<Vec<(Vec<u8>, Persona)>, Error> {
        let _g = ROSTER.lock().unwrap_or_else(|e| e.into_inner());
        let store = self.store(CONTACTS);
        let entries: Vec<Entry> = store.get("personas").unwrap_or_default();
        if !entries.is_empty() {
            return entries
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    let secret = unb64(&e.secret).ok_or_else(|| Error::Refused("persona roster is not readable".into()))?;
                    let hex = persona_public_hex(secret.clone())?;
                    Ok((secret, Persona { hex, name: e.name.clone(), color: e.color, created_at: e.created, primary: i == 0 }))
                })
                .collect();
        }
        let secret = store
            .get_string("persona_secret")
            .and_then(|s| unb64(&s))
            .unwrap_or_else(ducat_mobile::create_persona_secret);
        let hex = persona_public_hex(secret.clone())?;
        let entry = Entry { secret: b64(&secret), name: String::new(), color: 0, created: App::now() };
        store.put("personas", &vec![entry.clone()])?;
        store.put("persona_secret", &entry.secret)?;
        log::info(TAG, format!("minted this desk's identity: {}…", &hex[..12]));
        Ok(vec![(secret, Persona { hex, name: String::new(), color: 0, created_at: entry.created, primary: true })])
    }

    fn write_roster(&self, entries: &[(Vec<u8>, Persona)]) -> Result<(), Error> {
        let store = self.store(CONTACTS);
        let list: Vec<Entry> = entries
            .iter()
            .map(|(s, p)| Entry { secret: b64(s), name: p.name.clone(), color: p.color, created: p.created_at })
            .collect();
        store.put("personas", &list)?;
        if let Some((s, _)) = entries.first() {
            store.put("persona_secret", &b64(s))?;
        }
        bump();
        Ok(())
    }

    pub fn personas(&self) -> Result<Vec<Persona>, Error> {
        Ok(self.roster()?.into_iter().map(|(_, p)| p).collect())
    }

    pub fn persona_hexes(&self) -> HashSet<String> {
        self.personas().map(|ps| ps.into_iter().map(|p| p.hex).collect()).unwrap_or_default()
    }

    /// The primary persona's secret — entry zero, minted on first call.
    pub fn primary_secret(&self) -> Result<Vec<u8>, Error> {
        Ok(self.roster()?.remove(0).0)
    }

    /// The primary persona, in the hex form contacts are keyed by.
    pub fn primary_hex(&self) -> Result<String, Error> {
        Ok(self.roster()?.remove(0).1.hex)
    }

    pub fn persona_secret(&self, hex: &str) -> Result<Option<Vec<u8>>, Error> {
        Ok(self.roster()?.into_iter().find(|(_, p)| p.hex == hex).map(|(s, _)| s))
    }

    /// Mint a new compartment, or None at the cap.
    pub fn create_persona(&self, name: &str, color: i64) -> Result<Option<Persona>, Error> {
        let mut entries = self.roster()?;
        if entries.len() >= MAX_PERSONAS {
            return Ok(None);
        }
        let secret = ducat_mobile::create_persona_secret();
        let p = Persona {
            hex: persona_public_hex(secret.clone())?,
            name: clean_display_text(name.to_string()),
            color,
            created_at: App::now(),
            primary: false,
        };
        entries.push((secret, p.clone()));
        self.write_roster(&entries)?;
        Ok(Some(p))
    }

    pub fn rename_persona(&self, hex: &str, name: &str) -> Result<(), Error> {
        let mut entries = self.roster()?;
        if let Some((_, p)) = entries.iter_mut().find(|(_, p)| p.hex == hex) {
            p.name = clean_display_text(name.to_string());
        }
        self.write_roster(&entries)
    }

    pub fn set_persona_color(&self, hex: &str, color: i64) -> Result<(), Error> {
        let mut entries = self.roster()?;
        if let Some((_, p)) = entries.iter_mut().find(|(_, p)| p.hex == hex) {
            p.color = color;
        }
        self.write_roster(&entries)
    }

    /// The persona currently worn — whose card the QR hub shows and whose
    /// name a new thread is answered under. The primary unless changed.
    pub fn worn(&self) -> Result<String, Error> {
        let hexes = self.persona_hexes();
        Ok(self
            .store(CONTACTS)
            .get_string("worn_persona")
            .filter(|h| hexes.contains(h))
            .unwrap_or(self.primary_hex()?))
    }

    pub fn wear(&self, hex: &str) -> Result<(), Error> {
        if !self.persona_hexes().contains(hex) {
            return Err(Error::Refused("no such persona".into()));
        }
        self.store(CONTACTS).put("worn_persona", &hex)?;
        bump();
        Ok(())
    }

    /// Which of our personas a contact belongs to: what the record says,
    /// if that persona still exists, else the primary — the single-persona
    /// era's records carry no owner and were all the primary's.
    pub fn owner_hex_of(&self, c: &Contact) -> String {
        if !c.owner.is_empty() && self.persona_hexes().contains(&c.owner) {
            return c.owner.clone();
        }
        self.primary_hex().unwrap_or_default()
    }

    /// The secret that speaks for a contact's owner.
    pub fn owner_secret_of(&self, c: &Contact) -> Result<Vec<u8>, Error> {
        let hex = self.owner_hex_of(c);
        self.persona_secret(&hex)?.ok_or_else(|| Error::Refused("this thread's persona is gone".into()))
    }

    fn name_key(&self, persona_hex: Option<&str>) -> Result<String, Error> {
        let hex = match persona_hex {
            Some(h) => h.to_string(),
            None => self.worn()?,
        };
        Ok(if hex == self.primary_hex()? { "my_name".to_string() } else { format!("my_name|{hex}") })
    }

    /// The name this persona asserts on its cards and replies.
    pub fn my_name(&self, persona_hex: Option<&str>) -> Result<Option<String>, Error> {
        Ok(self.store(CONTACTS).get_string(&self.name_key(persona_hex)?).filter(|s| !s.trim().is_empty()))
    }

    pub fn set_my_name(&self, persona_hex: Option<&str>, name: &str) -> Result<(), Error> {
        let key = self.name_key(persona_hex)?;
        self.store(CONTACTS).put(&key, &clean_display_text(name.to_string()))?;
        bump();
        Ok(())
    }

    // ----- the profile (§16.9) ---------------------------------------------------

    fn profile_key(&self, persona_hex: &str, field: &str) -> String {
        format!("profile|{persona_hex}|{field}")
    }

    pub fn profile_field(&self, persona_hex: &str, field: &str) -> Option<String> {
        self.store(CONTACTS).get_string(&self.profile_key(persona_hex, field)).filter(|s| !s.is_empty())
    }

    pub fn set_profile_field(&self, persona_hex: &str, field: &str, value: Option<&str>) -> Result<(), Error> {
        let key = self.profile_key(persona_hex, field);
        match value.map(str::trim).filter(|v| !v.is_empty()) {
            Some(v) => self.store(CONTACTS).put(&key, &clean_display_text(v.to_string()))?,
            None => self.store(CONTACTS).remove(&key)?,
        }
        bump();
        Ok(())
    }

    /// §16.9: the profile is a choice. Off, the wire carries nothing.
    pub fn share_profile(&self, persona_hex: &str) -> bool {
        self.store(CONTACTS).get::<bool>(&self.profile_key(persona_hex, "share")).unwrap_or(false)
    }

    pub fn set_share_profile(&self, persona_hex: &str, v: bool) -> Result<(), Error> {
        self.store(CONTACTS).put(&self.profile_key(persona_hex, "share"), &v)?;
        bump();
        Ok(())
    }

    /// The profile as it rides a handshake, scoped to the handshake's
    /// purpose: reach-me identifiers only on a "profile" card, the car
    /// only when answering a hail as its driver.
    pub fn profile_wire(&self, persona_hex: &str, purpose: Option<&str>, driving: bool) -> Profile {
        let none = Profile {
            avatar: None,
            email: None,
            phone: None,
            signal: None,
            pronouns: None,
            car_model: None,
            car_color: None,
            plate: None,
        };
        if !self.share_profile(persona_hex) {
            return none;
        }
        let relational = purpose == Some("profile");
        let f = |k: &str| self.profile_field(persona_hex, k);
        Profile {
            avatar: f("avatar").and_then(|s| unb64(&s)),
            email: if relational { f("email") } else { None },
            phone: if relational { f("phone") } else { None },
            signal: if relational { f("signal") } else { None },
            pronouns: f("pronouns").and_then(|s| s.parse().ok()).filter(|p| (1..=6).contains(p)),
            car_model: if driving { f("car_model") } else { None },
            car_color: if driving { f("car_color") } else { None },
            plate: if driving { f("plate") } else { None },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_app(tag: &str) -> App {
        // Named per test: tests share a process, and two that started in
        // the same millisecond shared a directory — and each other's rows.
        let dir = std::env::temp_dir().join(format!("ducat-identity-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        App::open(&dir).unwrap()
    }

    #[test]
    fn the_primary_is_minted_once_and_stays_at_entry_zero() {
        let app = temp_app("the_primary_is_minted_once_and_stays_at_entry_zero");
        let first = app.primary_hex().unwrap();
        assert_eq!(app.primary_hex().unwrap(), first);
        assert_eq!(app.worn().unwrap(), first);
        let shop = app.create_persona("Shop", 0xff00ff).unwrap().unwrap();
        assert_ne!(shop.hex, first);
        assert_eq!(app.personas().unwrap()[0].hex, first);
        assert!(app.personas().unwrap()[0].primary);
        app.wear(&shop.hex).unwrap();
        assert_eq!(app.worn().unwrap(), shop.hex);
        // The legacy key follows the primary, never the worn hat.
        let legacy = app.store(CONTACTS).get_string("persona_secret").unwrap();
        assert_eq!(persona_public_hex(unb64(&legacy).unwrap()).unwrap(), first);
    }

    #[test]
    fn the_roster_is_capped_and_names_are_per_persona() {
        let app = temp_app("the_roster_is_capped_and_names_are_per_persona");
        for i in 0..(MAX_PERSONAS - 1) {
            assert!(app.create_persona(&format!("p{i}"), 0).unwrap().is_some());
        }
        assert!(app.create_persona("one too many", 0).unwrap().is_none());
        app.set_my_name(None, "Kara").unwrap();
        let shop = app.personas().unwrap()[1].hex.clone();
        assert_eq!(app.my_name(None).unwrap().as_deref(), Some("Kara"));
        assert_eq!(app.my_name(Some(&shop)).unwrap(), None);
        app.set_my_name(Some(&shop), "The Shop").unwrap();
        assert_eq!(app.my_name(Some(&shop)).unwrap().as_deref(), Some("The Shop"));
    }

    #[test]
    fn the_profile_is_off_until_chosen_and_scoped_to_the_purpose() {
        let app = temp_app("the_profile_is_off_until_chosen_and_scoped_to_the_purpose");
        let me = app.primary_hex().unwrap();
        app.set_profile_field(&me, "email", Some("k@example.org")).unwrap();
        app.set_profile_field(&me, "plate", Some("ABC 123")).unwrap();
        assert!(app.profile_wire(&me, Some("profile"), false).email.is_none());
        app.set_share_profile(&me, true).unwrap();
        assert_eq!(app.profile_wire(&me, Some("profile"), false).email.as_deref(), Some("k@example.org"));
        assert!(app.profile_wire(&me, Some("sale"), false).email.is_none());
        assert!(app.profile_wire(&me, Some("profile"), false).plate.is_none());
        assert_eq!(app.profile_wire(&me, None, true).plate.as_deref(), Some("ABC 123"));
    }
}
