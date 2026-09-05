/// Defines a `<name>Group`: an ordered group of `CryptoTyped` values holding one entry per `CryptoKind`.
///
/// Generates kind-keyed accessors (get, add, remove, contains), slice deref, and `[a:b,c:d]` display and parsing.
macro_rules! impl_crypto_typed_group {
    ($visibility:vis $name:ident) => {
        pastey::paste! {

            #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), wasm_bindgen)]
            #[derive(Clone, Debug, Serialize, Deserialize, PartialOrd, Ord, PartialEq, Eq, Hash, Default, GetSize)]
            #[serde(from = "Vec<_>", into = "Vec<_>")]
            /// An ordered group of typed values holding at most one entry per `CryptoKind`.
            ///
            /// Entries are kept sorted by kind so the first is the most-preferred cryptosystem.
            /// Adding a value whose kind is already present replaces that entry.
            /// Derefs to a `[..]` slice, so slice methods like `first` and `iter` are available.
            pub struct [<$name Group>]
            {
                items: Vec<$name>,
            }

            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
            impl_try_from_js_value!([<$name Group>]);

            impl [<$name Group>]
            {
                /// An empty group.
                #[must_use]
                pub fn new() -> Self {
                    Self { items: Vec::new() }
                }
                /// An empty group with room preallocated for `cap` entries.
                #[must_use]
                pub fn with_capacity(cap: usize) -> Self {
                    Self {
                        items: Vec::with_capacity(cap),
                    }
                }

                /// Iterates the entries in kind-sorted order.
                pub fn iter(&self) -> core::slice::Iter<'_, $name> {
                    self.items.iter()
                }

                /// Adds or replaces each value by kind, then re-sorts.
                pub fn add_all_from_iter<'a>(&mut self, typed_keys: impl IntoIterator<Item = &'a $name>) {
                    'outer: for typed_key in typed_keys {
                        for x in &mut self.items {
                            if x.kind() == typed_key.kind() {
                                *x = typed_key.clone();
                                continue 'outer;
                            }
                        }
                        self.items.push(typed_key.clone());
                    }
                    self.items.sort_unstable()
                }

                /// Whether any of the given values is present in the group.
                pub fn contains_any_from_iter<'a>(&self, typed_keys: impl IntoIterator<Item = &'a $name>) -> bool {
                    typed_keys.into_iter().any(|typed_key| self.items.contains(typed_key))
                }

            }

            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
            #[wasm_bindgen]
            impl [<$name Group>]
            {
                /// The entry for the given kind, if present.
                #[must_use]
                pub fn get(&self,
                    #[wasm_bindgen(unchecked_param_type = "CryptoKind")]
                    kind: CryptoKind) -> Option<$name> {
                    self.items.iter().find(|x| x.kind() == kind).cloned()
                }

                /// Whether the group holds an entry for the given kind.
                #[must_use]
                pub fn contains_kind(&self,
                    #[wasm_bindgen(unchecked_param_type = "CryptoKind")]
                    kind: CryptoKind) -> bool {
                    self.items.iter().any(|x| x.kind() == kind)
                }

                /// Removes and returns the entry for the given kind, if present.
                pub fn remove(&mut self,
                    #[wasm_bindgen(unchecked_param_type = "CryptoKind")]
                    kind: CryptoKind) -> Option<$name> {
                    if let Some(idx) = self.items.iter().position(|x| x.kind() == kind) {
                        return Some(self.items.remove(idx));
                    }
                    None
                }

                /// Removes the entries for all of the given kinds.
                #[wasm_bindgen(js_name = "removeAll")]
                pub fn remove_all(&mut self,
                    #[wasm_bindgen(unchecked_param_type = "CryptoKind[]")]
                    kinds: Vec<CryptoKind>) {
                    for k in kinds {
                        self.remove(k);
                    }
                }
            }
            #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
            impl [<$name Group>]
            {
                /// The entry for the given kind, if present.
                #[must_use]
                pub fn get(&self, kind: CryptoKind) -> Option<$name> {
                    self.items.iter().find(|x| x.kind() == kind).cloned()
                }

                /// Whether the group holds an entry for the given kind.
                #[must_use]
                pub fn contains_kind(&self, kind: CryptoKind) -> bool {
                    self.items.iter().any(|x| x.kind() == kind)
                }

                /// Removes and returns the entry for the given kind, if present.
                pub fn remove(&mut self, kind: CryptoKind) -> Option<$name> {
                    if let Some(idx) = self.items.iter().position(|x| x.kind() == kind) {
                        return Some(self.items.remove(idx));
                    }
                    None
                }

                /// Removes the entries for all of the given kinds.
                pub fn remove_all(&mut self, kinds: Vec<CryptoKind>) {
                    for k in kinds {
                        self.remove(k);
                    }
                }
            }


            #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), wasm_bindgen)]
            impl [<$name Group>] {
                /// The kinds present in the group, sorted by cryptosystem preference.
                #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), wasm_bindgen(getter, unchecked_return_type = "CryptoKind[]"))]
                pub fn kinds(&self) -> Vec<CryptoKind> {
                    let mut out = self.items.iter().map(|tk| tk.kind()).collect::<Vec<_>>();
                    out.sort_unstable_by(compare_crypto_kind);
                    out
                }

                /// The untagged values of every entry, in kind-sorted order.
                #[must_use]
                pub fn keys(&self) -> Vec<[<Bare $name>]> {
                    self.items.iter().map(|tk| tk.value()).collect()
                }
                /// Whether the group has no entries.
                #[must_use]
                #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), wasm_bindgen(js_name = "isEmpty"))]
                pub fn is_empty(&self) -> bool {
                    self.items.is_empty()
                }
                /// The number of entries in the group.
                #[must_use]
                #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), wasm_bindgen(getter, js_name = length))]
                pub fn len(&self) -> usize {
                    self.items.len()
                }
                /// Whether the group contains the given value.
                pub fn contains(&self, typed_key: &$name) -> bool {
                    self.items.contains(typed_key)
                }

                /// Whether any of the given values is present in the group.
                #[must_use]
                #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), wasm_bindgen(js_name = containsAny))]
                pub fn contains_any(&self, typed_keys: Vec<$name>) -> bool {
                    self.contains_any_from_iter(typed_keys.iter())
                }

                /// A copy of the entries as a vector, in kind-sorted order.
                #[must_use]
                #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), wasm_bindgen(js_name = toArray))]
                pub fn to_vec(&self) -> Vec<$name> {
                    self.items.clone()
                }

                /// Adds a value, or replaces the existing entry of the same kind, then re-sorts.
                pub fn add(&mut self, typed_key: $name) {
                    for x in &mut self.items {
                        if x.kind() == typed_key.kind() {
                            *x = typed_key;
                            return;
                        }
                    }
                    self.items.push(typed_key);
                    self.items.sort_unstable()
                }

                /// Adds or replaces each value by kind, then re-sorts.
                #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), wasm_bindgen(js_name = "addAll"))]
                pub fn add_all(&mut self, typed_keys: Vec<$name>) {
                    self.add_all_from_iter(typed_keys.iter())
                }

                /// Removes all entries.
                pub fn clear(&mut self) {
                    self.items.clear();
                }


            }

            impl core::ops::Deref for [<$name Group>]
            {
                type Target = [$name];

                #[inline]
                fn deref(&self) -> &[$name] {
                    &self.items
                }
            }

            impl fmt::Display for [<$name Group>]
            {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
                    write!(f, "[")?;
                    let mut first = true;
                    for x in &self.items {
                        if first {
                            first = false;
                        } else {
                            write!(f, ",")?;
                        }
                        write!(f, "{}", x)?;
                    }
                    write!(f, "]")
                }
            }
            impl FromStr for [<$name Group>]
            {
                type Err = VeilidAPIError;
                /// Errors with `VeilidAPIError::ParseError` if `s` is not bracketed (`[..]`),
                /// or `VeilidAPIError::Generic` if any comma-separated element fails to parse.
                fn from_str(s: &str) -> Result<Self, Self::Err> {
                    let mut items = Vec::new();
                    if s.len() < 2 {
                        apibail_parse_error!("invalid length", s);
                    }
                    if &s[0..1] != "[" || &s[(s.len() - 1)..] != "]" {
                        apibail_parse_error!("invalid format", s);
                    }
                    for x in s[1..s.len() - 1].split(',') {
                        let tk = $name::from_str(x.trim())?;
                        items.push(tk);
                    }

                    Ok(Self { items })
                }
            }
            impl From<$name> for [<$name Group>]
            {
                fn from(x: $name) -> Self {
                    let mut tks = [<$name Group>]::with_capacity(1);
                    tks.add(x);
                    tks
                }
            }
            impl From<Vec<$name>> for [<$name Group>]
            {
                fn from(x: Vec<$name>) -> Self {
                    let mut tks = [<$name Group>]::with_capacity(x.len());
                    tks.add_all_from_iter(x.iter());
                    tks
                }
            }
            impl From<&[$name]> for [<$name Group>]
            {
                fn from(x: &[$name]) -> Self {
                    let mut tks = [<$name Group>]::with_capacity(x.len());
                    tks.add_all_from_iter(x.iter());
                    tks
                }
            }
            impl From<[<$name Group>]> for Vec<$name>
            {
                fn from(val: [<$name Group>]) -> Self {
                    val.items
                }
            }

            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
            #[wasm_bindgen]
            impl [<$name Group>] {
                /// An empty group.
                #[wasm_bindgen(constructor)]
                #[must_use]
                pub fn js_new() -> Self {
                    Self::new()
                }

                /// Parses a group from its bracketed, comma-separated string form.
                ///
                /// Errors with `VeilidAPIError::ParseError` if `s` is not bracketed (`[..]`),
                /// or `VeilidAPIError::Generic` if any element fails to parse.
                #[wasm_bindgen(js_name = parse)]
                pub fn js_parse(s: String) -> VeilidAPIResult<Self> {
                    Self::from_str(&s)
                }

                /// Returns the bracketed, comma-separated string form.
                #[wasm_bindgen(js_name = toString)]
                #[must_use]
                pub fn js_to_string(&self) -> String {
                    self.to_string()
                }

                /// Returns `true` if both groups hold the same items in the same order.
                #[wasm_bindgen(js_name = isEqual)]
                #[must_use]
                pub fn js_is_equal(&self, other: &Self) -> bool {
                    self == other
                }

                // TODO: add more typescript-only operations here
            }
        }
    };
}
pub(crate) use impl_crypto_typed_group;
