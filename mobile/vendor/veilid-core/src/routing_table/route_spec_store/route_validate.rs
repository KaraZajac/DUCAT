use super::*;

impl RouteSpecStore {
    /// validate data using a private route's key and signature chain
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "rtab::route", skip(self, data), ret, fields(__VEILID_LOG_KEY = self.log_key()))
    )]
    pub async fn get_signature_validated_route(
        &self,
        public_key: &PublicKey,
        signatures: &[Signature],
        data: Bytes,
        sequencing: Sequencing,
        last_hop_id: &NodeId,
    ) -> Option<(SecretKey, SafetySpec)> {
        let crypto = self.crypto();

        let (hop_node_refs, secret_key, safety_spec) = {
            let cache = self.cache.read();

            let Some(id) = cache.get_allocated_route_id_by_key(public_key) else {
                veilid_log!(self debug target: "network_result", "route id does not exist for key: {}", public_key);
                return None;
            };
            let Some(arce) = cache.get_allocated_route_by_id(&id) else {
                veilid_log!(self debug "route does not exist for id: {}", id);
                return None;
            };

            let Ok(secret_key) = arce.route_set_secret_for_key(public_key) else {
                veilid_log!(self error "no secret key for public key: {}", public_key);
                return None;
            };

            // Ensure we have the right number of signatures
            if signatures.len() != arce.hop_count() - 1 {
                // Wrong number of signatures
                veilid_log!(self debug "wrong number of signatures ({} should be {}) for routed operation on private route {}", signatures.len(), arce.hop_count() - 1, public_key);
                return None;
            }

            (
                arce.hop_node_refs(),
                secret_key,
                SafetySpec {
                    preferred_route: Some(id.into()),
                    hop_count: arce.hop_count(),
                    stability: arce.stability(),
                    sequencing,
                },
            )
        };

        // Validate signatures to ensure the route was handled by the nodes and not messed with
        // This is in private route (reverse) order as we are receiving over the route
        for (hop_n, hop_node_ref) in hop_node_refs.iter().rev().enumerate() {
            // The last hop is not signed, as the whole packet is signed
            if hop_n == signatures.len() {
                // Verify the node we received the routed operation from is the last hop in our route
                if !hop_node_ref.node_ids().contains(last_hop_id) {
                    veilid_log!(self debug "received routed operation from the wrong hop ({} should be {}) on private route {}", hop_node_ref, last_hop_id, public_key);
                    return None;
                }
            } else {
                let Some(hop_public_key) = hop_node_ref
                    .public_keys(RoutingDomain::PublicInternet)
                    .get(signatures[hop_n].kind())
                else {
                    veilid_log!(self debug "no hop public key matching signature kind {} at hop {} for routed operation on private route {}", signatures[hop_n].kind(), hop_n, public_key);
                    return None;
                };
                // Verify a signature for a hop node along the route
                let Some(vcrypto) = crypto.get_async(hop_public_key.kind()) else {
                    veilid_log!(self debug "can't handle route hop with public key: {:?}", hop_public_key.kind());
                    return None;
                };
                match vcrypto
                    .verify(&hop_public_key, data.clone(), &signatures[hop_n])
                    .await
                {
                    Ok(true) => {}
                    Ok(false) => {
                        veilid_log!(self debug "invalid signature for hop {} at {} on private route {}", hop_n, hop_node_ref, public_key);
                        return None;
                    }
                    Err(e) => {
                        veilid_log!(self debug "error verifying signature for hop {} at {} on private route {}: {}", hop_n, hop_node_ref, public_key, e);
                        return None;
                    }
                }
            }
        }
        // We got the correct signatures, return a key and response safety spec
        Some((secret_key, safety_spec))
    }
}
