//! The specializer's machine: the primitives of `staged::machine` as Cranelift instructions, the
//! one place a native tier builds instructions by hand. Everything that has a meaning (a layout, a
//! language rule, a Wasm trap) is a `staged` operation over these. Both backends (`Emitter`, the
//! Wasm tier's, and `Emit`, the JavaScript tier's stage) build into one `FunctionBuilder`, so each
//! primitive body exists once, over `Builds`; one family per file: scalar, memory, flow, vector.

#[path = "emit_machine/flow.rs"]
mod flow;
#[path = "emit_machine/memory.rs"]
mod memory;
#[path = "emit_machine/scalar.rs"]
mod scalar;
#[path = "emit_machine/vector.rs"]
mod vector;

use super::object_slots::Emit;
use super::{Body, Emitter, RegionCore, VECTOR_IR};
use crate::staged::machine::{Lane, Machine};
use cranelift_codegen::ir::{self, types};
use cranelift_frontend::FunctionBuilder;

/// A backend that builds into one Cranelift function: the accessor the primitive bodies are
/// written over.
pub(in crate::vm::compiled) trait Builds<'b> {
    fn function_builder(&mut self) -> &mut FunctionBuilder<'b>;
}

/// A bare builder is a backend: the emitters of a function that no tier owns (a stencil, a
/// regexp search, a dispatch shell) compose the primitives on the function builder itself.
impl<'b> Builds<'b> for FunctionBuilder<'b> {
    fn function_builder(&mut self) -> &mut FunctionBuilder<'b> {
        self
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

impl<'b, B: Builds<'b>> Machine for B {
    type Word = ir::Value;
}

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
pub(in crate::vm) fn of_in_lane(ty: ir::Type) -> Lane {
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
