//! Signatures own interpretation; VM slots preserve numeric bits and references.

pub use crate::staged::wasm::V128_BYTES;
pub use super::scalar_types::{ScalarBits, WasmReferenceKind, WasmType, WasmValue, WasmValueScalarExt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WasmSignature {
    pub params: Vec<WasmType>,
    pub results: Vec<WasmType>,
}

/// A host function's signature and, when its types name declarations, the signature pool of
/// the module whose import it satisfies, which owns them.
pub struct WasmHostSignature {
    pub signature: WasmSignature,
    pub(crate) owner: Option<std::rc::Rc<WasmSignatures>>,
}

impl std::fmt::Debug for WasmHostSignature {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.signature.fmt(formatter)
    }
}

impl std::ops::Deref for WasmHostSignature {
    type Target = WasmSignature;
    fn deref(&self) -> &WasmSignature {
        &self.signature
    }
}

impl WasmHostSignature {
    /// The declarations the signature's declared types resolve in.
    pub(crate) fn declarations(&self) -> Option<&super::WasmTypes> {
        self.owner.as_deref().map(|owner| &owner.declarations)
    }
}

/// Binary callables retain their declaration identity. Embedding callables
/// declare an implicit final singleton function type from their signature.
#[derive(Clone, Debug)]
pub enum WasmCallableType {
    Declared(u32),
    Embedded(WasmSignature),
}

#[derive(Clone)]
struct CallableSignature {
    declaration: Option<u32>,
    shape: WasmSignature,
    canonical: CanonicalSignature,
}

/// Expected import facts consumed into the canonical signature pool during lowering.
#[derive(Clone)]
pub struct WasmFunctionImport {
    pub exact: bool,
    pub ty: WasmCallableType,
    pub name: super::WasmImportName,
}

/// Frontend-validated facts and the decoded stream, consumed during lowering.
pub struct WasmFunctionBody<I> {
    pub ty: WasmCallableType,
    pub locals: Vec<wasmparser::ValType>,
    pub operators: I,
}


#[derive(Clone)]
pub struct CallableImport {
    pub name: super::WasmImportName,
    pub(crate) exact: bool,
}

mod canonical;
mod interning;
pub use canonical::CanonicalSignature;

/// One signature pool; function indices project into its type-index domain.
#[derive(Clone)]
pub struct WasmSignatures {
    pub declarations: super::WasmTypes,
    types: Vec<CallableSignature>,
    /// The type indices of `types`, by their declarations' fingerprints.
    by_fingerprint: rustc_hash::FxHashMap<Option<u64>, Vec<u32>>,
    functions: Vec<u32>,
    imports: Vec<CallableImport>,
}

impl WasmSignatures {
    pub(crate) fn with_imports(
        name: &str,
        imports: &[WasmFunctionImport],
        functions: impl IntoIterator<Item = WasmCallableType>,
        declarations: &super::WasmTypes,
    ) -> Result<Self, crate::Diagnostic> {
        let mut pool = Self {
            declarations: declarations.clone(),
            types: Vec::new(),
            by_fingerprint: Default::default(),
            functions: Vec::new(),
            imports: imports
                .iter()
                .map(|import| CallableImport {
                    name: import.name.clone(),
                    exact: import.exact,
                })
                .collect(),
        };
        for signature in imports
            .iter()
            .map(|import| import.ty.clone())
            .chain(functions)
        {
            let index = pool.intern(name, signature)?;
            pool.functions.push(index);
        }
        Ok(pool)
    }

    pub(super) fn intern(
        &mut self,
        name: &str,
        ty: WasmCallableType,
    ) -> Result<u32, crate::Diagnostic> {
        let (declaration, shape) = self.resolve(name, ty)?;
        self.interned(declaration, shape)
            .and_then(|index| u32::try_from(index).ok())
            .ok_or_else(|| crate::Diagnostic::unsupported(name, "too many Wasm signature facts"))
    }

    /// `intern`'s answer for a type the pool already holds, from a pool that is finished:
    /// a function lowered on its first call interns what the module's lowering interned.
    pub(super) fn interned_index(
        &self,
        name: &str,
        ty: WasmCallableType,
    ) -> Result<u32, crate::Diagnostic> {
        let (declaration, shape) = self.resolve(name, ty)?;
        self.find(declaration, &shape)
            .map(|index| index as u32)
            .ok_or_else(|| crate::Diagnostic::unsupported(name, "Wasm signature missing from a finished pool"))
    }

