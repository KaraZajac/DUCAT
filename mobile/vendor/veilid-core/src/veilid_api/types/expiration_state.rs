use super::*;

/// The state of a timestamp, either live (not expired) or dead (expired)
#[apply(api_data_enum!)]
#[api(eq, copy, hash, default, get_size)]
pub enum ExpirationState {
    /// Not expired.
    #[default]
    Live,
    /// Expired.
    Dead,
}

impl fmt::Display for ExpirationState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                ExpirationState::Live => "live",
                ExpirationState::Dead => "dead",
            }
        )
    }
}
