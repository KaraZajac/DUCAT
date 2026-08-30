use std::slice::from_ref;

use capnp::{
    message::{self, ReaderOptions},
    serialize::{self},
};
use veilid_core::{
    BareOpaqueRecordKey, BarePublicKey, BareRecordKey, BareSignature, CryptoSystemGuard, KeyPair,
    PublicKey, RecordKey, Signature,
};

use crate::DHTAddr;

pub mod error;
#[rustfmt::skip]
mod veilnet_capnp;

pub use error::{Error, Result};

pub trait Encoder {
    fn encode(&self) -> Result<Vec<u8>>;
}

pub trait Decoder: Sized {
    fn decode(buf: &[u8]) -> Result<Self>;
}

// TODO: improve this module, zero-cost abstractions if we can

#[derive(Clone, Debug)]
pub struct DHTRouteData {
    pub route_data: Vec<u8>,
    pub owner_key: PublicKey,
}

impl DHTRouteData {
    pub fn new(route_data: Vec<u8>, owner_key: PublicKey) -> Self {
        Self {
            route_data,
            owner_key,
        }
    }
}

const MAX_DHT_ROUTE_LEN: usize = 32768;
const MAX_DATAGRAM_LEN: usize = 32768;
const MAX_PACKET_LEN: usize = 32768;

impl Encoder for DHTRouteData {
    fn encode(&self) -> Result<Vec<u8>> {
        let mut builder = message::Builder::new_default();
        let mut dht_route_builder = builder.get_root::<veilnet_capnp::dht_route::Builder>()?;

        dht_route_builder.set_route_data(self.route_data.as_slice());

        let mut typed_key_builder = dht_route_builder.reborrow().init_owner_key();
        typed_key_builder.set_kind(self.owner_key.kind().into());
        let mut key_builder = typed_key_builder.reborrow().init_key();
        let owner_key_value = self.owner_key.value();
        key_builder.set_p0(u64::from_be_bytes(owner_key_value[0..8].try_into()?));
        key_builder.set_p1(u64::from_be_bytes(owner_key_value[8..16].try_into()?));
        key_builder.set_p2(u64::from_be_bytes(owner_key_value[16..24].try_into()?));
        key_builder.set_p3(u64::from_be_bytes(owner_key_value[24..32].try_into()?));

        let message = serialize::write_message_segments_to_words(&builder);
        if message.len() > MAX_DHT_ROUTE_LEN {
            return Err(Error::MessageTooLarge {
                length: message.len(),
                limit: MAX_DHT_ROUTE_LEN,
            });
        }
        Ok(message)
    }
}

