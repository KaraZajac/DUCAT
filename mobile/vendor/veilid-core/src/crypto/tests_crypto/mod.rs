mod test_crypto_aead;
mod test_crypto_dh;
mod test_crypto_generation;
mod test_crypto_hpke;
mod test_crypto_no_auth;
mod test_crypto_sign;
mod test_envelope_receipt;
mod test_types;

// 0, 1, some, many, practical-limit data sizes for byte-oriented crypto tests
pub(crate) const TEST_DATA_SIZES: [usize; 5] = [0, 1, 127, 65536, 1048576];

// A kind no cryptosystem has, for wrong-kind error tests
pub(crate) const CRYPTO_KIND_FAKE: CryptoKind = CryptoKind::new(*b"FAKE");

pub mod mocks;
pub use mocks::*;

use super::*;
use crate::tests::*;

async fn crypto_tests_startup() -> VeilidAPI {
    trace!("crypto_tests: starting");
    let (update_callback, config) = fixture_veilid_core();

    api_startup(update_callback, config)
        .await
        .expect("startup failed")
}

async fn crypto_tests_shutdown(api: VeilidAPI) {
    trace!("crypto_tests: shutting down");
    api.shutdown().await;
    trace!("crypto_tests: finished");
}

pub async fn test_all() {
    test_types::test_all().await;
    test_crypto_aead::test_all().await;
    test_crypto_no_auth::test_all().await;
    test_crypto_dh::test_all().await;
    test_crypto_sign::test_all().await;
    test_crypto_generation::test_all().await;
    test_crypto_hpke::test_all().await;
    test_envelope_receipt::test_all().await;
}
