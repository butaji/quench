//! Signatures own interpretation; VM slots preserve numeric bits and references.

use crate::bytecode::Constant;

pub(crate) const V128_BYTES: usize = std::mem::size_of::<u128>();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmType {
    I32,
    I64,
    F32,
    F64,
    V128,
    Reference {
        kind: WasmReferenceKind,
        nullable: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmReferenceKind {
    Function,
    External,
    DeclaredFunction { index: u32, exact: bool },
    DeclaredGc { index: u32, exact: bool },
    Internal(wasmparser::AbstractHeapType),
    BottomFunction,
    BottomExternal,
}

impl WasmType {
    pub const FUNCREF: Self = Self::Reference {
        kind: WasmReferenceKind::Function,
        nullable: true,
    };
    pub const EXTERNREF: Self = Self::Reference {
        kind: WasmReferenceKind::External,
        nullable: true,
    };
    pub const FUNC: Self = Self::Reference {
        kind: WasmReferenceKind::Function,
        nullable: false,
    };
    pub const EXTERN: Self = Self::Reference {
        kind: WasmReferenceKind::External,
        nullable: false,
    };

    pub(crate) fn requires_declarations(self) -> bool {
        matches!(
            self,
            Self::Reference {
                kind: WasmReferenceKind::DeclaredFunction { .. }
                    | WasmReferenceKind::DeclaredGc { .. },
                ..
            }
        )
    }

    /// Exact embedding type projection; declared identities stay in WasmTypes.
    pub(crate) fn value_type(self) -> wasmparser::ValType {
        match self {
            Self::I32 => wasmparser::ValType::I32,
            Self::I64 => wasmparser::ValType::I64,
            Self::F32 => wasmparser::ValType::F32,
            Self::F64 => wasmparser::ValType::F64,
            Self::V128 => wasmparser::ValType::V128,
            Self::Reference { .. } => wasmparser::ValType::Ref(self.reference_type().unwrap()),
        }
    }

    pub(crate) fn reference_type(self) -> Option<wasmparser::RefType> {
        let Self::Reference { kind, nullable } = self else {
            return None;
        };
        let ty = match kind {
            WasmReferenceKind::Function => wasmparser::RefType::FUNC,
            WasmReferenceKind::External => wasmparser::RefType::EXTERN,
            WasmReferenceKind::BottomFunction => wasmparser::RefType::NOFUNC,
            WasmReferenceKind::BottomExternal => wasmparser::RefType::NOEXTERN,
            WasmReferenceKind::Internal(ty) => wasmparser::RefType::new(
                false,
                wasmparser::HeapType::Abstract { shared: false, ty },
            )
            .unwrap(),
            WasmReferenceKind::DeclaredFunction { index, exact }
            | WasmReferenceKind::DeclaredGc { index, exact } => {
                let index = wasmparser::UnpackedIndex::Module(index);
                let heap = if exact {
                    wasmparser::HeapType::Exact(index)
                } else {
                    wasmparser::HeapType::Concrete(index)
                };
                wasmparser::RefType::new(false, heap).unwrap()
            }
        };
        Some(if nullable { ty.nullable() } else { ty })
    }

    pub(crate) fn is_subtype_of(self, target: Self) -> bool {
        let declarations = super::WasmTypes::default();
        declarations.value_subtype(self, &declarations, target)
    }

    pub fn from_wasm(ty: wasmparser::ValType) -> Option<Self> {
        match ty {
            wasmparser::ValType::I32 => Some(Self::I32),
            wasmparser::ValType::I64 => Some(Self::I64),
            wasmparser::ValType::F32 => Some(Self::F32),
            wasmparser::ValType::F64 => Some(Self::F64),
            wasmparser::ValType::V128 => Some(Self::V128),
            wasmparser::ValType::Ref(ty) => {
                let kind = match ty.heap_type() {
                    wasmparser::HeapType::FUNC => WasmReferenceKind::Function,
                    wasmparser::HeapType::EXTERN => WasmReferenceKind::External,
                    wasmparser::HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::NoFunc,
                    } => WasmReferenceKind::BottomFunction,
                    wasmparser::HeapType::Abstract {
                        shared: false,
                        ty: wasmparser::AbstractHeapType::NoExtern,
                    } => WasmReferenceKind::BottomExternal,
                    wasmparser::HeapType::Abstract {
                        shared: false,
                        ty:
                            ty @ (wasmparser::AbstractHeapType::Any
                            | wasmparser::AbstractHeapType::Eq
                            | wasmparser::AbstractHeapType::Struct
                            | wasmparser::AbstractHeapType::Array
                            | wasmparser::AbstractHeapType::I31
                            | wasmparser::AbstractHeapType::None
                            | wasmparser::AbstractHeapType::Exn
                            | wasmparser::AbstractHeapType::NoExn),
                    } => WasmReferenceKind::Internal(ty),
                    _ => return None,
                };
                Some(Self::Reference {
                    kind,
                    nullable: ty.is_nullable(),
                })
            }
        }
    }

    pub(crate) fn default_value(self) -> Option<WasmValue> {
        Some(match self {
            Self::I32 => WasmValue::I32(0),
            Self::I64 => WasmValue::I64(0),
            Self::F32 => WasmValue::F32(0),
            Self::F64 => WasmValue::F64(0),
            Self::V128 => WasmValue::V128(0),
            Self::Reference {
                nullable: false, ..
            } => return None,
            Self::Reference {
                kind:
                    WasmReferenceKind::Function
                    | WasmReferenceKind::DeclaredFunction { .. }
                    | WasmReferenceKind::BottomFunction,
                nullable: true,
            } => WasmValue::FuncRef(crate::Value::NULL),
            Self::Reference {
                kind: WasmReferenceKind::External | WasmReferenceKind::BottomExternal,
                nullable: true,
            } => WasmValue::ExternRef(crate::Value::NULL),
            Self::Reference {
                kind: WasmReferenceKind::Internal(_) | WasmReferenceKind::DeclaredGc { .. },
                nullable: true,
            } => WasmValue::GcRef(crate::Value::NULL),
        })
    }
}

/// Floating-point variants contain IEEE bits, preserving NaN payloads and -0.
/// Reference variants hold values belonging to the invoking Runtime. Root a
/// returned heap reference with Runtime::root before subsequent VM work, and
/// keep that root alive while retaining or passing it back to the runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmValue {
    I32(i32),
    I64(i64),
    F32(u32),
    F64(u64),
    V128(u128),
    /// Shared VM references follow the Runtime Value rooting contract.
    FuncRef(crate::Value),
    ExternRef(crate::Value),
    GcRef(crate::Value),
}

