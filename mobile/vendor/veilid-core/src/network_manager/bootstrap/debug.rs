use super::*;

impl_veilid_log_facility!("net");

impl NetworkManager {
    #[cfg_attr(not(feature = "debug-api"), expect(dead_code))]
    pub async fn debug_info_txtrecord(&self, signing_key_pairs: KeyPairGroup) -> String {
        let routing_table = self.routing_table();

        let dial_info_details = routing_table.dial_info_details(RoutingDomain::PublicInternet);
        if dial_info_details.is_empty() {
            return "No PublicInternet DialInfo for TXT Record".to_owned();
        }
        let envelope_support = VALID_ENVELOPE_VERSIONS.to_vec();
        let public_keys = routing_table.public_keys();

        let mut out = "Bootstrap TXT Records:\n".to_owned();

        let bsrec = BootstrapRecord::new(
            public_keys,
            envelope_support,
            dial_info_details,
            Some(Timestamp::now().as_u64() / 1_000_000u64),
            vec![],
        );

        let dial_info_converter = BootstrapDialInfoConverter::default();

        match bsrec.to_v0_string(&dial_info_converter).await {
            Ok(v) => {
                //
                out += &format!("V0:\n{}\n", v);
            }
            Err(e) => {
                //
                out += &format!("V0 error: {}\n", e);
            }
        }

        for skp in signing_key_pairs.iter() {
            match bsrec
                .to_v1_string(self, &dial_info_converter, skp.clone())
                .await
            {
                Ok(v) => {
                    //
                    out += &format!("V1 ({}):\n{}\n", skp.kind(), v);
                }
                Err(e) => {
                    //
                    out += &format!("V1 ({}) error: {}\n", skp.kind(), e);
                }
            }
        }

        out
    }
}
