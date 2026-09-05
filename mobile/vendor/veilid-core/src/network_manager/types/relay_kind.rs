use super::*;

#[derive(
    Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Default,
)]
pub(crate) enum RelayKind {
    #[default]
    Inbound = 0,
    Outbound = 1,
}

impl fmt::Display for RelayKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            match self {
                RelayKind::Inbound => write!(f, "Inbound"),
                RelayKind::Outbound => write!(f, "Outbound"),
            }
        } else {
            match self {
                RelayKind::Inbound => write!(f, "inbound"),
                RelayKind::Outbound => write!(f, "outbound"),
            }
        }
    }
}

impl FromStr for RelayKind {
    type Err = VeilidAPIError;
    fn from_str(s: &str) -> VeilidAPIResult<RelayKind> {
        let s = s.to_lowercase();
        if s == "inbound" {
            Ok(RelayKind::Inbound)
        } else if s == "outbound" {
            Ok(RelayKind::Outbound)
        } else {
            Err(VeilidAPIError::parse_error("RelayKind::from_str failed", s))
        }
    }
}