impl WasmValue {
    pub(crate) fn fits_type(self, ty: WasmType) -> bool {
        let WasmType::Reference { kind, nullable } = ty else {
            return self.ty() == ty;
        };
        let value = match (self, kind) {
            (
                Self::FuncRef(value),
                WasmReferenceKind::Function | WasmReferenceKind::DeclaredFunction { .. },
            ) => value,
            (Self::ExternRef(value), WasmReferenceKind::External) => value,
            (
                Self::GcRef(value),
                WasmReferenceKind::Internal(wasmparser::AbstractHeapType::None),
            )
            | (Self::FuncRef(value), WasmReferenceKind::BottomFunction)
            | (Self::ExternRef(value), WasmReferenceKind::BottomExternal) => {
                return nullable && value.is_null();
            }
            (
                Self::GcRef(value),
                WasmReferenceKind::Internal(_) | WasmReferenceKind::DeclaredGc { .. },
            ) => value,
            _ => return false,
        };
        !value.is_null() || nullable
    }
    pub fn ty(self) -> WasmType {
        match self {
            Self::I32(_) => WasmType::I32,
            Self::I64(_) => WasmType::I64,
            Self::F32(_) => WasmType::F32,
            Self::F64(_) => WasmType::F64,
            Self::V128(_) => WasmType::V128,
            Self::FuncRef(_) => WasmType::FUNCREF,
            Self::ExternRef(_) => WasmType::EXTERNREF,
            Self::GcRef(value) if super::i31::bits(value).is_some() => super::i31::REFERENCE_TYPE,
            Self::GcRef(_) => WasmType::Reference {
                kind: WasmReferenceKind::Internal(wasmparser::AbstractHeapType::Any),
                nullable: true,
            },
        }
    }

    pub fn is_canonical_nan(self) -> bool {
        self.nan_bits()
            .is_some_and(|(bits, canonical, sign)| bits & !sign == canonical)
    }

    pub fn is_arithmetic_nan(self) -> bool {
        self.nan_bits()
            .is_some_and(|(bits, canonical, _)| bits & canonical == canonical)
    }

    fn nan_bits(self) -> Option<(u64, u64, u64)> {
        use super::numeric::{canonical_nan_bits, sign_mask};
        match self {
            Self::F32(bits) => Some((
                u64::from(bits),
                canonical_nan_bits(f32::MANTISSA_DIGITS, u64::from(f32::INFINITY.to_bits())),
                sign_mask(u32::BITS),
            )),
            Self::F64(bits) => Some((
                bits,
                canonical_nan_bits(f64::MANTISSA_DIGITS, f64::INFINITY.to_bits()),
                sign_mask(u64::BITS),
            )),
            _ => None,
        }
    }

