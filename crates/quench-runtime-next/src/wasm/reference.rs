//! A cast target retains a heap-type index; its declaration graph stays in the program.
use super::{Lowering, Reachability};
use crate::{Diagnostic, bytecode::Op};
use wasmparser::{AbstractHeapType, HeapType, Operator, RefType, UnpackedIndex};

super::numeric::selectors! { ExternalConversion;
    Internalize, AnyConvertExtern;
    Externalize, ExternConvertAny;
}

impl ExternalConversion {
    pub(crate) fn input_type(self) -> super::WasmType {
        match self {
            Self::Internalize => super::WasmType::EXTERNREF,
            Self::Externalize => Self::Internalize.output_type(true),
        }
    }

    pub(crate) fn output_type(self, nullable: bool) -> super::WasmType {
        super::WasmType::Reference {
            kind: match self {
                Self::Internalize => {
                    super::WasmReferenceKind::Internal(wasmparser::AbstractHeapType::Any)
                }
                Self::Externalize => super::WasmReferenceKind::External,
            },
            nullable,
        }
    }
}

/// A shared non-null check retains the operation's diagnostic contract.
#[derive(Clone, Copy, Debug)]
#[repr(u32)]
pub(crate) enum NonNullCheck {
    Reference,
    Function,
}
impl NonNullCheck {
    pub(crate) fn from_tag(tag: u32) -> Option<Self> {
        [Self::Reference, Self::Function].get(tag as usize).copied()
    }
    pub(crate) fn trap(self) -> super::WasmTrap {
        match self {
            Self::Reference => super::WasmTrap::NullReference,
            Self::Function => super::WasmTrap::NullFunctionReference,
        }
    }
}

const NULLABLE: u32 = 1;
pub(crate) const EXACT: u32 = NULLABLE << 1;
const DEFINED: u32 = EXACT << 1;
const INDEX_SHIFT: u32 = DEFINED.trailing_zeros() + 1;
const INDEX_MAX: u32 = u32::MAX >> INDEX_SHIFT;
const ABSTRACT_TYPES: &[AbstractHeapType] = &[
    AbstractHeapType::Func,
    AbstractHeapType::Extern,
    AbstractHeapType::Any,
    AbstractHeapType::Eq,
    AbstractHeapType::Struct,
    AbstractHeapType::Array,
    AbstractHeapType::I31,
    AbstractHeapType::None,
    AbstractHeapType::NoFunc,
    AbstractHeapType::NoExtern,
    AbstractHeapType::Exn,
    AbstractHeapType::NoExn,
];

#[derive(Clone, Copy, Debug)]
pub(crate) struct ReferenceTarget(u32);
impl ReferenceTarget {
    pub(crate) fn from_type(ty: RefType) -> Option<Self> {
        let (index, flags) = match ty.heap_type() {
            HeapType::Abstract { shared: false, ty } => (
                ABSTRACT_TYPES.iter().position(|&other| ty == other)? as u32,
                0,
            ),
            HeapType::Concrete(UnpackedIndex::Module(index)) => (index, DEFINED),
            HeapType::Exact(UnpackedIndex::Module(index)) => (index, DEFINED | EXACT),
            _ => return None,
        };
        (index <= INDEX_MAX).then_some(Self(
            (index << INDEX_SHIFT) | flags | if ty.is_nullable() { NULLABLE } else { 0 },
        ))
    }
    pub(crate) fn from_tag(tag: u32) -> Option<Self> {
        let target = Self(tag);
        target.reference_type().map(|_| target)
    }
    pub(crate) fn descriptor_type(self, owner: &super::WasmTypes) -> Option<super::WasmType> {
        let (index, exact) = match self.reference_type()?.heap_type() {
            HeapType::Concrete(UnpackedIndex::Module(index)) => (index, false),
            HeapType::Exact(UnpackedIndex::Module(index)) => (index, true),
            _ => return None,
        };
        let super::WasmType::Reference {
            kind: super::WasmReferenceKind::DeclaredGc { index, .. },
            ..
        } = owner.descriptor_type(index)?
        else {
            return None;
        };
        Some(super::WasmType::Reference {
            kind: super::WasmReferenceKind::DeclaredGc { index, exact },
            nullable: true,
        })
    }
    pub(crate) fn tag(self) -> u32 {
        self.0
    }
    pub(crate) fn reference_type(self) -> Option<RefType> {
        let index = self.0 >> INDEX_SHIFT;
        let heap = if self.0 & DEFINED != 0 {
            let index = UnpackedIndex::Module(index);
            if self.0 & EXACT != 0 {
                HeapType::Exact(index)
            } else {
                HeapType::Concrete(index)
            }
        } else {
            if self.0 & EXACT != 0 {
                return None;
            }
            HeapType::Abstract {
                shared: false,
                ty: *ABSTRACT_TYPES.get(index as usize)?,
            }
        };
        RefType::new(self.0 & NULLABLE != 0, heap)
    }
}

