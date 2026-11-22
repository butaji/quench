//! The specializer's machine: the primitives of `staged::machine` as Cranelift instructions, the
//! one place a native tier builds instructions by hand. Everything that has a meaning (a layout, a
//! language rule, a Wasm trap) is a `staged` operation over these. Both backends (`Emitter`, the
//! Wasm tier's, and `Emit`, the JavaScript tier's stage) build into one `FunctionBuilder`, so each
//! primitive body exists once, over `Builds`; one family per file: scalar, memory, flow, vector.

#[allow(unused_imports)]
use crate::ext_traits::*;
#[path = "emit_machine/flow.rs"]
pub(crate) mod flow;
#[path = "emit_machine/memory.rs"]
pub(crate) mod memory;
#[path = "emit_machine/scalar.rs"]
pub(crate) mod scalar;
#[path = "emit_machine/vector.rs"]
pub(crate) mod vector;

use super::object_slots::Emit;
use super::{Body, Emitter, RegionCore, VECTOR_IR};
use quench_runtime::staged::machine::{Lane, Machine};
use cranelift_codegen::ir::{self, types};
use cranelift_frontend::FunctionBuilder;

/// Implements a machine trait for every backend (`Builds`): the one body, over each builder type;
/// a blanket impl over `Builds` is not allowed for the core's machine traits.
macro_rules! for_each_builder {
    ($tr:path { $($body:tt)* }) => {
        impl<'b> $tr for crate::compiled::emit_machine::Raw<'b> { $($body)* }
        impl<'b> $tr for crate::compiled::RegionCore<'b> { $($body)* }
        impl<'b> $tr for crate::compiled::Body<'b> { $($body)* }
        impl<'a, 'b> $tr for crate::compiled::object_slots::Emit<'a, 'b> { $($body)* }
        impl<'a, 'b> $tr for crate::compiled::Emitter<'a, 'b> { $($body)* }
        impl<'a, 'b> $tr for crate::compiled::lane_region::Lowering<'a, 'b> { $($body)* }
    };
}
pub(crate) use for_each_builder;


/// A backend that builds into one Cranelift function: the accessor the primitive bodies are
/// written over.
pub(crate) trait Builds<'b> {
    fn function_builder(&mut self) -> &mut FunctionBuilder<'b>;
}

/// A bare builder as a backend: the emitters of a function that no tier owns (a stencil, a
/// regexp search, a dispatch shell) compose the primitives on the function builder itself, seen
/// through `raw` (the core owns the machine traits and Cranelift the builder, so the engine
/// implements them for its own view of it).
#[repr(transparent)]
pub(crate) struct Raw<'b>(pub(crate) FunctionBuilder<'b>);

/// `builder` as a backend (`Raw`).
#[inline(always)]
pub(crate) fn raw<'a, 'b>(builder: &'a mut FunctionBuilder<'b>) -> &'a mut Raw<'b> {
    // SAFETY: `Raw` is a transparent wrapper of the builder.
    unsafe { &mut *(builder as *mut FunctionBuilder<'b>).cast::<Raw<'b>>() }
}

impl<'b> std::ops::Deref for Raw<'b> {
    type Target = FunctionBuilder<'b>;
    #[inline(always)]
    fn deref(&self) -> &FunctionBuilder<'b> { &self.0 }
}

impl<'b> std::ops::DerefMut for Raw<'b> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut FunctionBuilder<'b> { &mut self.0 }
}

impl<'b> Builds<'b> for Raw<'b> {
    fn function_builder(&mut self) -> &mut FunctionBuilder<'b> {
        &mut self.0
    }
}

impl<'b> Builds<'b> for RegionCore<'b> {
    fn function_builder(&mut self) -> &mut FunctionBuilder<'b> {
        &mut self.builder
    }
}

impl<'b> Builds<'b> for Body<'b> {
    fn function_builder(&mut self) -> &mut FunctionBuilder<'b> {
        &mut self.core.builder
    }
}

impl<'b> Builds<'b> for Emit<'_, 'b> {
    fn function_builder(&mut self) -> &mut FunctionBuilder<'b> {
        &mut *self.builder
    }
}

impl<'b> Builds<'b> for Emitter<'_, 'b> {
    fn function_builder(&mut self) -> &mut FunctionBuilder<'b> {
        &mut self.body.core.builder
    }
}

crate::compiled::emit_machine::for_each_builder!(Machine {
    type Word = ir::Value;
});

/// The Cranelift type of a lane.
fn machine_type(lane: Lane) -> ir::Type {
    match lane {
        Lane::I8 => types::I8,
        Lane::I16 => types::I16,
        Lane::I32 => types::I32,
        Lane::I64 => types::I64,
        Lane::F32 => types::F32,
        Lane::F64 => types::F64,
        Lane::V128 => VECTOR_IR,
    }
}

/// The lane of a Cranelift type; a type that is none of the narrow, float or vector ones is the
/// 64-bit word.
pub(crate) fn of_in_lane(ty: ir::Type) -> Lane {
    match ty {
        types::I8 => Lane::I8,
        types::I16 => Lane::I16,
        types::I32 => Lane::I32,
        types::F32 => Lane::F32,
        types::F64 => Lane::F64,
        _ if ty.is_vector() => Lane::V128,
        _ => Lane::I64,
    }
}
