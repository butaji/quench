//! The specializer's machine: the primitives of `staged::machine` as Cranelift instructions, the
//! one place a native tier builds instructions by hand. Everything that has a meaning (a layout, a
//! language rule, a Wasm trap) is a `staged` operation over these. Both backends (`Emitter`, the
//! Wasm tier's, and `Emit`, the JavaScript tier's stage) build into one `FunctionBuilder`, so each
//! primitive body exists once, over `Builds`; one family per file: scalar, memory, flow, vector.

mod flow;
mod memory;
mod scalar;
mod vector;

use super::object_slots::Emit;
use super::{Emitter, Host, VECTOR_IR};
use crate::staged::machine::{Lane, Machine};
use cranelift_codegen::ir::{self, types};
use cranelift_frontend::FunctionBuilder;

/// A backend that builds into one Cranelift function: the accessor the primitive bodies are
/// written over.
pub(in crate::vm::compiled) trait Builds<'b> {
    fn function_builder(&mut self) -> &mut FunctionBuilder<'b>;
}

impl<'b> Builds<'b> for Emit<'_, 'b> {
    fn function_builder(&mut self) -> &mut FunctionBuilder<'b> {
        &mut *self.builder
    }
}

impl<'b, H: Host> Builds<'b> for Emitter<'_, 'b, H> {
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
fn lane_of(ty: ir::Type) -> Lane {
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
