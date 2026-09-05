// Recovers a wasm-bindgen-exported veilid-core type from a `&JsValue` by reading its
// `__wbg_ptr`, used by the crypto-type macros to accept JS instances as parameters.

/// Implement `TryFrom<&JsValue>` for a wasm-bindgen type by reading its `__wbg_ptr`
/// and recovering a cloned instance from the wasm linear memory.
macro_rules! impl_try_from_js_value {
    ($name:ident) => {
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        impl ::core::convert::TryFrom<&::wasm_bindgen::JsValue> for $name {
            type Error = String;

            fn try_from(js: &::wasm_bindgen::JsValue) -> Result<Self, Self::Error> {
                use ::wasm_bindgen::convert::RefFromWasmAbi;

                let ptr =
                    ::js_sys::Reflect::get(js, &::wasm_bindgen::JsValue::from_str("__wbg_ptr"))
                        .map_err(|_| {
                            ::alloc::format!(
                                "expected {}, value is not an object",
                                ::core::stringify!($name)
                            )
                        })?;
                let ptr_u32: u32 = ptr.as_f64().ok_or_else(|| {
                    ::alloc::format!("expected {}, missing __wbg_ptr", ::core::stringify!($name))
                })? as u32;
                let wasm_ptr = ::wasm_bindgen::__rt::WasmPtr::<
                    ::wasm_bindgen::__rt::WasmRefCell<$name>,
                >::from_usize(ptr_u32 as usize);
                let instance_ref = unsafe { Self::ref_from_abi(wasm_ptr) };
                Ok(instance_ref.clone())
            }
        }
    };
}
pub(crate) use impl_try_from_js_value;