impl Lowering<'_> {
    pub(super) fn reference_operator(
        &mut self,
        operator: &Operator<'_>,
    ) -> Result<bool, Diagnostic> {
        if let Some(conversion) = ExternalConversion::from_wasm(operator) {
            if self.path == Reachability::Live {
                let input = self.pop()?;
                let result = self.push()?;
                self.emit(
                    Op::WasmExternalConversion,
                    result,
                    input,
                    0,
                    conversion as u32,
                )?;
            }
            return Ok(true);
        }
        match *operator {
            Operator::RefEq => {
                if self.path == Reachability::Live {
                    let right = self.pop()?;
                    let left = self.pop()?;
                    let result = self.push()?;
                    self.emit(Op::WasmRefEq, result, left, right, 0)?;
                }
                return Ok(true);
            }
            Operator::BrOnCast {
                relative_depth,
                from_ref_type,
                to_ref_type,
            }
            | Operator::BrOnCastFail {
                relative_depth,
                from_ref_type,
                to_ref_type,
            }
            | Operator::BrOnCastDescEq {
                relative_depth,
                from_ref_type,
                to_ref_type,
            }
            | Operator::BrOnCastDescEqFail {
                relative_depth,
                from_ref_type,
                to_ref_type,
            } => {
                self.reference_target(from_ref_type)?;
                let target = self.reference_target(to_ref_type)?;
                let described = matches!(
                    operator,
                    Operator::BrOnCastDescEq { .. } | Operator::BrOnCastDescEqFail { .. }
                );
                if described
                    && target
                        .descriptor_type(&self.signatures.declarations)
                        .is_none()
                {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm cast target has no descriptor",
                    ));
                }
                if self.path == Reachability::Live {
                    let descriptor = if described { self.pop()? } else { 0 };
                    let reference = self.pop()?;
                    self.push()?; // Both branch and fallthrough carry the original reference.
                    let condition = self.push()?;
                    self.depth -= 1;
                    self.emit(
                        if described {
                            Op::WasmDescriptorTest
                        } else {
                            Op::WasmRefTest
                        },
                        condition,
                        reference,
                        descriptor,
                        target.tag(),
                    )?;
                    if matches!(
                        operator,
                        Operator::BrOnCastFail { .. } | Operator::BrOnCastDescEqFail { .. }
                    ) {
                        self.emit(
                            Op::WasmI32Unary,
                            condition,
                            condition,
                            0,
                            super::integer::I32UnaryOperator::EqualZero as u32,
                        )?;
                    }
                    self.branch_if(relative_depth, condition)?;
                }
                return Ok(true);
            }
            _ => {}
        }
        let (heap, nullable, cast, described) = match *operator {
            Operator::RefTestNonNull { hty } => (hty, false, false, false),
            Operator::RefTestNullable { hty } => (hty, true, false, false),
            Operator::RefCastNonNull { hty } => (hty, false, true, false),
            Operator::RefCastNullable { hty } => (hty, true, true, false),
            Operator::RefCastDescEqNonNull { hty } => (hty, false, true, true),
            Operator::RefCastDescEqNullable { hty } => (hty, true, true, true),
            _ => return Ok(false),
        };
        let error =
            || Diagnostic::unsupported(self.name, "unsupported Wasm reference test/cast target");
        let ty = RefType::new(nullable, heap).ok_or_else(error)?;
        let target = self.reference_target(ty)?;
        if described
            && target
                .descriptor_type(&self.signatures.declarations)
                .is_none()
        {
            return Err(Diagnostic::unsupported(
                self.name,
                "Wasm cast target has no descriptor",
            ));
        }
        if self.path == Reachability::Live {
            let descriptor = if described { self.pop()? } else { 0 };
            let input = self.pop()?;
            let result = self.push()?;
            self.emit(
                if described {
                    Op::WasmDescriptorCast
                } else if cast {
                    Op::WasmRefCast
                } else {
                    Op::WasmRefTest
                },
                result,
                input,
                descriptor,
                target.tag(),
            )?;
        }
        Ok(true)
    }
    fn reference_target(&self, ty: RefType) -> Result<ReferenceTarget, Diagnostic> {
        let error = || Diagnostic::unsupported(self.name, "unsupported Wasm reference target");
        self.signatures
            .declarations
            .callable_value_type(wasmparser::ValType::Ref(ty))
            .ok_or_else(error)?;
        ReferenceTarget::from_type(ty).ok_or_else(error)
    }
}
