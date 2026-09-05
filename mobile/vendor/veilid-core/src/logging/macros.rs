/// Sets the default log facility (tracing target) for the current module.
/// The `veilid_log!` macros emit to this facility unless given an explicit `target:`.
macro_rules! impl_veilid_log_facility {
    ($facility:literal) => {
        const __VEILID_LOG_FACILITY: &'static str = $facility;
    };
}
pub(crate) use impl_veilid_log_facility;

/// Builds a closure that logs a `VeilidAPIError` at the level reported by its `log_level()`.
macro_rules! log_veilid_api_error {
    ($self_expr:ident) => {
        |e: &$crate::VeilidAPIError| {
            match e.log_level() {
                $crate::Level::ERROR => {
                    veilid_log!($self_expr error "error = {}", e);
                }
                $crate::Level::WARN => {
                    veilid_log!($self_expr warn "error = {}", e);
                }
                $crate::Level::INFO => {
                    veilid_log!($self_expr info "error = {}", e);
                }
                $crate::Level::DEBUG => {
                    veilid_log!($self_expr debug "error = {}", e);
                }
                $crate::Level::TRACE => {
                    veilid_log!($self_expr trace "error = {}", e);
                }
            }
        }
    };
}
pub(crate) use log_veilid_api_error;

/// Builds a closure that logs its error argument at `ERROR` level, with an optional prefix message.
/// Useful as the argument to `Result::map_err` or `inspect_err`.
macro_rules! veilid_log_err {
    ($self_expr:expr) => {
        |e| veilid_log_event!($self_expr, prefix: "", level: $crate::Level::ERROR, "{}", e)
    };
    ($self_expr:expr, $message:expr) => {
        |e| veilid_log_event!($self_expr, prefix: "", level: $crate::Level::ERROR, "{}: {}", $message, e)
    };
    ($self_expr:expr, $fmt:expr, $($args:tt)*) => {
        |e| veilid_log_event!($self_expr, prefix: "", level: $crate::Level::ERROR, concat!($fmt,": {}"), $($args)*, e)
    };
}
pub(crate) use veilid_log_err;

/// Builds a closure that logs its error argument at `DEBUG` level in alternate (`{:#}`) form,
/// with an optional prefix message. Useful as the argument to `Result::map_err` or `inspect_err`.
macro_rules! veilid_log_dbg {
    ($self_expr:expr) => {
        |e| veilid_log_event!($self_expr, prefix: "", level: $crate::Level::DEBUG, "{:#}", e)
    };
    ($self_expr:expr, $message:expr) => {
        |e| veilid_log_event!($self_expr, prefix: "", level: $crate::Level::DEBUG, "{}: {:#}", $message, e)
    };
    ($self_expr:expr, $fmt:expr, $($args:tt)*) => {
        |e| veilid_log_event!($self_expr, prefix: "", level: $crate::Level::DEBUG, concat!($fmt,": {:#}"), $($args)*, e)
    };
}
pub(crate) use veilid_log_dbg;

/// Emits a single `tracing` event, tagging it with the log key from `$self_expr.log_key()`.
/// The facility defaults to the module's `__VEILID_LOG_FACILITY` unless a `target:` is given.
/// Prefer the `veilid_log!` wrapper; this is its underlying expansion.
macro_rules! veilid_log_event {
    // veilid_log_event!(self, prefix:"", level: Level::XXX, "message")
    ($self_expr:expr, prefix: $prefix:literal, level: $lvl:expr, $text:expr) => {event!(
        target: self::__VEILID_LOG_FACILITY,
        $lvl,
        __VEILID_LOG_KEY = $self_expr.log_key(),
        concat!($prefix,"{}"),
        $text)
    };
    // veilid_log!(self, prefix:"", level: Level::XXX, target: "facility", "message")
    ($self_expr:expr, prefix: $prefix:literal, level: $lvl:expr, target: $target:expr, $text:expr) => {event!(
        target: $target,
        $lvl,
        __VEILID_LOG_KEY = $self_expr.log_key(),
        concat!($prefix,"{}"),
        $text)
    };
    // veilid_log!(self, prefix:"", level: Level::XXX, "data: {}", data)
    ($self_expr:expr, prefix: $prefix:literal, level: $lvl:expr, $fmt:expr, $($args:tt)*) => {event!(
        target: self::__VEILID_LOG_FACILITY,
        $lvl,
        __VEILID_LOG_KEY = $self_expr.log_key(),
        concat!($prefix,$fmt),
        $($args)*)
    };
    // veilid_log!(self, prefix:"", level: Level::XXX, target: "facility", "data: {}", data)
    ($self_expr:expr, prefix: $prefix:literal, level: $lvl:expr, target: $target:expr, $fmt:expr, $($args:tt)*) => {event!(
        target: $target,
        $lvl,
        __VEILID_LOG_KEY = $self_expr.log_key(),
        concat!($prefix,$fmt),
        $($args)*)
    };
    // veilid_log!(self, prefix:"", level: Level::XXX, fields: field=value, ?other_field)
    ($self_expr:expr, prefix: $prefix:literal, level: $lvl:expr, fields: $($k:ident).+ = $($fields:tt)*) => {event!(
        target: self::__VEILID_LOG_FACILITY,
        $lvl,
        __VEILID_LOG_KEY = $self_expr.log_key(),
        $($k).+ = $($fields)*,
        concat!($prefix,""))
    };
    // veilid_log!(self, prefix:"", Level::XXX, target: "facility", fields: field=value, ?other_field)
    ($self_expr:expr, prefix: $prefix:literal, level: $lvl:expr, target: $target:expr, fields: $($k:ident).+ = $($fields:tt)*) => {event!(
        target: $target,
        $lvl,
        __VEILID_LOG_KEY = $self_expr.log_key(),
        $($k).+ = $($fields)*,
        concat!($prefix,""))
    };
}
pub(crate) use veilid_log_event;

