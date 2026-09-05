use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PunishmentReason {
    // Manually added punishment
    #[cfg_attr(
        not(any(feature = "debug-api", feature = "test-util")),
        expect(dead_code)
    )]
    Manual,
    // IP-level punishments
    FailedToDecryptEnvelopeBody,
    FailedToDecodeEnvelope,
    ShortPacket,
    InvalidFraming,
    // Node-level punishments
    FailedToDecodeOperation,
    WrongSenderPeerInfo,
    //FailedToVerifySenderPeerInfo,
    FailedToRegisterSenderPeerInfo,
    // Route-level punishments
    // FailedToDecodeRoutedMessage,
}

impl fmt::Display for PunishmentReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            match self {
                PunishmentReason::Manual => write!(f, "PMANUL"),
                PunishmentReason::FailedToDecryptEnvelopeBody => write!(f, "PCRYPT"),
                PunishmentReason::FailedToDecodeEnvelope => write!(f, "PDECEN"),
                PunishmentReason::ShortPacket => write!(f, "PSHORT"),
                PunishmentReason::InvalidFraming => write!(f, "PFRAME"),
                PunishmentReason::FailedToDecodeOperation => write!(f, "PDECOP"),
                PunishmentReason::WrongSenderPeerInfo => write!(f, "PSPBAD"),
                PunishmentReason::FailedToRegisterSenderPeerInfo => write!(f, "PSPREG"),
            }
        } else {
            match self {
                PunishmentReason::Manual => write!(f, "Manual"),
                PunishmentReason::FailedToDecryptEnvelopeBody => {
                    write!(f, "Failed to decrypt envelope body")
                }
                PunishmentReason::FailedToDecodeEnvelope => write!(f, "Failed to decode envelope"),
                PunishmentReason::ShortPacket => write!(f, "Short packet"),
                PunishmentReason::InvalidFraming => write!(f, "Invalid framing"),
                PunishmentReason::FailedToDecodeOperation => {
                    write!(f, "Failed to decode operation")
                }
                PunishmentReason::WrongSenderPeerInfo => write!(f, "Wrong sender peer info"),
                PunishmentReason::FailedToRegisterSenderPeerInfo => {
                    write!(f, "Failed to register sender peer info")
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Punishment {
    pub reason: PunishmentReason,
    pub timestamp: Timestamp,
}

impl fmt::Display for Punishment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} @ {}",
            f.to_string(self.reason),
            f.to_string(self.timestamp)
        )
    }
}