impl Decoder for DHTRouteData {
    fn decode(buf: &[u8]) -> Result<Self> {
        let reader = serialize::read_message(buf, ReaderOptions::new())?;
        let dht_route_reader = reader.get_root::<veilnet_capnp::dht_route::Reader>()?;

        let route_data = dht_route_reader.get_route_data()?;
        let typed_key_reader = dht_route_reader.get_owner_key()?;
        let key_reader = typed_key_reader.get_key()?;
        let mut key_bytes = [0u8; 32];
        key_bytes[0..8].clone_from_slice(&key_reader.get_p0().to_be_bytes()[..]);
        key_bytes[8..16].clone_from_slice(&key_reader.get_p1().to_be_bytes()[..]);
        key_bytes[16..24].clone_from_slice(&key_reader.get_p2().to_be_bytes()[..]);
        key_bytes[24..32].clone_from_slice(&key_reader.get_p3().to_be_bytes()[..]);
        let owner_key = PublicKey::new(
            typed_key_reader.get_kind().into(),
            BarePublicKey::new(&key_bytes),
        );
        Ok(Self {
            route_data: route_data.to_vec(),
            owner_key,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Datagram {
    pub addr: DHTAddr,
    pub owner_key: PublicKey,
    pub contents: Vec<u8>,
    pub sequence: Option<u32>,
}

impl Datagram {
    pub fn new(addr: DHTAddr, owner_key: PublicKey, contents: &[u8]) -> Datagram {
        Self {
            addr,
            owner_key,
            contents: contents.to_vec(),
            sequence: None,
        }
    }
}

impl Encoder for Datagram {
    fn encode(&self) -> Result<Vec<u8>> {
        let mut builder = message::Builder::new_default();
        let mut datagram_builder = builder.get_root::<veilnet_capnp::datagram::Builder>()?;

        // Set source address (TypedRecordKey)
        let mut source_addr_builder = datagram_builder.reborrow().init_source_addr();
        source_addr_builder.set_kind(self.addr.key.kind().into());
        let mut key_builder = source_addr_builder.reborrow().init_key();
        let key_bytes = self.addr.key.ref_value().ref_key();
        key_builder.set_p0(u64::from_be_bytes(key_bytes[0..8].try_into()?));
        key_builder.set_p1(u64::from_be_bytes(key_bytes[8..16].try_into()?));
        key_builder.set_p2(u64::from_be_bytes(key_bytes[16..24].try_into()?));
        key_builder.set_p3(u64::from_be_bytes(key_bytes[24..32].try_into()?));
        if let Some(secret) = self.addr.key.value().ref_encryption_key() {
            let secret_builder = source_addr_builder.reborrow().init_secret(32);
            secret_builder.copy_from_slice(secret);
        }

        // Set source port
        datagram_builder.set_source_port(self.addr.subkey);

        // Set owner key (TypedPublicKey)
        let mut owner_key_builder = datagram_builder.reborrow().init_owner_key();
        owner_key_builder.set_kind(self.owner_key.kind().into());
        let mut key_builder = owner_key_builder.reborrow().init_key();
        let key_bytes = self.owner_key.ref_value();
        key_builder.set_p0(u64::from_be_bytes(key_bytes[0..8].try_into()?));
        key_builder.set_p1(u64::from_be_bytes(key_bytes[8..16].try_into()?));
        key_builder.set_p2(u64::from_be_bytes(key_bytes[16..24].try_into()?));
        key_builder.set_p3(u64::from_be_bytes(key_bytes[24..32].try_into()?));

        // Set contents
        datagram_builder.set_contents(&self.contents);

        if let Some(sequence) = self.sequence {
            datagram_builder.set_sequence(sequence);
        }

        let message = serialize::write_message_segments_to_words(&builder);
        if message.len() > MAX_DATAGRAM_LEN {
            return Err(Error::MessageTooLarge {
                length: message.len(),
                limit: MAX_DATAGRAM_LEN,
            });
        }
        Ok(message)
    }
}

impl Decoder for Datagram {
    fn decode(buf: &[u8]) -> Result<Self> {
        let reader = serialize::read_message(buf, ReaderOptions::new())?;
        let datagram_reader = reader.get_root::<veilnet_capnp::datagram::Reader>()?;

        // Decode source address
        let source_addr_reader = datagram_reader.get_source_addr()?;
        let key_reader = source_addr_reader.get_key()?;
        let mut key_bytes = [0u8; 32];
        key_bytes[0..8].clone_from_slice(&key_reader.get_p0().to_be_bytes()[..]);
        key_bytes[8..16].clone_from_slice(&key_reader.get_p1().to_be_bytes()[..]);
        key_bytes[16..24].clone_from_slice(&key_reader.get_p2().to_be_bytes()[..]);
        key_bytes[24..32].clone_from_slice(&key_reader.get_p3().to_be_bytes()[..]);
        let secret_key = if source_addr_reader.has_secret() {
            let secret_reader = source_addr_reader.get_secret()?;
            Some(secret_reader.into())
        } else {
            None
        };
        let addr_key = RecordKey::new(
            source_addr_reader.get_kind().into(),
            BareRecordKey::new(BareOpaqueRecordKey::new(&key_bytes), secret_key),
        );

        // Create DHTAddr
        let addr = DHTAddr {
            key: addr_key,
            subkey: datagram_reader.get_source_port(),
        };

        let owner_key_reader = datagram_reader.get_owner_key()?;
        let key_reader = owner_key_reader.get_key()?;
        let mut key_bytes = [0u8; 32];
        key_bytes[0..8].clone_from_slice(&key_reader.get_p0().to_be_bytes()[..]);
        key_bytes[8..16].clone_from_slice(&key_reader.get_p1().to_be_bytes()[..]);
        key_bytes[16..24].clone_from_slice(&key_reader.get_p2().to_be_bytes()[..]);
        key_bytes[24..32].clone_from_slice(&key_reader.get_p3().to_be_bytes()[..]);
        let owner_key = PublicKey::new(
            owner_key_reader.get_kind().into(),
            BarePublicKey::new(&key_bytes),
        );

        // Decode contents
        let contents = datagram_reader.get_contents()?.to_vec();

        Ok(Self {
            addr,
            owner_key,
            contents,
            sequence: if datagram_reader.get_sequence() != 0 {
                Some(datagram_reader.get_sequence())
            } else {
                None
            },
        })
    }
}

#[derive(Clone, Debug)]
pub struct Packet {
    pub datagram: Vec<u8>,
    pub signature: Vec<u8>,
}

impl Packet {
    pub fn datagram(&self) -> Result<Datagram> {
        Datagram::decode(self.datagram.as_slice())
    }

    pub fn new_signature(
        datagram: Datagram,
        crypto: &CryptoSystemGuard<'_>,
        keypair: KeyPair,
    ) -> Result<Packet> {
        let datagram = datagram.encode()?;
        let signature = Self::sign(datagram.as_slice(), crypto, keypair)?;
        Ok(Self {
            datagram,
            signature,
        })
    }

    pub fn new_signed(datagram: Vec<u8>, signature: Vec<u8>) -> Packet {
        Self {
            datagram,
            signature,
        }
    }
}

impl Packet {
    pub fn sign(
        datagram: &[u8],
        crypto: &CryptoSystemGuard<'_>,
        keypair: KeyPair,
    ) -> Result<Vec<u8>> {
        let mut sigs = crypto
            .crypto()
            .generate_signatures(datagram, &[keypair], |_, sig| sig.value().to_vec())
            .map_err(|e| Error::Sign(e.into()))?;
        match sigs.pop() {
            Some(sig) => Ok(sig),
            None => Err(Error::Sign(anyhow::anyhow!("missing signature"))),
        }
    }

    pub fn verify(&self, datagram: &Datagram, crypto: &CryptoSystemGuard<'_>) -> Result<()> {
        let result = crypto
            .crypto()
            .verify_signatures(
                from_ref(&datagram.owner_key),
                self.datagram.as_slice(),
                &[Signature::new(
                    datagram.owner_key.kind(),
                    BareSignature::new(self.signature.as_slice()),
                )],
            )
            .map_err(|e| Error::Verify(e.into()))?;
        match result {
            Some(_) => Ok(()),
            None => Err(Error::Verify(anyhow::anyhow!(
                "failed to verify datagram signature"
            ))),
        }
    }
}

impl Encoder for Packet {
    fn encode(&self) -> Result<Vec<u8>> {
        let mut builder = message::Builder::new_default();
        let mut datagram_builder = builder.get_root::<veilnet_capnp::packet::Builder>()?;

        datagram_builder.set_datagram(&self.datagram);
        datagram_builder.set_signature(&self.signature);

        let message = serialize::write_message_segments_to_words(&builder);
        if message.len() > MAX_PACKET_LEN {
            return Err(Error::MessageTooLarge {
                length: message.len(),
                limit: MAX_PACKET_LEN,
            });
        }
        Ok(message)
    }
}

impl Decoder for Packet {
    fn decode(buf: &[u8]) -> Result<Self> {
        let reader = serialize::read_message(buf, ReaderOptions::new())?;
        let datagram_reader = reader.get_root::<veilnet_capnp::packet::Reader>()?;

        Ok(Self {
            datagram: datagram_reader.get_datagram()?.to_vec(),
            signature: datagram_reader.get_signature()?.to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::testing::create_test_config;
    use crate::{Connection, connection::Veilid, proto};
    use tempfile::TempDir;
    use veilid_core::{
        BareOpaqueRecordKey, BarePublicKey, BareRecordKey, CRYPTO_KIND_VLD0, PublicKey, RecordKey,
        VeilidAPIError,
    };

    fn create_test_datagram() -> Datagram {
        let key = RecordKey::new(
            CRYPTO_KIND_VLD0,
            BareRecordKey::new(
                BareOpaqueRecordKey::new(&[
                    0x12u8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                    0x00, 0x00, 0x00, 0x00, 0x00, 0x34,
                ]),
                None,
            ),
        );
        let owner_public_key = PublicKey::new(
            CRYPTO_KIND_VLD0,
            BarePublicKey::new(&[
                0xab, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0xcd,
            ]),
        );

        Datagram::new(
            DHTAddr { key, subkey: 8080 },
            owner_public_key,
            b"Hello, world!",
        )
    }

    fn create_test_datagram_packet() -> Packet {
        Packet {
            datagram: create_test_datagram().encode().expect("encode datagram"),
            signature: b"signature_data".to_vec(),
        }
    }

    #[test]
    fn test_datagram_encode_decode() {
        let original = create_test_datagram();

        let encoded = original.encode().expect("Failed to encode datagram");
        let decoded = Datagram::decode(&encoded).expect("Failed to decode datagram");

        assert_eq!(original.addr.key.kind(), decoded.addr.key.kind());
        assert_eq!(
            original.addr.key.value().key(),
            decoded.addr.key.value().key()
        );
        assert_eq!(original.addr.subkey, decoded.addr.subkey);
        assert_eq!(original.contents, decoded.contents);
    }

    #[test]
    fn test_datagram_packet_encode_decode() {
        let original = create_test_datagram_packet();

        let encoded = original.encode().expect("Failed to encode datagram packet");
        let decoded = Packet::decode(&encoded).expect("Failed to decode datagram packet");

        let original_datagram = original.datagram().expect("decode datagram");
        let decoded_datagram = decoded.datagram().expect("decode datagram");

        assert_eq!(
            original_datagram.addr.key.kind(),
            decoded_datagram.addr.key.kind(),
        );
        assert_eq!(
            original_datagram.addr.key.value().key(),
            decoded_datagram.addr.key.value().key(),
        );
        assert_eq!(original_datagram.addr.subkey, decoded_datagram.addr.subkey);
        assert_eq!(original_datagram.contents, decoded_datagram.contents);
        assert_eq!(original.signature, decoded.signature);
    }

    #[test]
    fn test_datagram_empty_contents() {
        let mut datagram = create_test_datagram();
        datagram.contents = Vec::new();

        let encoded = datagram.encode().expect("encode empty datagram");
        let decoded = Datagram::decode(&encoded).expect("decode empty datagram");

        assert_eq!(datagram.contents, decoded.contents);
        assert!(decoded.contents.is_empty());
    }

    #[test]
    fn test_datagram_contents_1k() {
        let mut datagram = create_test_datagram();
        datagram.contents = vec![0u8; 1024]; // 1KB of zeros

        let encoded = datagram.encode().expect("encode 1k datagram");
        let decoded = Datagram::decode(&encoded).expect("decode 1k datagram");

        assert_eq!(datagram.contents, decoded.contents);
        assert_eq!(decoded.contents.len(), 1024);
    }

    #[test]
    fn test_datagram_contents_over_limit() {
        let mut datagram = create_test_datagram();
        datagram.contents = vec![0u8; 65536]; // 64KB of zeros

        datagram.encode().expect_err("encode oversized datagram");
    }

    #[test]
    fn test_packet_contents_over_limit() {
        let mut datagram_packet = create_test_datagram_packet();
        datagram_packet.datagram = vec![0u8; 65536]; // 64KB of zeros

        datagram_packet
            .encode()
            .expect_err("encode oversized packet");
    }

    #[tokio::test]
    async fn test_datagram_sign_verify() {
        let state_dir = TempDir::new().expect("tempdir");
        let config = create_test_config(state_dir.path());
        let conn = Veilid::new_config(config).await.expect("veilid API");

        let owner_keypair = conn
            .with_crypto(|crypto| Ok::<_, VeilidAPIError>(crypto.generate_keypair()))
            .unwrap();
        let addr = DHTAddr::new(
            "VLD0:Q0TGVjxV-LjDtpsj1wojsEBx8SU9egVwSlKo0bNjQoY"
                .parse()
                .expect("record key"),
            0u16,
        );
        let datagram = Datagram::new(addr.to_owned(), owner_keypair.key(), b"hello world");

        conn.with_crypto(move |crypto| {
            let mut packet =
                Packet::new_signature(datagram.clone(), &crypto, owner_keypair.clone())?;
            packet.verify(&datagram, &crypto)?;

            packet.datagram = b"nope".to_vec();
            packet.verify(&datagram, &crypto).expect_err("nope");
            Ok::<(), proto::Error>(())
        })
        .expect("sign and verify");
    }
}
