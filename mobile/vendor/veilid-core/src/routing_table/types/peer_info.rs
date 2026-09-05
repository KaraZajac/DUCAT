use super::*;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerInfo {
    /// The routing domain this peer info was translatd to from its origin routing domain.
    /// This is the 'best' routing domain for this peer info at the time it was received.
    /// This may differ from the -set- of routing domains for which its node info is -valid- at the given time
    /// Local networks, for example, can change, making this routing domain no longer one of the ones for which the node info is valid
    routing_domain: RoutingDomain,
    /// The calculated node ids for each crypto type in the node info
    node_ids: NodeIdGroup,
    /// The node info, as parsed. May not include everthing that is in the node info message if it is not understandable by this node
    node_info: NodeInfo,
    /// The node info message bytes, as serialized from a capnp message
    node_info_message: Bytes,
    /// The signatures for each public key in the node info, over the node info message.
    /// Only the signatures that are for the supported cryptosystems of this node are included, and those are validated.
    signatures: SignatureGroup,
}

impl fmt::Display for PeerInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "routing_domain: {:?}", self.routing_domain)?;
        writeln!(f, "node_ids: {}", self.node_ids)?;
        writeln!(f, "node_info:")?;
        write!(f, "{}", indent_all_string(f.to_string(&self.node_info)))?;
        Ok(())
    }
}

impl fmt::Debug for PeerInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PeerInfo")
            .field("routing_domain", &self.routing_domain)
            .field("node_ids", &self.node_ids)
            .field("node_info", &self.node_info)
            .field("len(node_info_message)", &self.node_info_message.len())
            .field("signatures", &self.signatures)
            .finish()
    }
}

impl PeerInfo {
    // Create a peer info from node info and secret
    pub fn new_from_node_info(
        routing_table: &RoutingTable,
        routing_domain: RoutingDomain,
        secret_keys: &SecretKeyGroup,
        node_info: NodeInfo,
    ) -> VeilidAPIResult<Self> {
        // Get signing public keys from node info
        let public_keys = node_info.public_keys();

        // Ensure node ids are within limits
        if public_keys.is_empty() {
            apibail_internal!(
                "no public keys for peer info ({:?})\n{:#?}",
                routing_domain,
                node_info
            );
        } else if public_keys.len() > MAX_CRYPTO_KINDS {
            apibail_internal!(
                "too many public keys for peer info ({:?}): {:?}\n{:#?}",
                routing_domain,
                public_keys,
                node_info
            );
        }

        // Make sure secret keys and public keys match and make keypairs
        let mut keypairs = KeyPairGroup::new();
        for pk in public_keys.iter() {
            let Some(sk) = secret_keys.get(pk.kind()) else {
                apibail_internal!("secret key not found for public key: {}", pk);
            };
            keypairs.add(KeyPair::new_from_parts(pk.clone(), sk.value()));
        }

        // Generate on-the-wire node info message
        let mut node_info_message_builder = ::capnp::message::Builder::new_default();
        let mut node_info_builder =
            node_info_message_builder.init_root::<veilid_capnp::node_info::Builder>();
        encode_node_info(&node_info, &mut node_info_builder)?;
        let node_info_message =
            canonical_message_builder_to_bytes_writer_packed(node_info_message_builder, |size| {
                BytesWriter::with_capacity(size)
            })?
            .into_inner()
            .freeze();

        // Sign the message
        let crypto = routing_table.crypto();
        let signatures = SignatureGroup::from(crypto.generate_signatures(
            &node_info_message,
            &keypairs,
            |_kp, sig| sig,
        )?);

        // Extract node ids for convenience
        let mut node_ids = NodeIdGroup::new();
        for pk in public_keys.iter() {
            node_ids.add(routing_table.generate_node_id(pk)?);
        }

        Ok(Self {
            routing_domain,
            node_ids,
            node_info,
            node_info_message,
            signatures,
        })
    }

