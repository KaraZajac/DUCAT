#![allow(clippy::absurd_extreme_comparisons)]

use super::*;
use crate::routing_table::*;
use core::convert::TryInto;

// Version number of receipt format
fourcc_type!(ReceiptVersion);

// RCP0
pub const RECEIPT_VERSION_RCP0: ReceiptVersion = ReceiptVersion::new(*b"RCP0");
pub const RCP0_NONCE_LENGTH: usize = 24;
pub const RCP0_SIGNATURE_LENGTH: usize = 64;
pub const RCP0_MAX_RECEIPT_SIZE: usize = 1380;
pub const RCP0_MAX_EXTRA_DATA_SIZE: usize = RCP0_MAX_RECEIPT_SIZE - RCP0_MIN_RECEIPT_SIZE; // 1250
pub const RCP0_MIN_RECEIPT_SIZE: usize = 130;

/// Receipt versions in order of preference, best receipt version is the first one, worst is the last one
pub const VALID_RECEIPT_VERSIONS: [ReceiptVersion; 1] = [RECEIPT_VERSION_RCP0];

/// Return the best receipt version we support
pub fn best_receipt_version() -> ReceiptVersion {
    VALID_RECEIPT_VERSIONS[0]
}

/// Out-of-band receipts are versioned along with envelopes.
///
/// #[repr(C, packed)]
/// struct ReceiptHeader {
///     // Size is at least 4 bytes. Depending on the version specified, the size may vary and should be case to the appropriate struct
///     version: [u8; 4],            // 0x00: 0x52 0x43 0x50 0x30 ("RCP0")
/// }
///
/// #[repr(C, packed)]
/// struct ReceiptRCP0 {
///     // Size is 66 bytes without extra data and signature, 130 with signature
///     version: [u8; 4],            // 0x00: 0x52 0x43 0x50 0x30 ("RCP0")
///     crypto_kind: [u8; 4],        // 0x04: CryptoSystemVersion FOURCC code
///     size: u16,                   // 0x08: Total size of the receipt including the extra data and the signature. Maximum size is 1380 bytes.
///     nonce: [u8; 24],             // 0x0A: Randomly chosen bytes that represent a unique receipt. Could be used to encrypt the extra data, but it's not required.
///     sender_id: [u8; 32],         // 0x22: Node ID of the message source, which is the public key of the sender
///     extra_data: [u8; ??],        // 0x42: Extra data is appended (arbitrary extra data, not encrypted by receipt itself, maximum size is 1250 bytes)
///     signature: [u8; 64],         // 0x?? (end-0x40): BareSignature of the entire receipt including header and extra data is appended to the packet
/// }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Receipt {
    RCP0 { rcp0: ReceiptRCP0 },
}

