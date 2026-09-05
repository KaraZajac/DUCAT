use super::*;
use crate::routing_table::*;
use core::convert::TryInto;

// Version number of envelope format
fourcc_type!(EnvelopeVersion);

// ENV0
pub const ENVELOPE_VERSION_ENV0: EnvelopeVersion = EnvelopeVersion::new(*b"ENV0");
pub const ENV0_NONCE_LENGTH: usize = 24;
pub const ENV0_SIGNATURE_LENGTH: usize = 64;
pub const ENV0_MAX_ENVELOPE_SIZE: usize = 65507;
pub const ENV0_MIN_ENVELOPE_SIZE: usize = 0x6A + 0x40; // Header + BareSignature

/// Envelope versions in order of preference, best envelope version is the first one, worst is the last one
pub const VALID_ENVELOPE_VERSIONS: [EnvelopeVersion; 1] = [ENVELOPE_VERSION_ENV0];
/// Number of envelope versions to keep on structures if many are present beyond the ones we consider valid
pub const MAX_ENVELOPE_VERSIONS: usize = 16;

/// Envelopes are versioned
///
/// These are the formats for the on-the-wire serialization performed by this module
///
/// #[repr(C, packed)]
/// struct EnvelopeHeader {
///     // Size is at least 4 bytes. Depending on the version specified, the size may vary and should be case to the appropriate struct
///     version: [u8; 4],            // 0x00: 0x45 0x4E 0x56 0x30 ("ENV0")
/// }
///
/// #[repr(C, packed)]
/// struct EnvelopeENV0 {
///     // Size is 106 bytes without signature and 170 with signature
///     version: [u8; 4],            // 0x00: 0x45 0x4E 0x56 0x30 ("ENV0")
///     crypto_kind: [u8; 4],        // 0x04: CryptoSystemVersion FOURCC code (CryptoKind)
///     size: u16,                   // 0x08: Total size of the envelope including the encrypted operations message. Maximum size is 65,507 bytes, which is the data size limit for a single UDP message on IPv4.
///     timestamp: u64,              // 0x0A: Duration since UNIX_EPOCH in microseconds when this message is sent. Messages older than 10 seconds are dropped.
///     nonce: [u8; 24],             // 0x12: Random nonce for replay protection and for dh
///     sender_id: [u8; 32],         // 0x2A: Node ID of the message source, which is the public key of the sender (must be verified with find_node if this is a new node_id/address combination)
///     recipient_id: [u8; 32],      // 0x4A: Node ID of the intended recipient, which is the public key of the recipient (must be the receiving node, or a relay lease holder)
///                                  // 0x6A: message is appended (operations)
///     signature: [u8; 64],         // 0x?? (end-0x40): BareSignature of the entire envelope including header is appended to the packet
///                                  // entire header needs to be included in message digest, relays are not allowed to modify the envelope without invalidating the signature.
/// }
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Envelope {
    ENV0 { env0: EnvelopeENV0 },
}