    fn resolve(
        &self,
        name: &str,
        ty: WasmCallableType,
    ) -> Result<(Option<u32>, WasmSignature), crate::Diagnostic> {
        Ok(match ty {
            WasmCallableType::Embedded(shape) => {
                if shape
                    .params
                    .iter()
                    .chain(&shape.results)
                    .any(|ty| ty.requires_declarations())
                {
                    return Err(crate::Diagnostic::unsupported(
                        name,
                        "embedded Wasm signature requires a declaration owner",
                    ));
                }
                (None, shape)
            }
            WasmCallableType::Declared(index) => {
                let function = self.declarations.function(name, index as usize)?;
                let convert = |ty| {
                    self.declarations.callable_value_type(ty).ok_or_else(|| {
                        crate::Diagnostic::unsupported(name, "unsupported Wasm callable value type")
                    })
                };
                (
                    Some(index),
                    WasmSignature {
                        params: function
                            .params()
                            .iter()
                            .copied()
                            .map(convert)
                            .collect::<Result<_, _>>()?,
                        results: function
                            .results()
                            .iter()
                            .copied()
                            .map(convert)
                            .collect::<Result<_, _>>()?,
                    },
                )
            }
        })
    }

    pub fn imports(&self) -> &[CallableImport] {
        &self.imports
    }

    pub fn defined_index(&self, function: u32) -> Option<u32> {
        self.get(function as usize)?;
        function.checked_sub(self.imports.len() as u32)
    }

    pub fn defined_signature(&self, function: u32) -> Option<&WasmSignature> {
        self.get(function as usize + self.imports.len())
    }

    pub(crate) fn defined_count(&self) -> usize {
        self.function_count() - self.imports.len()
    }

    pub(crate) fn function_count(&self) -> usize {
        self.functions.len()
    }

    pub fn get(&self, function: usize) -> Option<&WasmSignature> {
        self.type_signature(*self.functions.get(function)?)
    }

    pub fn type_signature(&self, index: u32) -> Option<&WasmSignature> {
        Some(&self.types.get(index as usize)?.shape)
    }

    pub(crate) fn function_type_index(&self, function: usize) -> Option<u32> {
        self.functions.get(function).copied()
    }

    /// Test probe: a pool function's reference type. Lowering reads `function_type_index` directly.
    #[cfg(test)]
    pub(crate) fn function_reference_type(&self, function: usize) -> Option<WasmType> {
        let fact = self
            .types
            .get(self.function_type_index(function)? as usize)?;
        Some(match fact.declaration {
            Some(index) => WasmType::Reference {
                kind: WasmReferenceKind::DeclaredFunction {
                    index,
                    exact: self.imports.get(function).is_none_or(|import| import.exact),
                },
                nullable: false,
            },
            None => WasmType::FUNC,
        })
    }

    pub(crate) fn defined_type_index(&self, function: u32) -> Option<u32> {
        self.function_type_index(function as usize + self.imports.len())
    }

    pub(crate) fn matches_declaration(
        &self,
        actual: u32,
        expected: &super::WasmTypes,
        index: u32,
        exact: bool,
    ) -> bool {
        let Some(actual) = self.types.get(actual as usize) else {
            return false;
        };
        match actual.declaration {
            Some(source) if exact => {
                self.declarations
                    .equivalent(source as usize, expected, index as usize)
            }
            Some(source) => self
                .declarations
                .is_subtype(source as usize, expected, index as usize),
            None => expected.matches_implicit_function(index as usize, &actual.shape),
        }
    }

    pub(crate) fn accepts_embedded(&self, expected: u32, actual: &WasmSignature) -> bool {
        let Some(expected) = self.types.get(expected as usize) else {
            return false;
        };
        match expected.declaration {
            Some(index) => self
                .declarations
                .matches_implicit_function(index as usize, actual),
            None => expected.shape == *actual,
        }
    }

    pub(crate) fn accepts(
        &self,
        expected_index: u32,
        actual: &Self,
        actual_index: u32,
        exact: bool,
    ) -> bool {
        let Some(expected) = self.types.get(expected_index as usize) else {
            return false;
        };
        let Some(actual_type) = actual.types.get(actual_index as usize) else {
            return false;
        };
        // Equal canonical ids are equivalent types, already a proof of compatibility (exact
        // matches included) that walks no recursive declaration graph on a hot indirect call.
        // Different ids relate only through subtyping, which declarations alone have.
        if expected.canonical == actual_type.canonical {
            return true;
        }
        match (expected.declaration, actual_type.declaration) {
            (Some(target), _) => {
                actual.matches_declaration(actual_index, &self.declarations, target, exact)
            }
            (None, Some(_)) => actual.accepts_embedded(actual_index, &expected.shape),
            (None, None) => false,
        }
    }
}

#[cfg(test)]
mod signature_pool_tests {
    use super::*;

