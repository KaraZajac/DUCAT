//! Parameterized attribute macros for exported API types.
//! One `#[apply(api_data_struct!)]` or `#[apply(api_data_enum!)]` per item, with an
//! optional `#[api(...)]` parameter list immediately after it (consumed, never expanded).
//! Base attrs are always emitted: Clone/Debug/Serialize/Deserialize, camelCase rename
//! under `json-camel-case` (structs only), JsonSchema under `schemars`, `#[must_use]`.
//! Params: `eq`, `copy`, `ord`, `hash`, `default`, `get_size`, `ts` (bare Tsify on wasm),
//! `ts(<args>)` (Tsify + `tsify(<args>)` verbatim). Extra one-off derives still work as
//! a separate `#[derive(...)]` line after `#[api(...)]`.

macro_rules! api_data_struct {
    ( @munch [] [$($acc:tt)*] $($item:tt)* ) => { $($acc)* $($item)* };
    ( @munch [eq $(, $($rest:tt)*)?] [$($acc:tt)*] $($item:tt)* ) => {
        api_data_struct! { @munch [$($($rest)*)?] [$($acc)* #[derive(PartialEq, Eq)]] $($item)* }
    };
    ( @munch [copy $(, $($rest:tt)*)?] [$($acc:tt)*] $($item:tt)* ) => {
        api_data_struct! { @munch [$($($rest)*)?] [$($acc)* #[derive(Copy)]] $($item)* }
    };
    ( @munch [ord $(, $($rest:tt)*)?] [$($acc:tt)*] $($item:tt)* ) => {
        api_data_struct! { @munch [$($($rest)*)?] [$($acc)* #[derive(PartialOrd, Ord)]] $($item)* }
    };
    ( @munch [hash $(, $($rest:tt)*)?] [$($acc:tt)*] $($item:tt)* ) => {
        api_data_struct! { @munch [$($($rest)*)?] [$($acc)* #[derive(Hash)]] $($item)* }
    };
    ( @munch [default $(, $($rest:tt)*)?] [$($acc:tt)*] $($item:tt)* ) => {
        api_data_struct! { @munch [$($($rest)*)?] [$($acc)* #[derive(Default)]] $($item)* }
    };
    ( @munch [get_size $(, $($rest:tt)*)?] [$($acc:tt)*] $($item:tt)* ) => {
        api_data_struct! { @munch [$($($rest)*)?] [$($acc)* #[derive(::get_size2::GetSize)]] $($item)* }
    };
    ( @munch [ts($($tsargs:tt)*) $(, $($rest:tt)*)?] [$($acc:tt)*] $($item:tt)* ) => {
        api_data_struct! { @munch [$($($rest)*)?] [$($acc)*
            #[cfg_attr(
                all(target_arch = "wasm32", target_os = "unknown"),
                derive(::tsify::Tsify),
                tsify($($tsargs)*)
            )]
        ] $($item)* }
    };
    ( @munch [ts $(, $($rest:tt)*)?] [$($acc:tt)*] $($item:tt)* ) => {
        api_data_struct! { @munch [$($($rest)*)?] [$($acc)*
            #[cfg_attr(
                all(target_arch = "wasm32", target_os = "unknown"),
                derive(::tsify::Tsify)
            )]
        ] $($item)* }
    };
    ( @munch [$other:tt $(, $($rest:tt)*)?] [$($acc:tt)*] $($item:tt)* ) => {
        compile_error!(concat!("api_data_struct: unknown parameter `", stringify!($other), "`"));
    };

    ( @seed [$($params:tt)*] [$($peeled:tt)*] $($item:tt)* ) => {
        api_data_struct! { @munch [$($params)*] [
            $($peeled)*
            #[derive(Clone, Debug, ::serde::Serialize, ::serde::Deserialize)]
            #[cfg_attr(feature = "json-camel-case", serde(rename_all = "camelCase"))]
            #[cfg_attr(feature = "schemars", derive(::schemars::JsonSchema))]
            #[must_use]
        ] $($item)* }
    };
    // peel forwarded attrs (docs etc.) one at a time until #[api(...)] or the item
    ( @peel [$($peeled:tt)*] #[api($($params:tt)*)] $($item:tt)* ) => {
        api_data_struct! { @seed [$($params)*] [$($peeled)*] $($item)* }
    };
    ( @peel [$($peeled:tt)*] #[$next:meta] $($item:tt)* ) => {
        api_data_struct! { @peel [$($peeled)* #[$next]] $($item)* }
    };
    ( @peel [$($peeled:tt)*] $($item:tt)* ) => {
        api_data_struct! { @seed [] [$($peeled)*] $($item)* }
    };

    ( #[api($($params:tt)*)] $($item:tt)* ) => {
        api_data_struct! { @seed [$($params)*] [] $($item)* }
    };
    ( #[$first:meta] $($item:tt)* ) => {
        api_data_struct! { @peel [#[$first]] $($item)* }
    };
    ( $($item:tt)* ) => {
        api_data_struct! { @seed [] [] $($item)* }
    };
}
pub(crate) use api_data_struct;

macro_rules! api_data_enum {
    ( @munch [] [$($acc:tt)*] $($item:tt)* ) => { $($acc)* $($item)* };
    ( @munch [eq $(, $($rest:tt)*)?] [$($acc:tt)*] $($item:tt)* ) => {
        api_data_enum! { @munch [$($($rest)*)?] [$($acc)* #[derive(PartialEq, Eq)]] $($item)* }
    };
    ( @munch [copy $(, $($rest:tt)*)?] [$($acc:tt)*] $($item:tt)* ) => {
        api_data_enum! { @munch [$($($rest)*)?] [$($acc)* #[derive(Copy)]] $($item)* }
    };
    ( @munch [ord $(, $($rest:tt)*)?] [$($acc:tt)*] $($item:tt)* ) => {
        api_data_enum! { @munch [$($($rest)*)?] [$($acc)* #[derive(PartialOrd, Ord)]] $($item)* }
    };
    ( @munch [hash $(, $($rest:tt)*)?] [$($acc:tt)*] $($item:tt)* ) => {
        api_data_enum! { @munch [$($($rest)*)?] [$($acc)* #[derive(Hash)]] $($item)* }
    };
    ( @munch [default $(, $($rest:tt)*)?] [$($acc:tt)*] $($item:tt)* ) => {
        api_data_enum! { @munch [$($($rest)*)?] [$($acc)* #[derive(Default)]] $($item)* }
    };
    ( @munch [get_size $(, $($rest:tt)*)?] [$($acc:tt)*] $($item:tt)* ) => {
        api_data_enum! { @munch [$($($rest)*)?] [$($acc)* #[derive(::get_size2::GetSize)]] $($item)* }
    };
    ( @munch [ts($($tsargs:tt)*) $(, $($rest:tt)*)?] [$($acc:tt)*] $($item:tt)* ) => {
        api_data_enum! { @munch [$($($rest)*)?] [$($acc)*
            #[cfg_attr(
                all(target_arch = "wasm32", target_os = "unknown"),
                derive(::tsify::Tsify),
                tsify($($tsargs)*)
            )]
        ] $($item)* }
    };
    ( @munch [ts $(, $($rest:tt)*)?] [$($acc:tt)*] $($item:tt)* ) => {
        api_data_enum! { @munch [$($($rest)*)?] [$($acc)*
            #[cfg_attr(
                all(target_arch = "wasm32", target_os = "unknown"),
                derive(::tsify::Tsify)
            )]
        ] $($item)* }
    };
    ( @munch [$other:tt $(, $($rest:tt)*)?] [$($acc:tt)*] $($item:tt)* ) => {
        compile_error!(concat!("api_data_enum: unknown parameter `", stringify!($other), "`"));
    };

    ( @seed [$($params:tt)*] [$($peeled:tt)*] $($item:tt)* ) => {
        api_data_enum! { @munch [$($params)*] [
            $($peeled)*
            #[derive(Clone, Debug, ::serde::Serialize, ::serde::Deserialize)]
            #[cfg_attr(feature = "schemars", derive(::schemars::JsonSchema))]
            #[must_use]
        ] $($item)* }
    };
    // peel forwarded attrs (docs etc.) one at a time until #[api(...)] or the item
    ( @peel [$($peeled:tt)*] #[api($($params:tt)*)] $($item:tt)* ) => {
        api_data_enum! { @seed [$($params)*] [$($peeled)*] $($item)* }
    };
    ( @peel [$($peeled:tt)*] #[$next:meta] $($item:tt)* ) => {
        api_data_enum! { @peel [$($peeled)* #[$next]] $($item)* }
    };
    ( @peel [$($peeled:tt)*] $($item:tt)* ) => {
        api_data_enum! { @seed [] [$($peeled)*] $($item)* }
    };

    ( #[api($($params:tt)*)] $($item:tt)* ) => {
        api_data_enum! { @seed [$($params)*] [] $($item)* }
    };
    ( #[$first:meta] $($item:tt)* ) => {
        api_data_enum! { @peel [#[$first]] $($item)* }
    };
    ( $($item:tt)* ) => {
        api_data_enum! { @seed [] [] $($item)* }
    };
}
pub(crate) use api_data_enum;
