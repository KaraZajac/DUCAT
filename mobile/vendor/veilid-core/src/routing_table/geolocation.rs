use crate::CountryCode;
use serde::Deserialize;
use std::net::IpAddr;
use std::sync::LazyLock;
use tracing::error;

const IPV4_MMDB: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/ipv4.mmdb"));
const IPV6_MMDB: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/ipv6.mmdb"));

static IPV4: LazyLock<Option<maxminddb::Reader<&'static [u8]>>> =
    LazyLock::new(|| match maxminddb::Reader::from_source(IPV4_MMDB) {
        Ok(reader) => Some(reader),
        Err(err) => {
            error!("Unable to open embedded IPv4 geolocation database: {}", err);
            None
        }
    });

static IPV6: LazyLock<Option<maxminddb::Reader<&'static [u8]>>> =
    LazyLock::new(|| match maxminddb::Reader::from_source(IPV6_MMDB) {
        Ok(reader) => Some(reader),
        Err(err) => {
            error!("Unable to open embedded IPv6 geolocation database: {}", err);
            None
        }
    });

#[derive(Deserialize)]
struct Country {
    country_code: CountryCode,
}

pub fn query_country_code(addr: IpAddr) -> Option<CountryCode> {
    let db = match addr {
        IpAddr::V4(_) => &*IPV4,
        IpAddr::V6(_) => &*IPV6,
    };

    let Some(db) = db else {
        return None;
    };

    let result: Country = match db.lookup(addr).and_then(|res| res.decode::<Country>()) {
        Ok(Some(result)) => result,
        Ok(None) => return None,
        Err(err) => {
            // We only expect AddressNotFoundError as possible error,
            // anything else means there's a problem
            error!("Unable to query country code: {}", err);
            return None;
        }
    };

    Some(result.country_code)
}

#[cfg(test)]
mod tests {
    use crate::CountryCode;
    use core::str::FromStr;
    use veilid_tools::*;

    #[test]
    fn test_query_country_code() {
        let test_cases = [
            ("1.2.3.4", "AU"),
            ("18.103.1.1", "US"),
            ("100.128.1.1", "US"),
            ("198.3.123.4", "US"),
            ("2001:2a0::1", "JP"),
        ];

        for (ip_str, expected_country) in test_cases {
            let ip = ip_str.parse().unwrap_or_log();
            let expected_country_code = CountryCode::from_str(expected_country).unwrap_or_log();

            let country_code = super::query_country_code(ip).unwrap_or_log();
            assert_eq!(
                country_code, expected_country_code,
                "Wrong country for {ip_str}",
            );

            eprintln!("{ip_str} -> {country_code}");
        }

        assert!(super::query_country_code("127.0.0.1".parse().unwrap_or_log()).is_none());
        assert!(super::query_country_code("10.0.0.1".parse().unwrap_or_log()).is_none());
        assert!(super::query_country_code("::1".parse().unwrap_or_log()).is_none());
    }

    #[test]
    fn test_iter_over_ipv4_mmdb() {
        let db = super::IPV4.as_ref().unwrap_or_log();

        let count = db
            .within("0.0.0.0/0".parse().unwrap_or_log(), Default::default())
            .unwrap_or_log()
            .count();

        assert!(count > 100, "Expecting some IPv4 subnets in IPv4 MMDB");
    }

    #[test]
    fn test_iter_over_ipv6_mmdb() {
        let db = super::IPV6.as_ref().unwrap_or_log();

        let count = db
            .within("::/0".parse().unwrap_or_log(), Default::default())
            .unwrap_or_log()
            .count();

        assert!(count > 100, "Expecting some IPv6 subnets in IPv6 MMDB");
    }
}
