use super::*;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum BucketEntryStateDeadReason {
    ExcessiveUnreachable,
    ExcessiveSendFailures,
    NeverSeenLostQuestions,
    SteadyLostQuestions,
}

impl fmt::Display for BucketEntryStateDeadReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            match self {
                BucketEntryStateDeadReason::ExcessiveUnreachable => write!(f, "DUNRCH"),
                BucketEntryStateDeadReason::ExcessiveSendFailures => write!(f, "DFSEND"),
                BucketEntryStateDeadReason::NeverSeenLostQuestions => write!(f, "DNEVER"),
                BucketEntryStateDeadReason::SteadyLostQuestions => write!(f, "DLOSTQ"),
            }
        } else {
            match self {
                BucketEntryStateDeadReason::ExcessiveUnreachable => {
                    write!(f, "Excessive unreachable attempts")
                }
                BucketEntryStateDeadReason::ExcessiveSendFailures => {
                    write!(f, "Excessive send failures")
                }
                BucketEntryStateDeadReason::NeverSeenLostQuestions => {
                    write!(f, "Never seen + lost questions")
                }
                BucketEntryStateDeadReason::SteadyLostQuestions => {
                    write!(f, "Steady lost questions")
                }
            }
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum BucketEntryStateMissingReason {
    Unreachable,
    FailedToSend,
    LostQuestions,
}

impl fmt::Display for BucketEntryStateMissingReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            match self {
                BucketEntryStateMissingReason::Unreachable => write!(f, "MUNRCH"),
                BucketEntryStateMissingReason::FailedToSend => write!(f, "MFSEND"),
                BucketEntryStateMissingReason::LostQuestions => write!(f, "MLOSTQ"),
            }
        } else {
            match self {
                BucketEntryStateMissingReason::Unreachable => write!(f, "Unreachable"),
                BucketEntryStateMissingReason::FailedToSend => {
                    write!(f, "Failed to send")
                }
                BucketEntryStateMissingReason::LostQuestions => {
                    write!(f, "Lost questions")
                }
            }
        }
    }
}

// Node-level state reason.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum BucketEntryStateReason {
    Punished(PunishmentReason),
    Dead(BucketEntryStateDeadReason),
    Missing(BucketEntryStateMissingReason),
    Initial,
    Unreliable,
    Reliable,
}

impl fmt::Display for BucketEntryStateReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            match self {
                BucketEntryStateReason::Punished(p) => write!(f, "{}", f.to_string(p)),
                BucketEntryStateReason::Dead(d) => write!(f, "{}", f.to_string(d)),
                BucketEntryStateReason::Missing(m) => write!(f, "{}", f.to_string(m)),
                BucketEntryStateReason::Initial => write!(f, "INITAL"),
                BucketEntryStateReason::Unreliable => write!(f, "UNRLBL"),
                BucketEntryStateReason::Reliable => write!(f, "RELIBL"),
            }
        } else {
            match self {
                BucketEntryStateReason::Punished(p) => write!(f, "Punished({})", f.to_string(p)),
                BucketEntryStateReason::Dead(d) => write!(f, "Dead({})", f.to_string(d)),
                BucketEntryStateReason::Missing(m) => write!(f, "Missing({})", f.to_string(m)),
                BucketEntryStateReason::Initial => write!(f, "Initial"),
                BucketEntryStateReason::Unreliable => {
                    write!(f, "Unreliable")
                }
                BucketEntryStateReason::Reliable => write!(f, "Reliable"),
            }
        }
    }
}

/// Node-level state.
/// Do not change order, it will mess up other sorts
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum BucketEntryState {
    Punished,
    Dead,
    Missing,
    Initial,
    Unreliable,
    Reliable,
}

impl FromStr for BucketEntryState {
    type Err = VeilidAPIError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.to_lowercase();
        if "punished".starts_with(&s) {
            Ok(BucketEntryState::Punished)
        } else if "dead".starts_with(&s) {
            Ok(BucketEntryState::Dead)
        } else if "missing".starts_with(&s) {
            Ok(BucketEntryState::Missing)
        } else if "initial".starts_with(&s) {
            Ok(BucketEntryState::Initial)
        } else if "unreliable".starts_with(&s) {
            Ok(BucketEntryState::Unreliable)
        } else if "reliable".starts_with(&s) {
            Ok(BucketEntryState::Reliable)
        } else {
            Err(VeilidAPIError::parse_error("invalid bucket entry state", s))
        }
    }
}

impl fmt::Display for BucketEntryState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            match self {
                BucketEntryState::Punished => write!(f, "pnsh"),
                BucketEntryState::Dead => write!(f, "dead"),
                BucketEntryState::Missing => write!(f, "miss"),
                BucketEntryState::Initial => write!(f, "init"),
                BucketEntryState::Unreliable => write!(f, "urel"),
                BucketEntryState::Reliable => write!(f, "reli"),
            }
        } else {
            match self {
                BucketEntryState::Punished => write!(f, "punished"),
                BucketEntryState::Dead => write!(f, "dead"),
                BucketEntryState::Missing => write!(f, "missing"),
                BucketEntryState::Initial => write!(f, "initial"),
                BucketEntryState::Unreliable => write!(f, "unreliable"),
                BucketEntryState::Reliable => write!(f, "reliable"),
            }
        }
    }
}

impl BucketEntryState {
    /// Whether the node might be alive (is not known to be dead)
    #[must_use]
    pub fn maybe_live(&self) -> bool {
        match self {
            BucketEntryState::Punished | BucketEntryState::Dead => false,
            BucketEntryState::Missing
            | BucketEntryState::Initial
            | BucketEntryState::Unreliable
            | BucketEntryState::Reliable => true,
        }
    }

    /// Whether the node is live (is presumed or proven not dead)
    #[must_use]
    pub fn is_live(&self) -> bool {
        match self {
            BucketEntryState::Punished | BucketEntryState::Dead | BucketEntryState::Missing => {
                false
            }
            BucketEntryState::Initial
            | BucketEntryState::Unreliable
            | BucketEntryState::Reliable => true,
        }
    }

    /// Whether the node is responsive (has been seen and is responding to requests, but not necessarily reliably)
    #[must_use]
    pub fn is_responsive(&self) -> bool {
        match self {
            BucketEntryState::Punished
            | BucketEntryState::Dead
            | BucketEntryState::Missing
            | BucketEntryState::Initial => false,
            BucketEntryState::Unreliable | BucketEntryState::Reliable => true,
        }
    }

    /// Whether the node is reliable (tested continuously and responding to requests)
    #[must_use]
    pub fn is_reliable(&self) -> bool {
        match self {
            BucketEntryState::Punished
            | BucketEntryState::Dead
            | BucketEntryState::Missing
            | BucketEntryState::Initial
            | BucketEntryState::Unreliable => false,
            BucketEntryState::Reliable => true,
        }
    }
}

impl From<BucketEntryStateReason> for BucketEntryState {
    fn from(value: BucketEntryStateReason) -> Self {
        match value {
            BucketEntryStateReason::Punished(_) => BucketEntryState::Punished,
            BucketEntryStateReason::Dead(_) => BucketEntryState::Dead,
            BucketEntryStateReason::Missing(_) => BucketEntryState::Missing,
            BucketEntryStateReason::Initial => BucketEntryState::Initial,
            BucketEntryStateReason::Unreliable => BucketEntryState::Unreliable,
            BucketEntryStateReason::Reliable => BucketEntryState::Reliable,
        }
    }
}
