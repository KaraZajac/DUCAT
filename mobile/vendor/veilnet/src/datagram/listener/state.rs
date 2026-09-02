use std::fmt::Display;

use tracing::warn;

use crate::{
    connection::{self},
    datagram::listener::error::{Error, Result},
};

#[derive(Debug, Clone, Copy)]
pub enum State {
    Unknown,
    Healthy,
    NeedRebind { attempts: u32 },
    NeedReset,
    Fault { attempts: u32 },
    Unrecoverable,
}

const MAX_ATTEMPTS: u32 = 5;

impl State {
    pub fn next<T>(self, result: Result<T>) -> State {
        if let State::Unrecoverable = self {
            return self;
        }
        match &result {
            Ok(_) => State::Healthy,
            Err(err) => self.next_err(err),
        }
    }

    pub fn next_err(self, err: &Error) -> State {
        warn!(?err);
        if let State::Unrecoverable = self {
            return self;
        }
        let next = match err {
            Error::DeadRoute => State::NeedRebind {
                attempts: self.attempts() + 1,
            },
            Error::BindFailure(_) => State::NeedRebind {
                attempts: self.attempts() + 1,
            },
            Error::Connection(connection::Error::Unrecoverable(_)) => State::Unrecoverable,
            Error::Watcher(_) => State::Unrecoverable,
            Error::Connection(connection::Error::Down(_)) => State::NeedReset,
            _ => State::Fault {
                attempts: self.attempts() + 1,
            },
        };
        if next.attempts() > MAX_ATTEMPTS {
            State::NeedReset
        } else {
            next
        }
    }

    pub fn attempts(&self) -> u32 {
        match self {
            State::NeedRebind { attempts } => *attempts,
            State::Fault { attempts } => *attempts,
            _ => 0,
        }
    }

    pub fn need_rebind(&self) -> bool {
        matches!(self, State::NeedRebind { .. })
    }

    pub fn need_reset(&self) -> bool {
        matches!(self, State::NeedReset)
    }

    pub fn unrecoverable(&self) -> bool {
        matches!(self, State::Unrecoverable)
    }
}

impl Default for State {
    fn default() -> Self {
        Self::Unknown
    }
}

impl Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            State::Unknown => write!(f, "unknown"),
            State::Healthy => write!(f, "healthy"),
            State::NeedRebind { attempts } => write!(f, "need rebind (attempt {attempts})"),
            State::NeedReset => write!(f, "need reset"),
            State::Fault { attempts } => write!(f, "fault (attempt {attempts})"),
            State::Unrecoverable => write!(f, "unrecoverable"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto;
    use veilid_core::VeilidAPIError;

    fn make_test_api_error() -> VeilidAPIError {
        VeilidAPIError::NoConnection {
            message: "test error".into(),
        }
    }

    #[test]
    fn test_state_defaults() {
        assert_eq!(State::default().to_string(), "unknown");
    }

    #[test]
    fn test_state_transitions_success() {
        let state = State::Unknown;
        let result: Result<()> = Ok(());
        assert_eq!(state.next(result).to_string(), "healthy");
    }

    #[test]
    fn test_state_transitions_dead_route() {
        let state = State::Healthy;
        let err = Error::DeadRoute;
        assert_eq!(state.next_err(&err).to_string(), "need rebind (attempt 1)");
    }

    #[test]
    fn test_state_transitions_bind_failure() {
        let state = State::Healthy;
        let err = Error::BindFailure(make_test_api_error());
        assert_eq!(state.next_err(&err).to_string(), "need rebind (attempt 1)");
    }

    #[test]
    fn test_state_transitions_unrecoverable() {
        let state = State::Healthy;
        let err = Error::Connection(connection::Error::Unrecoverable(make_test_api_error()));
        assert_eq!(state.next_err(&err).to_string(), "unrecoverable");

        // Watcher error also leads to unrecoverable
        let state = State::Healthy;
        let err = Error::Watcher(anyhow::Error::msg("recv error").into_boxed_dyn_error());
        assert_eq!(state.next_err(&err).to_string(), "unrecoverable");

        // Unrecoverable state should stay unrecoverable
        let state = State::Unrecoverable;
        let err = Error::DeadRoute;
        assert_eq!(state.next_err(&err).to_string(), "unrecoverable");
        assert_eq!(state.next(Ok(())).to_string(), "unrecoverable");
    }

    #[test]
    fn test_state_transitions_connection_down() {
        let state = State::Healthy;
        let err = Error::Connection(connection::Error::Down(make_test_api_error()));
        assert_eq!(state.next_err(&err).to_string(), "need reset");
    }

    #[test]
    fn test_state_transitions_fault() {
        let state = State::Healthy;
        let err = Error::Protocol(proto::Error::MessageTooLarge {
            length: 1000,
            limit: 100,
        });
        assert_eq!(state.next_err(&err).to_string(), "fault (attempt 1)");
    }

    #[test]
    fn test_max_attempts_reset() {
        let mut state = State::Healthy;
        let err = Error::Protocol(proto::Error::MessageTooLarge {
            length: 1000,
            limit: 100,
        });

        // Apply error multiple times to exceed MAX_ATTEMPTS
        for _ in 0..MAX_ATTEMPTS {
            state = state.next_err(&err);
        }
        assert_eq!(state.next_err(&err).to_string(), "need reset");
    }

    #[test]
    fn test_state_predicates() {
        let state = State::NeedRebind { attempts: 1 };
        assert!(state.need_rebind());
        assert!(!state.need_reset());
        assert!(!state.unrecoverable());

        let state = State::NeedReset;
        assert!(!state.need_rebind());
        assert!(state.need_reset());
        assert!(!state.unrecoverable());

        let state = State::Unrecoverable;
        assert!(!state.need_rebind());
        assert!(!state.need_reset());
        assert!(state.unrecoverable());
    }

    #[test]
    fn test_state_attempts() {
        let state = State::NeedRebind { attempts: 3 };
        assert_eq!(state.attempts(), 3);

        let state = State::Fault { attempts: 3 };
        assert_eq!(state.attempts(), 3);

        let state = State::Healthy;
        assert_eq!(state.attempts(), 0);
    }
}