    // Decode someone else's peer info we got from the network
    pub fn new_from_wire(
        routing_table: &RoutingTable,
        origin_routing_domain: RoutingDomain,
        node_info_message: &[u8],
        signatures: SignatureGroup,
    ) -> VeilidAPIResult<Option<Self>> {
        // Read node info message
        let node_info_message_reader = ::capnp::serialize_packed::read_message(
            node_info_message,
            capnp::message::ReaderOptions::new(),
        )
        .map_err(|e| VeilidAPIError::parse_error(e.to_string(), hex::encode(node_info_message)))?;

        let node_info_reader = node_info_message_reader
            .get_root::<veilid_capnp::node_info::Reader>()
            .map_err(|e| {
                VeilidAPIError::parse_error(e.to_string(), hex::encode(node_info_message))
            })?;

        let node_info = decode_node_info(&node_info_reader)?;

        // Get signing public keys from node info
        let public_keys = node_info.public_keys();

        // Ensure node ids are within limits
        if public_keys.is_empty() {
            apibail_internal!(
                "no public keys for peer info ({:?})\n{:#?}",
                origin_routing_domain,
                node_info
            );
        } else if public_keys.len() > MAX_CRYPTO_KINDS {
            apibail_internal!(
                "too many public keys for peer info ({:?}): {:?}\n{:#?}",
                origin_routing_domain,
                public_keys,
                node_info
            );
        }

        // Verify signatures
        let crypto = routing_table.crypto();

        let Some(valid_signatures) =
            crypto.verify_signatures(&public_keys, node_info_message, &signatures)?
        else {
            apibail_generic!("invalid signature for node info");
        };
        if valid_signatures.is_empty() {
            // No supported crypto kind for this peer info
            return Ok(None);
        }

        // Translate the routing domain
        let Some(routing_domain) =
            routing_table.find_best_node_info_routing_domain(origin_routing_domain, &node_info)
        else {
            return Ok(None);
        };

        // Extract node ids for convenience
        let mut node_ids = NodeIdGroup::new();
        for pk in public_keys.iter() {
            node_ids.add(routing_table.generate_node_id(pk)?);
        }

        // Make owned node info message
        let node_info_message = Bytes::copy_from_slice(node_info_message);

        // Return decoded peerinfo
        Ok(Some(Self {
            routing_domain,
            node_ids,
            node_info,
            node_info_message,
            signatures,
        }))
    }

    // Create an unsigned peer info
    pub fn new_from_unsigned(
        routing_table: &RoutingTable,
        routing_domain: RoutingDomain,
        node_info: NodeInfo,
    ) -> VeilidAPIResult<Self> {
        // Get signing public keys from node info
        let public_keys = node_info.public_keys();

        // Ensure node ids are within limits
        if public_keys.is_empty() {
            apibail_internal!(
                "no public keys for peer info ({:?})\n{:#?}",
                routing_domain,
                node_info
            );
        } else if public_keys.len() > MAX_CRYPTO_KINDS {
            apibail_internal!(
                "too many public keys for peer info ({:?}): {:?}\n{:#?}",
                routing_domain,
                public_keys,
                node_info
            );
        }

        // Generate on-the-wire node info message
        let mut node_info_message_builder = ::capnp::message::Builder::new_default();
        let mut node_info_builder =
            node_info_message_builder.init_root::<veilid_capnp::node_info::Builder>();
        encode_node_info(&node_info, &mut node_info_builder)?;
        let node_info_message =
            canonical_message_builder_to_bytes_writer_packed(node_info_message_builder, |size| {
                BytesWriter::with_capacity(size)
            })?
            .into_inner()
            .freeze();

        // Extract node ids for convenience
        let mut node_ids = NodeIdGroup::new();
        for pk in public_keys.iter() {
            node_ids.add(routing_table.generate_node_id(pk)?);
        }

        Ok(Self {
            routing_domain,
            node_ids,
            node_info,
            node_info_message,
            signatures: SignatureGroup::new(),
        })
    }

    pub fn routing_domain(&self) -> RoutingDomain {
        self.routing_domain
    }
    pub fn node_ids(&self) -> &NodeIdGroup {
        &self.node_ids
    }
    pub fn node_info(&self) -> &NodeInfo {
        &self.node_info
    }
    pub fn node_info_message(&self) -> &[u8] {
        &self.node_info_message
    }
    pub fn signatures(&self) -> &SignatureGroup {
        &self.signatures
    }

    #[expect(dead_code)]
    pub fn destructure(self) -> (RoutingDomain, NodeIdGroup, NodeInfo, Bytes, SignatureGroup) {
        (
            self.routing_domain,
            self.node_ids,
            self.node_info,
            self.node_info_message,
            self.signatures,
        )
    }

    /// Compare this PeerInfo to another one
    /// Exclude the signature and timestamp and any other fields that are not
    /// semantically valuable
    /// If the two are not equivalent they should be considered different
    /// enough for republication, but this is not the only criteria required
    /// for publication.
    pub fn equivalent(&self, other: &PeerInfo) -> bool {
        self.routing_domain == other.routing_domain
            && self.node_ids == other.node_ids
            && self.node_info.equivalent(&other.node_info)
    }
}