impl Envelope {
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "envelope", skip_all, fields(__VEILID_LOG_KEY = crypto.log_key()))
    )]
    pub fn try_new_env0(
        crypto: &Crypto,
        crypto_kind: CryptoKind,
        timestamp: Timestamp,
        nonce: Nonce,
        sender_id: NodeId,
        recipient_id: NodeId,
    ) -> VeilidAPIResult<Self> {
        Ok(Self::ENV0 {
            env0: EnvelopeENV0::try_new(
                crypto,
                crypto_kind,
                timestamp,
                nonce,
                sender_id,
                recipient_id,
            )?,
        })
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "envelope", skip_all, fields(__VEILID_LOG_KEY = crypto.log_key()))
    )]
    pub async fn try_from_signed_data(
        crypto: &Crypto,
        data: Bytes,
        network_key: &Option<BareSharedSecret>,
    ) -> VeilidAPIResult<Self> {
        // Ensure we are at least the length of the envelope
        // Silent drop here, as we use zero length packets as part of the protocol for hole punching
        if data.len() < 4 {
            apibail_generic!("envelope header too small");
        }

        // Check envelope version
        let version: EnvelopeVersion = data[0x00..0x04]
            .try_into()
            .map_err(VeilidAPIError::internal)?;

        match version {
            ENVELOPE_VERSION_ENV0 => Ok(Self::ENV0 {
                env0: EnvelopeENV0::try_from_signed_data(crypto, data, network_key).await?,
            }),
            _ => {
                apibail_parse_error!("unsupported envelope version", version.to_string());
            }
        }
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "envelope", skip_all, fields(__VEILID_LOG_KEY = crypto.log_key()))
    )]
    pub async fn decrypt_body(
        &self,
        crypto: &Crypto,
        data: Bytes,
        secret_key: &SecretKey,
        network_key: &Option<BareSharedSecret>,
    ) -> VeilidAPIResult<Bytes> {
        match self {
            Envelope::ENV0 { env0 } => {
                env0.decrypt_body(crypto, data, secret_key, network_key)
                    .await
            }
        }
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "envelope", skip_all, err, fields(__VEILID_LOG_KEY = crypto.log_key()))
    )]
    pub async fn to_encrypted_data(
        &self,
        crypto: &Crypto,
        body: Bytes,
        secret_key: &SecretKey,
        network_key: &Option<BareSharedSecret>,
    ) -> VeilidAPIResult<Bytes> {
        match self {
            Envelope::ENV0 { env0 } => {
                env0.to_encrypted_data(crypto, body, secret_key, network_key)
                    .await
            }
        }
    }

    pub fn get_version(&self) -> EnvelopeVersion {
        match self {
            Envelope::ENV0 { env0: _ } => ENVELOPE_VERSION_ENV0,
        }
    }
    pub fn get_crypto_kind(&self) -> CryptoKind {
        match self {
            Envelope::ENV0 { env0 } => env0.get_crypto_kind(),
        }
    }

    pub fn get_timestamp(&self) -> Timestamp {
        match self {
            Envelope::ENV0 { env0 } => env0.get_timestamp(),
        }
    }

    pub fn get_sender_id(&self) -> NodeId {
        match self {
            Envelope::ENV0 { env0 } => env0.get_sender_id(),
        }
    }

    pub fn get_recipient_id(&self) -> NodeId {
        match self {
            Envelope::ENV0 { env0 } => env0.get_recipient_id(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvelopeENV0 {
    crypto_kind: CryptoKind,
    timestamp: Timestamp,
    nonce: Nonce,
    bare_sender_id: BareNodeId,
    bare_recipient_id: BareNodeId,
}

impl EnvelopeENV0 {
    fn try_new(
        crypto: &Crypto,
        crypto_kind: CryptoKind,
        timestamp: Timestamp,
        nonce: Nonce,
        sender_id: NodeId,
        recipient_id: NodeId,
    ) -> VeilidAPIResult<Self> {
        let vcrypto = Self::validate_crypto_kind(crypto, crypto_kind)?;

        vcrypto.check_nonce(&nonce)?;
        Self::check_node_id(crypto_kind, &sender_id)?;
        Self::check_node_id(crypto_kind, &recipient_id)?;

        Ok(Self {
            crypto_kind,
            timestamp,
            nonce,
            bare_sender_id: sender_id.value(),
            bare_recipient_id: recipient_id.value(),
        })
    }

    async fn try_from_signed_data(
        crypto: &Crypto,
        data: Bytes,
        network_key: &Option<BareSharedSecret>,
    ) -> VeilidAPIResult<Self> {
        // Ensure we are at least the length of the envelope
        // Silent drop here, as we use zero length packets as part of the protocol for hole punching
        if data.len() < ENV0_MIN_ENVELOPE_SIZE {
            apibail_generic!("envelope data too small");
        }

        // Check crypto kind
        let crypto_kind = CryptoKind::new(
            data[0x04..0x08]
                .try_into()
                .map_err(VeilidAPIError::internal)?,
        );

        let vcrypto = Self::validate_crypto_kind(crypto, crypto_kind)?.as_async();

        // Get size and ensure it matches the size of the envelope and is less than the maximum message size
        let size: u16 = u16::from_le_bytes(
            data[0x08..0x0A]
                .try_into()
                .map_err(VeilidAPIError::internal)?,
        );
        if (size as usize) > ENV0_MAX_ENVELOPE_SIZE {
            apibail_parse_error!("envelope too large", size);
        }
        if (size as usize) != data.len() {
            apibail_parse_error!(
                "size doesn't match envelope size",
                format!(
                    "size doesn't match envelope size: size={} data.len()={}",
                    size,
                    data.len()
                )
            );
        }

        // Get the timestamp
        let timestamp: Timestamp = u64::from_le_bytes(
            data[0x0A..0x12]
                .try_into()
                .map_err(VeilidAPIError::internal)?,
        )
        .into();

        // Get nonce and sender node id
        let mut nonce_slice: [u8; ENV0_NONCE_LENGTH] = data[0x12..0x2A]
            .try_into()
            .map_err(VeilidAPIError::internal)?;
        let mut sender_id_slice: [u8; HASH_COORDINATE_LENGTH] = data[0x2A..0x4A]
            .try_into()
            .map_err(VeilidAPIError::internal)?;
        let mut recipient_id_slice: [u8; HASH_COORDINATE_LENGTH] = data[0x4A..0x6A]
            .try_into()
            .map_err(VeilidAPIError::internal)?;

        // Apply network key (not the best, but it will keep networks from colliding without much overhead)
        if let Some(nk) = network_key.as_ref() {
            for n in 0..ENV0_NONCE_LENGTH {
                nonce_slice[n] ^= nk[n];
            }
            for n in 0..HASH_COORDINATE_LENGTH {
                sender_id_slice[n] ^= nk[n];
            }
            for n in 0..HASH_COORDINATE_LENGTH {
                recipient_id_slice[n] ^= nk[n];
            }
        }

        let nonce: Nonce = Nonce::new(&nonce_slice);
        let bare_sender_id = BareNodeId::new(&sender_id_slice);
        let bare_recipient_id = BareNodeId::new(&recipient_id_slice);

        // Ensure sender_id and recipient_id are not the same
        if bare_sender_id == bare_recipient_id {
            apibail_parse_error!(
                "bare_sender_id should not be same as bare_recipient_id",
                bare_recipient_id.encode()
            );
        }

        let sender_public_key = PublicKey::new(crypto_kind, BarePublicKey::new(&bare_sender_id));

        // Get signature
        let bare_signature = BareSignature::new(
            data[(data.len() - ENV0_SIGNATURE_LENGTH)..]
                .try_into()
                .map_err(VeilidAPIError::internal)?,
        );
        let signature = Signature::new(crypto_kind, bare_signature);
        // Validate signature
        if !vcrypto
            .verify(
                &sender_public_key,
                data.slice(0..(data.len() - ENV0_SIGNATURE_LENGTH)),
                &signature,
            )
            .await
            .map_err(VeilidAPIError::internal)?
        {
            apibail_parse_error!("signature verification of envelope failed", signature);
        }

        // Return envelope
        Ok(Self {
            crypto_kind,
            timestamp,
            nonce,
            bare_sender_id,
            bare_recipient_id,
        })
    }

    pub async fn decrypt_body(
        &self,
        crypto: &Crypto,
        data: Bytes,
        secret_key: &SecretKey,
        network_key: &Option<BareSharedSecret>,
    ) -> VeilidAPIResult<Bytes> {
        // Get DH secret
        let vcrypto = crypto
            .get_async(self.crypto_kind)
            .expect_or_log("need to ensure only valid crypto kinds here");
        vcrypto.check_secret_key(secret_key)?;

        let sender_public_key =
            PublicKey::new(self.crypto_kind, BarePublicKey::new(&self.bare_sender_id));

        let mut dh_secret = vcrypto.cached_dh(&sender_public_key, secret_key).await?;

        // Apply network key
        if let Some(nk) = network_key.as_ref() {
            let mut dh_secret_bytes = dh_secret.ref_value().to_vec();

            for n in 0..dh_secret_bytes.len() {
                dh_secret_bytes[n] ^= nk[n % dh_secret_bytes.len()];
            }

            dh_secret =
                SharedSecret::new(dh_secret.kind(), BareSharedSecret::new(&dh_secret_bytes));
        }
        // Decrypt message without authentication
        let body = vcrypto
            .crypt_no_auth_unaligned(
                data.slice(0x6A..data.len() - ENV0_SIGNATURE_LENGTH),
                &self.nonce,
                &dh_secret,
            )
            .await?;

        // Decompress body
        let body = decompress_size_prepended(&body, ENV0_MAX_ENVELOPE_SIZE)?;

        Ok(body.into())
    }

    pub async fn to_encrypted_data(
        &self,
        crypto: &Crypto,
        body: Bytes,
        secret_key: &SecretKey,
        network_key: &Option<BareSharedSecret>,
    ) -> VeilidAPIResult<Bytes> {
        let vcrypto = crypto
            .get_async(self.crypto_kind)
            .expect_or_log("need to ensure only valid crypto kinds here");
        vcrypto.check_secret_key(secret_key)?;

        // Ensure body isn't too long
        let uncompressed_body_size: usize = body.len() + ENV0_MIN_ENVELOPE_SIZE;
        if uncompressed_body_size > ENV0_MAX_ENVELOPE_SIZE {
            apibail_parse_error!(
                "envelope size before compression is too large",
                uncompressed_body_size
            );
        }

        // Compress body
        let body = Bytes::from(compress_prepend_size(&body));

        // Ensure body isn't too long
        let envelope_size: usize = body.len() + ENV0_MIN_ENVELOPE_SIZE;
        if envelope_size > ENV0_MAX_ENVELOPE_SIZE {
            apibail_parse_error!(
                "envelope size after compression is too large",
                envelope_size
            );
        }
        // Generate dh secret
        let recipient_public_key = PublicKey::new(
            self.crypto_kind,
            BarePublicKey::new(&self.bare_recipient_id),
        );

        let mut dh_secret = vcrypto.cached_dh(&recipient_public_key, secret_key).await?;

        // Write envelope body
        let mut data = BytesMut::zeroed(envelope_size);

        // Write version
        data[0x00..0x04].copy_from_slice(&ENVELOPE_VERSION_ENV0.0);
        // Write crypto kind
        data[0x04..0x08].copy_from_slice(self.crypto_kind.bytes());
        // Write size
        data[0x08..0x0A].copy_from_slice(&(envelope_size as u16).to_le_bytes());
        // Write timestamp
        data[0x0A..0x12].copy_from_slice(&self.timestamp.as_u64().to_le_bytes());
        // Write nonce
        data[0x12..0x2A].copy_from_slice(&self.nonce);
        // Write sender node id
        data[0x2A..0x4A].copy_from_slice(&self.bare_sender_id);
        // Write recipient node id
        data[0x4A..0x6A].copy_from_slice(&self.bare_recipient_id);

        // Apply network key (not the best, but it will keep networks from colliding without much overhead)
        if let Some(nk) = network_key.as_ref() {
            let mut dh_secret_bytes = dh_secret.ref_value().to_vec();

            for n in 0..dh_secret_bytes.len() {
                dh_secret_bytes[n] ^= nk[n % dh_secret_bytes.len()];
            }
            for n in 0..ENV0_NONCE_LENGTH {
                data[0x12 + n] ^= nk[n];
            }
            for n in 0..HASH_COORDINATE_LENGTH {
                data[0x2A + n] ^= nk[n];
            }
            for n in 0..HASH_COORDINATE_LENGTH {
                data[0x4A + n] ^= nk[n];
            }

            dh_secret =
                SharedSecret::new(dh_secret.kind(), BareSharedSecret::new(&dh_secret_bytes));
        }

        // Encrypt message to body data
        let data = vcrypto
            .crypt_b2b_no_auth(body, data, 0x6A, &self.nonce, &dh_secret)
            .await?;

        // Sign the envelope
        let sender_public_key =
            PublicKey::new(self.crypto_kind, BarePublicKey::new(&self.bare_sender_id));
        let data = vcrypto
            .sign_in_place(
                &sender_public_key,
                secret_key,
                data,
                0..envelope_size - ENV0_SIGNATURE_LENGTH,
                envelope_size - ENV0_SIGNATURE_LENGTH,
            )
            .await?;

        Ok(data.freeze())
    }

    pub fn get_crypto_kind(&self) -> CryptoKind {
        self.crypto_kind
    }

    pub fn get_timestamp(&self) -> Timestamp {
        self.timestamp
    }

    pub fn get_sender_id(&self) -> NodeId {
        NodeId::new(self.crypto_kind, self.bare_sender_id.clone())
    }

    pub fn get_recipient_id(&self) -> NodeId {
        NodeId::new(self.crypto_kind, self.bare_recipient_id.clone())
    }

    //////////////////////////////////////////////////////////////////

    fn validate_crypto_kind(
        crypto: &Crypto,
        crypto_kind: CryptoKind,
    ) -> VeilidAPIResult<CryptoSystemGuard<'_>> {
        let vcrypto = crypto
            .get(crypto_kind)
            .ok_or_else(|| VeilidAPIError::parse_error("unsupported crypto kind", crypto_kind))?;

        // Verify crypto kind can be used with this envelope
        if vcrypto.nonce_length() != ENV0_NONCE_LENGTH
            || vcrypto.hash_digest_length() != HASH_COORDINATE_LENGTH
            || vcrypto.public_key_length() != HASH_COORDINATE_LENGTH
            || vcrypto.signature_length() != ENV0_SIGNATURE_LENGTH
        {
            apibail_generic!("unsupported crypto kind for this envelope type");
        }

        Ok(vcrypto)
    }

    fn check_node_id(crypto_kind: CryptoKind, node_id: &NodeId) -> VeilidAPIResult<()> {
        if node_id.kind() != crypto_kind {
            apibail_parse_error!("invalid crypto kind for ENV0", node_id.kind());
        }
        if node_id.ref_value().len() != HASH_COORDINATE_LENGTH {
            apibail_parse_error!("invalid node_id length for ENV0", node_id.ref_value().len());
        }
        Ok(())
    }
}
