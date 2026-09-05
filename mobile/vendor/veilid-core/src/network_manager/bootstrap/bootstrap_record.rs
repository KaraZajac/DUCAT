use super::*;

impl_veilid_log_facility!("net");

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BootstrapRecord {
    public_keys: PublicKeyGroup,
    envelope_support: Vec<EnvelopeVersion>,
    dial_info_details: Vec<DialInfoDetail>,
    timestamp_secs: Option<u64>,
    extra: Vec<String>,
}

impl BootstrapRecord {
    pub fn new(
        public_keys: PublicKeyGroup,
        mut envelope_support: Vec<EnvelopeVersion>,
        mut dial_info_details: Vec<DialInfoDetail>,
        timestamp_secs: Option<u64>,
        extra: Vec<String>,
    ) -> Self {
        envelope_support.sort_unstable();
        dial_info_details.sort_unstable();

        Self {
            public_keys,
            envelope_support,
            dial_info_details,
            timestamp_secs,
            extra,
        }
    }

    pub fn public_keys(&self) -> &PublicKeyGroup {
        &self.public_keys
    }
    pub fn envelope_support(&self) -> &[EnvelopeVersion] {
        &self.envelope_support
    }
    pub fn dial_info_details(&self) -> &[DialInfoDetail] {
        &self.dial_info_details
    }
    pub fn timestamp_secs(&self) -> Option<u64> {
        self.timestamp_secs
    }
    #[expect(dead_code)]
    pub fn extra(&self) -> &[String] {
        &self.extra
    }

    pub fn merge(&mut self, other: BootstrapRecord) {
        self.public_keys.add_all_from_iter(other.public_keys.iter());
        for x in other.envelope_support {
            if !self.envelope_support.contains(&x) {
                self.envelope_support.push(x);
                self.envelope_support.sort_unstable();
            }
        }
        for did in other.dial_info_details {
            if !self.dial_info_details.contains(&did) {
                self.dial_info_details.push(did);
            }
        }
        self.dial_info_details.sort_unstable();
        if let Some(ts) = self.timestamp_secs.as_mut() {
            if let Some(other_ts) = other.timestamp_secs {
                // Use earliest timestamp if merging
                ts.min_assign(other_ts);
            } else {
                // Do nothing
            }
        } else {
            self.timestamp_secs = other.timestamp_secs;
        }
        self.extra.extend_from_slice(&other.extra);
    }