impl Receipt {
    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "envelope", skip_all, fields(__VEILID_LOG_KEY = crypto.log_key()))
    )]
    pub fn try_new_rcp0(
        crypto: &Crypto,
        crypto_kind: CryptoKind,
        nonce: Nonce,
        sender_id: NodeId,
        extra_data: Bytes,
    ) -> VeilidAPIResult<Self> {
        Ok(Self::RCP0 {
            rcp0: ReceiptRCP0::try_new(crypto, crypto_kind, nonce, sender_id, extra_data)?,
        })
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "receipt", skip_all, err, fields(__VEILID_LOG_KEY = crypto.log_key()))
    )]
    pub async fn try_from_signed_data(crypto: &Crypto, data: Bytes) -> VeilidAPIResult<Receipt> {
        // Ensure we are at least the length of the envelope
        if data.len() < 4 {
            apibail_parse_error!("receipt header too small", data.len());
        }

        // Check version
        let version: ReceiptVersion = data[0x00..0x04]
            .try_into()
            .map_err(VeilidAPIError::internal)?;

        match version {
            RECEIPT_VERSION_RCP0 => Ok(Self::RCP0 {
                rcp0: ReceiptRCP0::try_from_signed_data(crypto, data).await?,
            }),
            _ => {
                apibail_parse_error!("unsupported receipt version", version.to_string());
            }
        }
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "envelope", skip_all, fields(__VEILID_LOG_KEY = crypto.log_key()))
    )]
    pub async fn to_signed_data(
        &self,
        crypto: &Crypto,
        secret_key: &SecretKey,
    ) -> VeilidAPIResult<Bytes> {
        match self {
            Receipt::RCP0 { rcp0 } => rcp0.to_signed_data(crypto, secret_key).await,
        }
    }

    #[expect(dead_code)]
    pub fn get_version(&self) -> ReceiptVersion {
        match self {
            Receipt::RCP0 { rcp0: _ } => RECEIPT_VERSION_RCP0,
        }
    }

    #[expect(dead_code)]
    pub fn get_crypto_kind(&self) -> CryptoKind {
        match self {
            Receipt::RCP0 { rcp0 } => rcp0.get_crypto_kind(),
        }
    }

    pub fn get_nonce(&self) -> Nonce {
        match self {
            Receipt::RCP0 { rcp0 } => rcp0.get_nonce(),
        }
    }

    #[expect(dead_code)]
    pub fn get_sender_id(&self) -> NodeId {
        match self {
            Receipt::RCP0 { rcp0 } => rcp0.get_sender_id(),
        }
    }

    pub fn get_extra_data(&self) -> &[u8] {
        match self {
            Receipt::RCP0 { rcp0 } => rcp0.get_extra_data(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptRCP0 {
    crypto_kind: CryptoKind,
    nonce: Nonce,
    bare_sender_id: BareNodeId,
    extra_data: Bytes,
}

impl ReceiptRCP0 {
    pub fn try_new(
        crypto: &Crypto,
        crypto_kind: CryptoKind,
        nonce: Nonce,
        sender_id: NodeId,
        extra_data: Bytes,
    ) -> VeilidAPIResult<Self> {
        let vcrypto = Self::validate_crypto_kind(crypto, crypto_kind)?;

        vcrypto.check_nonce(&nonce)?;
        Self::check_node_id(crypto_kind, &sender_id)?;

        if extra_data.as_ref().len() > RCP0_MAX_EXTRA_DATA_SIZE {
            apibail_parse_error!(
                "extra data too large for receipt",
                extra_data.as_ref().len()
            );
        }

        Ok(Self {
            crypto_kind,
            nonce,
            bare_sender_id: sender_id.value(),
            extra_data,
        })
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "receipt", skip_all, err, fields(__VEILID_LOG_KEY = crypto.log_key()))
    )]
    pub async fn try_from_signed_data(crypto: &Crypto, data: Bytes) -> VeilidAPIResult<Self> {
        // Ensure we are at least the length of the envelope
        if data.len() < RCP0_MIN_RECEIPT_SIZE {
            apibail_parse_error!("receipt too small", data.len());
        }

        // Check crypto kind
        let crypto_kind = CryptoKind::try_from(&data[0x04..0x08])?;
        let vcrypto = Self::validate_crypto_kind(crypto, crypto_kind)?.as_async();

        // Get size and ensure it matches the size of the envelope and is less than the maximum message size
        let size: u16 = u16::from_le_bytes(
            data[0x08..0x0A]
                .try_into()
                .map_err(VeilidAPIError::internal)?,
        );
        if (size as usize) > RCP0_MAX_RECEIPT_SIZE {
            apibail_parse_error!("receipt size is too large", size);
        }
        if (size as usize) != data.len() {
            apibail_parse_error!(
                "size doesn't match receipt size",
                format!("size={} data.len()={}", size, data.len())
            );
        }

        // Get sender id
        let bare_sender_id = BareNodeId::new(
            data[0x22..0x42]
                .try_into()
                .map_err(VeilidAPIError::internal)?,
        );
        let sender_public_key = PublicKey::new(crypto_kind, BarePublicKey::new(&bare_sender_id));

        // Get signature
        let bare_signature = BareSignature::new(
            data[(data.len() - 64)..]
                .try_into()
                .map_err(VeilidAPIError::internal)?,
        );

        let signature = Signature::new(crypto_kind, bare_signature);

        // Validate signature
        if !vcrypto
            .verify(
                &sender_public_key,
                data.slice(0..(data.len() - 64)),
                &signature,
            )
            .await?
        {
            apibail_parse_error!("signature failure in receipt", signature);
        }

        // Get nonce
        let nonce: Nonce = Nonce::new(
            data[0x0A..0x22]
                .try_into()
                .map_err(VeilidAPIError::internal)?,
        );

        // Get extra data and signature
        let extra_data = Bytes::copy_from_slice(&data[0x42..(data.len() - 64)]);

        // Return receipt
        Ok(Self {
            crypto_kind,
            nonce,
            bare_sender_id,
            extra_data,
        })
    }

    #[cfg_attr(
        feature = "instrument",
        instrument(level = "trace", target = "receipt", skip_all, err, fields(__VEILID_LOG_KEY = crypto.log_key()))
    )]
    pub async fn to_signed_data(
        &self,
        crypto: &Crypto,
        secret_key: &SecretKey,
    ) -> VeilidAPIResult<Bytes> {
        let vcrypto = crypto
            .get_async(self.crypto_kind)
            .expect_or_log("need to ensure only valid crypto kinds here");
        vcrypto.check_secret_key(secret_key)?;

        // Ensure extra data isn't too long
        let receipt_size: usize = self.extra_data.len() + RCP0_MIN_RECEIPT_SIZE;
        if receipt_size > RCP0_MAX_RECEIPT_SIZE {
            apibail_parse_error!("receipt too large", receipt_size);
        }

        let mut data = BytesMut::zeroed(receipt_size);

        // Write version
        data[0x00..0x04].copy_from_slice(&RECEIPT_VERSION_RCP0.0);
        // Write crypto kind
        data[0x04..0x08].copy_from_slice(self.crypto_kind.bytes());
        // Write size
        data[0x08..0x0A].copy_from_slice(&(receipt_size as u16).to_le_bytes());
        // Write nonce
        data[0x0A..0x22].copy_from_slice(&self.nonce);
        // Write sender node id
        data[0x22..0x42].copy_from_slice(&self.bare_sender_id);
        // Write extra data
        if !self.extra_data.is_empty() {
            data[0x42..(receipt_size - RCP0_SIGNATURE_LENGTH)].copy_from_slice(&self.extra_data);
        }

        // Sign the receipt
        let sender_public_key =
            PublicKey::new(self.crypto_kind, BarePublicKey::new(&self.bare_sender_id));

        let data = vcrypto
            .sign_in_place(
                &sender_public_key,
                secret_key,
                data,
                0..(receipt_size - RCP0_SIGNATURE_LENGTH),
                receipt_size - RCP0_SIGNATURE_LENGTH,
            )
            .await?;

        Ok(data.freeze())
    }

    pub fn get_crypto_kind(&self) -> CryptoKind {
        self.crypto_kind
    }

    pub fn get_nonce(&self) -> Nonce {
        self.nonce.clone()
    }

    pub fn get_sender_id(&self) -> NodeId {
        NodeId::new(self.crypto_kind, self.bare_sender_id.clone())
    }

    pub fn get_extra_data(&self) -> &[u8] {
        &self.extra_data
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
        if vcrypto.nonce_length() != RCP0_NONCE_LENGTH
            || vcrypto.hash_digest_length() != HASH_COORDINATE_LENGTH
            || vcrypto.public_key_length() != HASH_COORDINATE_LENGTH
        {
            apibail_generic!("unsupported crypto kind for this envelope type");
        }

        Ok(vcrypto)
    }

    fn check_node_id(crypto_kind: CryptoKind, node_id: &NodeId) -> VeilidAPIResult<()> {
        if node_id.kind() != crypto_kind {
            apibail_parse_error!("invalid crypto kind for RCP0", node_id.kind());
        }
        if node_id.ref_value().len() != HASH_COORDINATE_LENGTH {
            apibail_parse_error!("invalid node_id length for RCP0", node_id.ref_value().len());
        }
        Ok(())
    }
}
