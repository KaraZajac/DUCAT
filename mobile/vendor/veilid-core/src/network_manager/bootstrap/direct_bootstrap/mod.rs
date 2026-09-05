mod v0;
mod v1;

use super::*;
use v1::*;

impl_veilid_log_facility!("net");

impl NetworkManager {
    /// Direct bootstrap request
    /// Sends a bootstrap request to a dialinfo and returns the list of peers to bootstrap with
    /// If no bootstrap keys are specified, uses the v0 mechanism, otherwise uses the v1 mechanism
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "net", err, skip(self), fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn direct_bootstrap(&self, dial_info: DialInfo) -> EyreResult<Vec<Arc<PeerInfo>>> {
        let direct_boot_version = if self
            .config()
            .network
            .routing_table
            .bootstrap_keys
            .is_empty()
        {
            0
        } else {
            1
        };

        if direct_boot_version == 0 {
            self.direct_bootstrap_v0(dial_info).await
        } else {
            self.direct_bootstrap_v1(dial_info).await
        }
    }

    /// Uses the bootstrap v0 (BOOT) mechanism
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "net", err, skip(self), fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn direct_bootstrap_v0(&self, dial_info: DialInfo) -> EyreResult<Vec<Arc<PeerInfo>>> {
        let timeout_ms = self.config().internal().network.rpc.timeout_ms;
        // Send boot magic to requested peer address
        let data = Bytes::copy_from_slice(BOOT_MAGIC);

        let out_data = network_result_value_or_log!(self self
            .net()
            .send_recv_data_unbound_to_dial_info(dial_info, data, timeout_ms)
            .await? => [ format!(": dial_info={}, data.len={}", dial_info, data.len()) ]
        {
            return Ok(Vec::new());
        });

        let bootstrap_peerinfo_str =
            std::str::from_utf8(&out_data).wrap_err("bad utf8 in boot peerinfo")?;

        let bootstrap_peerinfo: Vec<PeerInfo> = match deserialize_json(bootstrap_peerinfo_str) {
            Ok(v) => v,
            Err(e) => {
                error!("{}", e);
                return Err(e).wrap_err("failed to deserialize peerinfo");
            }
        };

        Ok(bootstrap_peerinfo.into_iter().map(Arc::new).collect())
    }

    /// Uses the bootstrap v1 (B01T) mechanism
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "net", err, skip(self), fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    async fn direct_bootstrap_v1(&self, dial_info: DialInfo) -> EyreResult<Vec<Arc<PeerInfo>>> {
        let timeout_ms = self.config().internal().network.rpc.timeout_ms;

        // Send boot magic to requested peer address
        let data = Bytes::copy_from_slice(B01T_MAGIC);

        let out_data = network_result_value_or_log!(self self
            .net()
            .send_recv_data_unbound_to_dial_info(dial_info, data, timeout_ms)
            .await? => [ format!(": dial_info={}, data.len={}", dial_info, data.len()) ]
        {
            return Ok(Vec::new());
        });

        let bootv1response_str =
            std::str::from_utf8(&out_data).wrap_err("bad utf8 in bootstrap v1 records")?;

        veilid_log!(self debug "Direct bootstrap v1 response: {}", bootv1response_str);

        let bootv1response: BootV1Response = match deserialize_json(bootv1response_str) {
            Ok(v) => v,
            Err(e) => {
                error!("{}", e);
                return Err(e).wrap_err("failed to deserialize bootstrap v1 response");
            }
        };

        // Parse v1 records
        let bsrecs = match self.parse_bootstrap_v1(&bootv1response.records) {
            Ok(v) => v,
            Err(e) => {
                veilid_log!(self debug "Direct bootstrap v1 parsing failure: {}", e);
                return Err(e);
            }
        };

        veilid_log!(self debug "Direct bootstrap v1 resolution: {:#?}", bsrecs);

        // Returned bootstrapped peers
        let routing_table = self.routing_table();

        let peers: Vec<Arc<PeerInfo>> = bsrecs
            .into_iter()
            .filter_map(|bsrec| {
                if routing_table.matches_own_public_key(bsrec.public_keys()) {
                    veilid_log!(self debug "Ignoring own node in bootstrap list");
                    None
                } else {
                    // If signed peer info exists for this record, use it
                    // This is important for browser websocket bootstrapping where the
                    // dialinfo in the bootstrap record has an unspecified IP address,
                    // and as such, a routing domain can not be determined for it
                    // by the code that receives the FindNodeA result
                    for pi in bootv1response.peers.iter().cloned() {
                        if pi.node_info().public_keys().contains_any_from_iter(bsrec.public_keys().iter()) {
                            return Some(pi);
                        }
                    }

                    // Otherwise use an unsigned peerinfo and try to resolve it directly from the bootstrap record
                    // The bootstrap will be rejected if a FindNodeQ could not resolve the peer info

                    // Get crypto support from list of node ids
                    let crypto_info_list: Vec<CryptoInfo> = bsrec.public_keys().iter().filter_map(|pk| {
                        match pk.kind() {
                            CRYPTO_KIND_VLD0 =>
                                Some(CryptoInfo::VLD0 { public_key: pk.value() }),

                            ck => {
                                veilid_log!(self warn "Ignoring unsupported bootstrap crypto kind: {}", ck);
                                None
                            }
                        }
                    }).collect();

                    // Make unsigned node info
                    // Use the current time for the timestamp since any older bootstrap peer info
                    // needs to be overwritten for this to be guaranteed to work
                    let timestamp = Timestamp::now();

                    let ni = NodeInfo::new(
                        timestamp,
                        bsrec.envelope_support().to_vec(), // Envelope support is as specified in the bootstrap list
                        crypto_info_list, // Crypto support is derived from list of node ids
                        vec![],           // Bootstrap needs no capabilities
                        ProtocolTypeSet::all(), // Bootstraps are always capable of all protocols
                        AddressTypeSet::all(), // Bootstraps are always IPV4 and IPV6 capable
                        bsrec.dial_info_details().to_vec(), // Dial info is as specified in the bootstrap list
                        vec![],                             // No relays for bootstrap nodes
                    );
                    let bspi = match PeerInfo::new_from_unsigned(
                        &routing_table,
                        RoutingDomain::PublicInternet,
                        ni,
                    ) {
                        Ok(v) => v,
                        Err(e) => {
                            veilid_log!(self error "Bootstrap has invalid peer info: {}", e);
                            return None;
                        }
                    };

                    Some(Arc::new(bspi))
                }
            })
            .collect();

        Ok(peers)
    }
}