    async fn to_vcommon_string(
        &self,
        dial_info_converter: &dyn DialInfoConverter,
    ) -> EyreResult<String> {
        let valid_envelope_versions = self
            .envelope_support()
            .iter()
            .map(|x| {
                if x.bytes()[0..3] == *b"ENV" {
                    x.to_string().split_off(3)
                } else {
                    x.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(",");

        let public_keys = self
            .public_keys
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(",");

        let mut short_urls = Vec::new();
        let mut some_hostname = Option::<String>::None;
        for did in self.dial_info_details() {
            let ShortDialInfo {
                short_url,
                hostname,
            } = dial_info_converter.to_short(did.dial_info.clone()).await;
            if let Some(h) = &some_hostname {
                if h != &hostname {
                    bail!(
                        "Inconsistent hostnames for dial info: {} vs {}",
                        some_hostname.unwrap_or_log(),
                        hostname
                    );
                }
            } else {
                some_hostname = Some(hostname);
            }

            short_urls.push(short_url);
        }
        if some_hostname.is_none() || short_urls.is_empty() {
            bail!("No dial info for bootstrap host");
        }
        short_urls.sort_unstable();
        short_urls.dedup();

        let vcommon = format!(
            "|{}|{}|{}|{}",
            valid_envelope_versions,
            public_keys,
            some_hostname.as_ref().unwrap_or_log(),
            short_urls.join(",")
        );

        Ok(vcommon)
    }

    pub async fn to_v0_string(
        &self,
        dial_info_converter: &dyn DialInfoConverter,
    ) -> EyreResult<String> {
        let vcommon = self.to_vcommon_string(dial_info_converter).await?;
        Ok(format!("{}{}", BOOTSTRAP_TXT_VERSION_0, vcommon))
    }

    pub async fn to_v1_string(
        &self,
        network_manager: &NetworkManager,
        dial_info_converter: &dyn DialInfoConverter,
        signing_key_pair: KeyPair,
    ) -> EyreResult<String> {
        let vcommon = self.to_vcommon_string(dial_info_converter).await?;
        let ts = if let Some(ts) = self.timestamp_secs() {
            ts
        } else {
            bail!("timestamp required for bootstrap v1 format");
        };
        let mut v1 = format!("{}{}|{}|", BOOTSTRAP_TXT_VERSION_1, vcommon, ts);

        let crypto = network_manager.crypto();

        let sig =
            match crypto.generate_signatures(v1.as_bytes(), &[signing_key_pair], |_kp, sig| {
                sig.to_string()
            }) {
                Ok(v) => {
                    let Some(sig) = v.first().cloned() else {
                        bail!("No signature generated");
                    };
                    sig
                }
                Err(e) => {
                    bail!("Failed to generate signature: {}", e);
                }
            };

        v1 += &sig;
        Ok(v1)
    }

    pub fn new_from_v0_str(
        network_manager: &NetworkManager,
        dial_info_converter: &dyn DialInfoConverter,
        record_str: &str,
    ) -> EyreResult<Option<BootstrapRecord>> {
        // All formats split on '|' character
        let fields: Vec<String> = record_str
            .trim()
            .split('|')
            .map(|x| x.trim().to_owned())
            .collect();

        // Bootstrap TXT record version
        let txt_version: u8 = match fields[0].parse::<u8>() {
            Ok(v) => v,
            Err(e) => {
                bail!(
                    "invalid txt_version specified in bootstrap node txt record: {}",
                    e
                );
            }
        };
        let bootstrap_record = match txt_version {
            BOOTSTRAP_TXT_VERSION_0 => {
                match Self::process_bootstrap_fields_v0(
                    network_manager,
                    dial_info_converter,
                    &fields,
                ) {
                    Err(e) => {
                        bail!(
                            "couldn't process v0 bootstrap records from {:?}: {}",
                            fields,
                            e
                        );
                    }
                    Ok(Some(v)) => v,
                    Ok(None) => {
                        // skipping
                        return Ok(None);
                    }
                }
            }
            _ => return Ok(None),
        };

        Ok(Some(bootstrap_record))
    }

    pub fn new_from_v1_str(
        network_manager: &NetworkManager,
        dial_info_converter: &dyn DialInfoConverter,
        record_str: &str,
        signing_keys: &[PublicKey],
    ) -> EyreResult<Option<BootstrapRecord>> {
        // All formats split on '|' character
        let fields: Vec<String> = record_str
            .trim()
            .split('|')
            .map(|x| x.trim().to_owned())
            .collect();

        // Bootstrap TXT record version
        let txt_version: u8 = match fields[0].parse::<u8>() {
            Ok(v) => v,
            Err(e) => {
                bail!(
                    "invalid txt_version specified in bootstrap node txt record: {}",
                    e
                );
            }
        };
        let bootstrap_record = match txt_version {
            BOOTSTRAP_TXT_VERSION_1 => {
                match Self::process_bootstrap_fields_v1(
                    network_manager,
                    dial_info_converter,
                    record_str,
                    &fields,
                    signing_keys,
                ) {
                    Err(e) => {
                        bail!(
                            "couldn't process v1 bootstrap records from {:?}: {}",
                            fields,
                            e
                        );
                    }
                    Ok(Some(v)) => v,
                    Ok(None) => {
                        // skipping
                        return Ok(None);
                    }
                }
            }
            _ => return Ok(None),
        };

        Ok(Some(bootstrap_record))
    }

    /// Process bootstrap version 0
    ///
    /// Bootstrap TXT Record Format Version 0:
    /// txt_version|envelope_support|node_ids|hostname|dialinfoshort*
    ///
    /// Split bootstrap node record by '|' and then lists by ','. Example:
    /// 0|0|VLD0:7lxDEabK_qgjbe38RtBa3IZLrud84P6NhGP-pRTZzdQ|bootstrap-1.dev.veilid.net|T5150,U5150,W5150/ws
    fn process_bootstrap_fields_v0(
        network_manager: &NetworkManager,
        dial_info_converter: &dyn DialInfoConverter,
        fields: &[String],
    ) -> EyreResult<Option<BootstrapRecord>> {
        if fields.len() != 5 {
            bail!("invalid number of fields in bootstrap v0 txt record");
        }

        // Envelope support
        let mut envelope_support = Vec::new();
        for ess in fields[1].split(',') {
            let ess = ess.trim();

            let es = if ess.len() == 1 {
                let mut envb: [u8; 4] = *b"ENV0";
                envb[3] = ess.as_bytes()[0];
                EnvelopeVersion::from(envb)
            } else if ess.len() == 4 {
                match ess.parse::<EnvelopeVersion>() {
                    Ok(v) => v,
                    Err(e) => {
                        bail!(
                            "invalid envelope version fourcc specified in bootstrap v0 node txt record: {}",
                            e
                        );
                    }
                }
            } else {
                bail!(
                    "invalid envelope version length specified in bootstrap v0 node txt record: {}",
                    ess
                );
            };
            envelope_support.push(es);
        }
        envelope_support.sort_unstable();
        envelope_support.dedup();

        // Node Id
        let mut public_keys = PublicKeyGroup::new();
        for public_key_str in fields[2].split(',') {
            let public_key_str = public_key_str.trim();
            let public_key = match PublicKey::from_str(public_key_str) {
                Ok(v) => v,
                Err(e) => {
                    bail!(
                        "Invalid public key in bootstrap node record {}: {}",
                        public_key_str,
                        e
                    );
                }
            };
            public_keys.add(public_key);
        }

        // Hostname
        let hostname_str = fields[3].trim();

        // Resolve each record and store in node dial infos list
        let mut dial_info_details = Vec::new();
        for rec in fields[4].split(',') {
            let rec = rec.trim();
            let short_dial_info = ShortDialInfo {
                short_url: rec.to_string(),
                hostname: hostname_str.to_string(),
            };
            let dial_infos = match dial_info_converter.try_vec_from_short(&short_dial_info) {
                Ok(dis) => dis,
                Err(e) => {
                    veilid_log!(network_manager debug "Couldn't resolve bootstrap node dial info {}: {}", rec, e);
                    continue;
                }
            };

            for di in dial_infos {
                dial_info_details.push(DialInfoDetail {
                    dial_info: di,
                    class: DialInfoClass::Direct,
                });
            }
        }

        // If no dial info could resolve don't consider this a successful bootstrap
        if dial_info_details.is_empty() {
            return Ok(None);
        }

        Ok(Some(BootstrapRecord::new(
            public_keys,
            envelope_support,
            dial_info_details,
            None,
            vec![],
        )))
    }

    /// Process bootstrap version 1
    ///
    /// Bootstrap TXT Record Format Version 1:
    /// txt_version|envelope_support|node_ids|hostname|dialinfoshort*|timestamp|extra..|....| typedsignature
    ///
    /// Split bootstrap node record by '|' and then lists by ','. Example:
    /// 1|0|VLD0:7lxDEabK_qgjbe38RtBa3IZLrud84P6NhGP-pRTZzdQ|bootstrap-1.dev.veilid.net|T5150,U5150,W5150/ws|1746308366
    /// timestamp is a uint64 number of seconds since epoch (unix time64)
    /// extra is any extra data to be covered by the signature, any number of extra '|' fields
    /// the signature is over all of the byte data in the string that precedes the signature itself, including all delimeters and/or whitespace
    fn process_bootstrap_fields_v1(
        network_manager: &NetworkManager,
        dial_info_converter: &dyn DialInfoConverter,
        record_str: &str,
        fields: &[String],
        signing_keys: &[PublicKey],
    ) -> EyreResult<Option<BootstrapRecord>> {
        if fields.len() < 7 {
            bail!("invalid number of fields in bootstrap v1 txt record");
        }

        // Get signature from last record
        let sigstring = fields.last().unwrap_or_log();
        let sig =
            Signature::from_str(sigstring).wrap_err("invalid signature for bootstrap v1 record")?;

        // Get slice that was signed
        let signed_str = &record_str[0..record_str.len() - sigstring.len()];

        // Validate signature against any signing keys if we have them
        if !signing_keys.is_empty() {
            let mut validated = false;
            for key in signing_keys {
                if let Some(valid_keys) = network_manager.crypto().verify_signatures(
                    std::slice::from_ref(key),
                    signed_str.as_bytes(),
                    std::slice::from_ref(&sig),
                )? {
                    if valid_keys.contains(key) {
                        validated = true;
                        break;
                    }
                }
            }
            if !validated {
                bail!(
                    "bootstrap record did not have valid signature: {}",
                    record_str
                );
            }
        }

        // Envelope support
        let mut envelope_support = Vec::new();
        for ess in fields[1].split(',') {
            let ess = ess.trim();

            let es = if ess.len() == 1 {
                let mut envb: [u8; 4] = *b"ENV0";
                envb[3] = ess.as_bytes()[0];
                EnvelopeVersion::from(envb)
            } else if ess.len() == 4 {
                match ess.parse::<EnvelopeVersion>() {
                    Ok(v) => v,
                    Err(e) => {
                        bail!(
                            "invalid envelope version fourcc specified in bootstrap v1 node txt record: {}",
                            e
                        );
                    }
                }
            } else {
                bail!(
                    "invalid envelope version length specified in bootstrap v1 node txt record: {}",
                    ess
                );
            };
            envelope_support.push(es);
        }
        envelope_support.sort_unstable();
        envelope_support.dedup();

        // Public Keys
        let mut public_keys = PublicKeyGroup::new();
        for public_key_str in fields[2].split(',') {
            let public_key_str = public_key_str.trim();
            let public_key = match PublicKey::from_str(public_key_str) {
                Ok(v) => v,
                Err(e) => {
                    bail!(
                        "Invalid public key in bootstrap node record {}: {}",
                        public_key_str,
                        e
                    );
                }
            };
            public_keys.add(public_key);
        }

        // Hostname
        let hostname_str = fields[3].trim();

        // DialInfos
        let mut dial_info_details = Vec::new();
        for rec in fields[4].split(',') {
            let rec = rec.trim();
            let short_dial_info = ShortDialInfo {
                short_url: rec.to_string(),
                hostname: hostname_str.to_string(),
            };
            let dial_infos = match dial_info_converter.try_vec_from_short(&short_dial_info) {
                Ok(dis) => dis,
                Err(e) => {
                    veilid_log!(network_manager debug "Couldn't resolve bootstrap node dial info {}: {}", rec, e);
                    continue;
                }
            };

            for di in dial_infos {
                dial_info_details.push(DialInfoDetail {
                    dial_info: di,
                    class: DialInfoClass::Direct,
                });
            }
        }

        // If no dial info could resolve don't consider this a successful bootstrap
        if dial_info_details.is_empty() {
            return Ok(None);
        }

        // Timestamp
        let secs_u64 = u64::from_str(&fields[5]).wrap_err("invalid timestamp")?;

        // Extra fields
        let extra = fields[6..fields.len() - 1].to_vec();

        Ok(Some(BootstrapRecord::new(
            public_keys,
            envelope_support,
            dial_info_details,
            Some(secs_u64),
            extra,
        )))
    }
}