// Warn in debug builds so it's obvious without enabling debug logging; demote to
// debug in release so it stays silent unless debug logging is turned on.
#[cfg(debug_assertions)]
pub(crate) const DEBUGWARN: tracing::Level = tracing::Level::WARN;
#[cfg(not(debug_assertions))]
pub(crate) const DEBUGWARN: tracing::Level = tracing::Level::DEBUG;

/// Emits a Veilid log event tagged with the instance's log key.
///
/// The first argument is the instance providing `log_key()`, followed by a level keyword
/// (`error`, `warn`, `info`, `debug`, `trace`, or `debugwarn`), then a message or format
/// string with arguments. An optional `target:` selects a facility other than the module
/// default, and a `fields:` form records structured fields. The `debugwarn` level prefixes
/// `DEBUGWARN:` and logs at `WARN` in debug builds, `DEBUG` in release builds.
macro_rules! veilid_log {

    // ERROR //////////////////////////////////////////////////////////////////////////
    // veilid_log!(self error "message")
    ($self_ident:ident error $text:expr) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::ERROR, $text)};
    // veilid_log!(self error target: "facility", "message")
    ($self_ident:ident error target: $target:expr, $text:expr) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::ERROR, target: $target, $text)};
    // veilid_log!(self error "data: {}", data)
    ($self_ident:ident error $fmt:expr, $($args:tt)*) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::ERROR, $fmt, $($args)*)};
    // veilid_log!(self error target: "facility", "data: {}", data)
    ($self_ident:ident error target: $target:expr, $fmt:expr, $($args:tt)*) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::ERROR, target: $target, $fmt, $($args)*)};
    // veilid_log!(self error, fields: field=value, ?other_field)
    ($self_ident:ident error, fields: $($k:ident).+ = $($fields:tt)*) => {veilid_log_event!($self_ident level: $crate::Level::ERROR, fields: $($k).+ = $($fields)*)};
    // veilid_log!(self error target: "facility", fields: field=value, ?other_field)
    ($self_ident:ident error target: $target:expr, fields: $($k:ident).+ = $($fields:tt)*) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::ERROR, target: $target, fields: $($k).+ = $($fields)*)};

    // WARN //////////////////////////////////////////////////////////////////////////
    // veilid_log!(self warn "message")
    ($self_ident:ident warn $text:expr) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::WARN, $text)};
    // veilid_log!(self warn target: "facility", "message")
    ($self_ident:ident warn target: $target:expr, $text:expr) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::WARN, target: $target, $text)};
    // veilid_log!(self warn "data: {}", data)
    ($self_ident:ident warn $fmt:expr, $($args:tt)*) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::WARN, $fmt, $($args)*)};
    // veilid_log!(self warn target: "facility", "data: {}", data)
    ($self_ident:ident warn target: $target:expr, $fmt:expr, $($args:tt)*) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::WARN, target: $target, $fmt, $($args)*)};
    // veilid_log!(self warn, fields: field=value, ?other_field)
    ($self_ident:ident warn, fields: $($k:ident).+ = $($fields:tt)*) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::WARN, fields: $($k).+ = $($fields)*)};
    // veilid_log!(self warn target: "facility", fields: field=value, ?other_field)
    ($self_ident:ident warn target: $target:expr, fields: $($k:ident).+ = $($fields:tt)*) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::WARN, target: $target, fields: $($k).+ = $($fields)*)};

    // INFO //////////////////////////////////////////////////////////////////////////
    // veilid_log!(self info "message")
    ($self_ident:ident info $text:expr) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::INFO, $text)};
    // veilid_log!(self info target: "facility", "message")
    ($self_ident:ident info target: $target:expr, $text:expr) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::INFO, target: $target, $text)};
    // veilid_log!(self info "data: {}", data)
    ($self_ident:ident info $fmt:expr, $($args:tt)*) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::INFO, $fmt, $($args)*)};
    // veilid_log!(self info target: "facility", "data: {}", data)
    ($self_ident:ident info target: $target:expr, $fmt:expr, $($args:tt)*) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::INFO, target: $target, $fmt, $($args)*)};
    // veilid_log!(self info, fields: field=value, ?other_field)
    ($self_ident:ident info, fields: $($k:ident).+ = $($fields:tt)*) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::INFO, fields: $($k).+ = $($fields)*)};
    // veilid_log!(self info target: "facility", fields: field=value, ?other_field)
    ($self_ident:ident info target: $target:expr, fields: $($k:ident).+ = $($fields:tt)*) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::INFO, target: $target, fields: $($k).+ = $($fields)*)};

    // DEBUG //////////////////////////////////////////////////////////////////////////
    // veilid_log!(self debug "message")
    ($self_ident:ident debug $text:expr) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::DEBUG, $text)};
    // veilid_log!(self debug target: "facility", "message")
    ($self_ident:ident debug target: $target:expr, $text:expr) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::DEBUG, target: $target, $text)};
    // veilid_log!(self debug "data: {}", data)
    ($self_ident:ident debug $fmt:expr, $($args:tt)*) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::DEBUG, $fmt, $($args)*)};
    // veilid_log!(self debug target: "facility", "data: {}", data)
    ($self_ident:ident debug target: $target:expr, $fmt:literal, $($arg:tt)*) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::DEBUG, target: $target, $fmt, $($arg)*)};
    // veilid_log!(self debug, fields: field=value, ?other_field)
    ($self_ident:ident debug, fields: $($k:ident).+ = $($fields:tt)*) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::DEBUG, fields: $($k).+ = $($fields)*)};
    // veilid_log!(self debug target: "facility" fields: field=value, ?other_field)
    ($self_ident:ident debug target: $target:expr, fields: $($k:ident).+ = $($fields:tt)*) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::DEBUG, target: $target, fields: $($k).+ = $($fields)*)};

    // TRACE //////////////////////////////////////////////////////////////////////////
    // veilid_log!(self trace "message")
    ($self_ident:ident trace $text:expr) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::TRACE, $text)};
    // veilid_log!(self trace target: "facility", "message")
    ($self_ident:ident trace target: $target:expr, $text:expr) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::TRACE, target: $target, $text)};
    // veilid_log!(self trace "data: {}", data)
    ($self_ident:ident trace $fmt:literal, $($arg:tt)*) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::TRACE, $fmt, $($arg)*)};
    // veilid_log!(self trace target: "facility", "data: {}", data)
    ($self_ident:ident trace target: $target:expr, $fmt:expr, $($args:tt)*) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::TRACE, target: $target, $fmt, $($args)*)};
    // veilid_log!(self trace, fields: field=value, ?other_field)
    ($self_ident:ident trace, fields: $($k:ident).+ = $($fields:tt)*) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::TRACE, fields: $($k).+ = $($fields)*)};
    // veilid_log!(self trace target: "facility", fields: field=value, ?other_field)
    ($self_ident:ident trace target: $target:expr, fields: $($k:ident).+ = $($fields:tt)*) => {veilid_log_event!($self_ident, prefix: "", level: $crate::Level::TRACE, target: $target, fields: $($k).+ = $($fields)*)};

    // DEBUGWARN //////////////////////////////////////////////////////////////////////////
    // veilid_log!(self debugwarn "message")
    ($self_ident:ident debugwarn $text:expr) => {veilid_log_event!($self_ident, prefix: "DEBUGWARN: ", level: $crate::DEBUGWARN, $text)};
    // veilid_log!(self debugwarn target: "facility", "message")
    ($self_ident:ident debugwarn target: $target:expr, $text:expr) => {veilid_log_event!($self_ident, prefix: "DEBUGWARN: ", level: $crate::DEBUGWARN, target: $target, $text)};
    // veilid_log!(self debugwarn "data: {}", data)
    ($self_ident:ident debugwarn $fmt:expr, $($args:tt)*) => {veilid_log_event!($self_ident, prefix: "DEBUGWARN: ", level: $crate::DEBUGWARN, $fmt, $($args)*)};
    // veilid_log!(self debugwarn target: "facility", "data: {}", data)
    ($self_ident:ident debugwarn target: $target:expr, $fmt:expr, $($args:tt)*) => {veilid_log_event!($self_ident, prefix: "DEBUGWARN: ", level: $crate::DEBUGWARN, target: $target, $fmt, $($args)*)};
    // veilid_log!(self debugwarn, fields: field=value, ?other_field)
    ($self_ident:ident debugwarn, fields: $($k:ident).+ = $($fields:tt)*) => {veilid_log_event!($self_ident, prefix: "DEBUGWARN", level: $crate::DEBUGWARN, fields: $($k).+ = $($fields)*)};
    // veilid_log!(self debugwarn target: "facility", fields: field=value, ?other_field)
    ($self_ident:ident debugwarn target: $target:expr, fields: $($k:ident).+ = $($fields:tt)*) => {veilid_log_event!($self_ident, prefix: "DEBUGWARN", level: $crate::DEBUGWARN, target: $target, fields: $($k).+ = $($fields)*)};
}
pub(crate) use veilid_log;