    pub(crate) fn bits(self) -> Option<ScalarBits> {
        Some(match self {
            Self::I32(value) => ScalarBits::Bits32(value as u32),
            Self::F32(bits) => ScalarBits::Bits32(bits),
            Self::I64(value) => ScalarBits::Bits64(value as u64),
            Self::F64(bits) => ScalarBits::Bits64(bits),
            Self::V128(bits) => ScalarBits::Bits128(bits.to_le_bytes()),
            Self::FuncRef(_) | Self::ExternRef(_) | Self::GcRef(_) => return None,
        })
    }

    pub(super) fn constant(self) -> Option<Constant> {
        match self {
            Self::GcRef(value) if super::i31::bits(value).is_some() => Some(Constant::Number(
                f64::from(super::i31::bits(value).unwrap()),
            )),
            Self::FuncRef(value) | Self::ExternRef(value) | Self::GcRef(value) => {
                value.is_null().then_some(Constant::Null)
            }
            _ => self.bits().map(|bits| match bits {
                ScalarBits::Bits32(bits) => Constant::Number(f64::from(bits as i32)),
                ScalarBits::Bits64(bits) => Constant::WasmBits64(bits),
                ScalarBits::Bits128(bits) => Constant::WasmV128(bits),
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WasmSignature {
    pub params: Vec<WasmType>,
    pub results: Vec<WasmType>,
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

#[derive(Clone, Copy)]
pub(crate) enum ScalarBits {
    Bits32(u32),
    Bits64(u64),
    Bits128([u8; V128_BYTES]),
}

impl WasmType {
    pub(crate) fn decode(self, bits: ScalarBits) -> Option<WasmValue> {
        match (self, bits) {
            (Self::I32, ScalarBits::Bits32(bits)) => Some(WasmValue::I32(bits as i32)),
            (Self::F32, ScalarBits::Bits32(bits)) => Some(WasmValue::F32(bits)),
            (Self::I64, ScalarBits::Bits64(bits)) => Some(WasmValue::I64(bits as i64)),
            (Self::F64, ScalarBits::Bits64(bits)) => Some(WasmValue::F64(bits)),
            (Self::V128, ScalarBits::Bits128(bits)) => {
                Some(WasmValue::V128(u128::from_le_bytes(bits)))
            }
            _ => None,
        }
    }
}

#[derive(Clone)]
pub(crate) struct CallableImport {
    pub(crate) name: super::WasmImportName,
    pub(crate) exact: bool,
}

/// One signature pool; function indices project into its type-index domain.
#[derive(Clone)]
pub(crate) struct WasmSignatures {
    pub(crate) declarations: super::WasmTypes,
    types: Vec<CallableSignature>,
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
        let (declaration, shape) = match ty {
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
        };
        let index = self
            .types
            .iter()
            .position(|ty| match (ty.declaration, declaration) {
                (Some(left), Some(right)) => {
                    self.declarations
                        .equivalent(left as usize, &self.declarations, right as usize)
                }
                (None, None) => ty.shape == shape,
                _ => false,
            })
            .unwrap_or_else(|| {
                self.types.push(CallableSignature { declaration, shape });
                self.types.len() - 1
            });
        u32::try_from(index)
            .map_err(|_| crate::Diagnostic::unsupported(name, "too many Wasm signature facts"))
    }

    pub(crate) fn imports(&self) -> &[CallableImport] {
        &self.imports
    }

    pub(crate) fn defined_index(&self, function: u32) -> Option<u32> {
        self.get(function as usize)?;
        function.checked_sub(self.imports.len() as u32)
    }

    pub(crate) fn defined_signature(&self, function: u32) -> Option<&WasmSignature> {
        self.get(function as usize + self.imports.len())
    }

    pub(crate) fn defined_count(&self) -> usize {
        self.function_count() - self.imports.len()
    }

    pub(crate) fn function_count(&self) -> usize {
        self.functions.len()
    }

    pub(crate) fn get(&self, function: usize) -> Option<&WasmSignature> {
        self.type_signature(*self.functions.get(function)?)
    }

    pub(crate) fn type_signature(&self, index: u32) -> Option<&WasmSignature> {
        Some(&self.types.get(index as usize)?.shape)
    }

    pub(crate) fn function_type_index(&self, function: usize) -> Option<u32> {
        self.functions.get(function).copied()
    }

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
        match expected.declaration {
            Some(target) => {
                actual.matches_declaration(actual_index, &self.declarations, target, exact)
            }
            None => match actual_type.declaration {
                Some(_) => actual.accepts_embedded(actual_index, &expected.shape),
                None => actual_type.shape == expected.shape,
            },
        }
    }
}

#[cfg(test)]
mod signature_pool_tests {
    use super::*;

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
            [WasmCallableType::Embedded(reference_signature.clone())],
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
