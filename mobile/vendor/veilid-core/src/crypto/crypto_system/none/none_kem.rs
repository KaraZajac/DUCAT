use ::hpke::hybrid_array::typenum::U32;
use ::hpke::hybrid_array::Array;
use ::hpke::kem::SharedSecret;
use ::hpke::rand_core::CryptoRng;
use ::hpke::{Deserializable, HpkeError, Kem as KemTrait, Serializable};
use subtle::{Choice, ConstantTimeEq};

const NONE_KEM_KEY_LENGTH: usize = 32;

macro_rules! none_kem_key {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub(crate) struct $name([u8; NONE_KEM_KEY_LENGTH]);

        impl Serializable for $name {
            type OutputSize = U32;
            fn write_exact(&self, buf: &mut [u8]) {
                buf.copy_from_slice(&self.0);
            }
        }
        impl Deserializable for $name {
            fn from_bytes(encoded: &[u8]) -> Result<Self, HpkeError> {
                Ok(Self(encoded.try_into().map_err(|_| {
                    HpkeError::IncorrectInputLength(NONE_KEM_KEY_LENGTH, encoded.len())
                })?))
            }
        }
    };
}

none_kem_key!(NoneKemPublicKey);
none_kem_key!(NoneKemPrivateKey);
none_kem_key!(NoneKemEncappedKey);

impl ConstantTimeEq for NoneKemPrivateKey {
    fn ct_eq(&self, other: &Self) -> Choice {
        self.0.ct_eq(&other.0)
    }
}

/// Fake KEM over the NONE xor scheme. Exercises the seam's Kem type parameter, the same swap
/// VLD1 will make with a post-quantum KEM.
pub(crate) struct NoneKem;

impl KemTrait for NoneKem {
    type PublicKey = NoneKemPublicKey;
    type PrivateKey = NoneKemPrivateKey;
    type EncappedKey = NoneKemEncappedKey;
    type NSecret = U32;

    // Private-use codepoint, not an IANA KEM id
    const KEM_ID: u16 = 0xFFFF;

    fn sk_to_pk(sk: &Self::PrivateKey) -> Self::PublicKey {
        let mut pk = [0u8; NONE_KEM_KEY_LENGTH];
        for n in 0..NONE_KEM_KEY_LENGTH {
            pk[n] = !sk.0[n];
        }
        NoneKemPublicKey(pk)
    }

    fn derive_keypair(ikm: &[u8]) -> (Self::PrivateKey, Self::PublicKey) {
        let mut sk = [0u8; NONE_KEM_KEY_LENGTH];
        let len = ikm.len().min(NONE_KEM_KEY_LENGTH);
        sk[..len].copy_from_slice(&ikm[..len]);
        let sk = NoneKemPrivateKey(sk);
        let pk = Self::sk_to_pk(&sk);
        (sk, pk)
    }

    fn encap_with_rng(
        pk_recip: &Self::PublicKey,
        _sender_id_keypair: Option<(&Self::PrivateKey, &Self::PublicKey)>,
        csprng: &mut impl CryptoRng,
    ) -> Result<(SharedSecret<Self>, Self::EncappedKey), HpkeError> {
        // Mirror VLD0's low-order-point rejection
        if pk_recip.0 == [0u8; NONE_KEM_KEY_LENGTH] {
            return Err(HpkeError::EncapError);
        }
        let mut enc = [0u8; NONE_KEM_KEY_LENGTH];
        csprng.fill_bytes(&mut enc);
        let mut ss = Array::<u8, U32>::default();
        for n in 0..NONE_KEM_KEY_LENGTH {
            ss[n] = enc[n] ^ pk_recip.0[n];
        }
        Ok((SharedSecret(ss), NoneKemEncappedKey(enc)))
    }

    fn decap(
        sk_recip: &Self::PrivateKey,
        _pk_sender_id: Option<&Self::PublicKey>,
        encapped_key: &Self::EncappedKey,
    ) -> Result<SharedSecret<Self>, HpkeError> {
        // Mirror VLD0's low-order-point rejection
        if encapped_key.0 == [0u8; NONE_KEM_KEY_LENGTH] {
            return Err(HpkeError::DecapError);
        }
        let pk = Self::sk_to_pk(sk_recip);
        let mut ss = Array::<u8, U32>::default();
        for n in 0..NONE_KEM_KEY_LENGTH {
            ss[n] = encapped_key.0[n] ^ pk.0[n];
        }
        Ok(SharedSecret(ss))
    }
}