/// If an operation returns a NetworkResult:
///
/// * If it is a Value, return it.
/// * If it is any other enum variant, log it and return an alternative value in the `=>` block
///
/// If a `[ ]` block is also provided, an extra logging string provided in the block will be added to the log message
macro_rules! network_result_value_or_log {
    ($self:ident $r:expr => $f:expr) => {
        network_result_value_or_log!($self target: self::__VEILID_LOG_FACILITY, $r => [ "" ] $f )
    };
    ($self:ident $r:expr => [ $d:expr ] $f:expr) => {
        network_result_value_or_log!($self target: self::__VEILID_LOG_FACILITY, $r => [ $d ] $f )
    };
    ($self:ident target: $target:expr, $r:expr => $f:expr) => {
        network_result_value_or_log!($self target: $target, $r => [ "" ] $f )
    };
    ($self:ident target: $target:expr, $r:expr => [ $d:expr ] $f:expr) => { {
        let __extra_message = if debug_target_enabled!("network_result") {
            $d.to_string()
        } else {
            "".to_string()
        };
        match $r {
            NetworkResult::Timeout => {
                veilid_log!($self debug target: $target,
                    "{} at {}@{}:{} in {}{}",
                    "Timeout",
                    file!(),
                    line!(),
                    column!(),
                    fn_name::uninstantiated!(),
                    __extra_message
                );
                $f
            }
            NetworkResult::ServiceUnavailable(ref s) => {
                veilid_log!($self debug target: $target,
                    "{}({}) at {}@{}:{} in {}{}",
                    "ServiceUnavailable",
                    s,
                    file!(),
                    line!(),
                    column!(),
                    fn_name::uninstantiated!(),
                    __extra_message
                );
                $f
            }
            NetworkResult::NoConnection(ref e) => {
                veilid_log!($self debug target: $target,
                    "{}({}) at {}@{}:{} in {}{}",
                    "No connection",
                    e.to_string(),
                    file!(),
                    line!(),
                    column!(),
                    fn_name::uninstantiated!(),
                    __extra_message
                );
                $f
            }
            NetworkResult::AlreadyExists(ref e) => {
                veilid_log!($self debug target: $target,
                    "{}({}) at {}@{}:{} in {}{}",
                    "Already exists",
                    e.to_string(),
                    file!(),
                    line!(),
                    column!(),
                    fn_name::uninstantiated!(),
                    __extra_message
                );
                $f
            }
            NetworkResult::InvalidMessage(ref s) => {
                veilid_log!($self debug target: $target,
                    "{}({}) at {}@{}:{} in {}{}",
                    "Invalid message",
                    s,
                    file!(),
                    line!(),
                    column!(),
                    fn_name::uninstantiated!(),
                    __extra_message
                );
                $f
            }
            NetworkResult::Value(v) => v,
        }
    } };

}
pub(crate) use network_result_value_or_log;