    #[test]
    fn accepts_uses_pool_identity_only_as_a_sufficient_fast_path() {
        let i32_signature = WasmSignature {
            params: vec![WasmType::I32],
            results: vec![WasmType::I32],
        };
        let i64_signature = WasmSignature {
            params: vec![WasmType::I32],
            results: vec![WasmType::I64],
        };
        let declarations = super::super::WasmTypes::default();
        let same_pool = WasmSignatures::with_imports(
            "signature-identity",
            &[],
            [
                WasmCallableType::Embedded(i32_signature.clone()),
                WasmCallableType::Embedded(i64_signature.clone()),
            ],
            &declarations,
        )
        .unwrap();
        let structurally_equal_other_pool = WasmSignatures::with_imports(
            "signature-identity",
            &[],
            [WasmCallableType::Embedded(i32_signature)],
            &declarations,
        )
        .unwrap();
        let different_other_pool = WasmSignatures::with_imports(
            "signature-identity",
            &[],
            [WasmCallableType::Embedded(i64_signature)],
            &declarations,
        )
        .unwrap();

        assert!(same_pool.accepts(0, &same_pool, 0, true));
        assert!(!same_pool.accepts(0, &same_pool, 1, false));
        assert!(same_pool.accepts(0, &structurally_equal_other_pool, 0, false));
        assert!(!same_pool.accepts(0, &different_other_pool, 0, false));
    }

    #[test]
    fn equal_function_facts_share_one_signature_and_unused_types_do_not_become_functions() {
        let signature = WasmSignature {
            params: vec![WasmType::I32],
            results: vec![WasmType::I32],
        };
        let mut pool = WasmSignatures::with_imports(
            "pool-domains",
            &[],
            [
                WasmCallableType::Embedded(signature.clone()),
                WasmCallableType::Embedded(signature.clone()),
            ],
            &super::super::WasmTypes::default(),
        )
        .unwrap();
        assert!(std::ptr::eq(pool.get(0).unwrap(), pool.get(1).unwrap()));
        assert_eq!(
            pool.intern("pool-domains", WasmCallableType::Embedded(signature))
                .unwrap(),
            0
        );
        let unused = pool
            .intern(
                "pool-domains",
                WasmCallableType::Embedded(WasmSignature {
                    params: vec![WasmType::F64],
                    results: vec![],
                }),
            )
            .unwrap();
        assert!(pool.type_signature(unused).is_some());
        assert_eq!(pool.function_count(), 2);
        assert!(pool.get(2).is_none());
        assert!(pool.type_signature(u32::MAX).is_none());
        let reference_signature = WasmSignature {
            params: vec![WasmType::FUNCREF],
            results: vec![],
        };
        let concrete = wasmparser::RefType::new(
            true,
            wasmparser::HeapType::Concrete(wasmparser::UnpackedIndex::Module(0)),
        )
        .unwrap();
        let declarations = super::super::WasmTypes::from_functions([
            wasmparser::FuncType::new([wasmparser::ValType::Ref(wasmparser::RefType::FUNCREF)], []),
            wasmparser::FuncType::new([wasmparser::ValType::Ref(concrete)], []),
        ]);
        assert!(declarations.matches_implicit_function(0, &reference_signature));
        assert!(!declarations.matches_implicit_function(1, &reference_signature));
        let declared = WasmSignatures::with_imports(
            "pool-domains",
            &[],
            [WasmCallableType::Declared(0)],
            &declarations,
        )
        .unwrap();
        let embedded = WasmSignatures::with_imports(
            "pool-domains",
            &[],
            [WasmCallableType::Embedded(reference_signature)],
            &super::super::WasmTypes::default(),
        )
        .unwrap();
        assert!(declared.accepts(0, &embedded, 0, false));
        assert!(embedded.accepts(0, &declared, 0, false));
        let imports: Vec<_> = [false, true]
            .into_iter()
            .enumerate()
            .map(|(index, exact)| WasmFunctionImport {
                exact,
                ty: WasmCallableType::Declared(0),
                name: super::super::WasmImportName {
                    index: index as u32,
                    module: "owner".into(),
                    name: "function".into(),
                },
            })
            .collect();
        let projected = WasmSignatures::with_imports(
            "pool-domains",
            &imports,
            [WasmCallableType::Declared(0)],
            &declarations,
        )
        .unwrap();
        assert_eq!(
            projected.function_type_index(0),
            projected.function_type_index(1)
        );
        assert_eq!(
            projected.function_type_index(1),
            projected.function_type_index(2)
        );
        for (function, exact) in [false, true, true].into_iter().enumerate() {
            assert_eq!(
                projected.function_reference_type(function),
                Some(WasmType::Reference {
                    kind: WasmReferenceKind::DeclaredFunction { index: 0, exact },
                    nullable: false,
                })
            );
        }
        assert!(!declared.accepts_embedded(
            0,
            &WasmSignature {
                params: vec![WasmType::FUNC],
                results: vec![],
            }
        ));
    }
}
