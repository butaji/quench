//! The loop tier: Cranelift code for a hot loop whose body makes no guest call or collection,
//! entered at its backward jump in compiled execution. Effectful instructions can form
//! exact-PC boundaries: native code writes back its live state and the existing lane runs
//! the instruction and all following semantics.
//!
//! A region is a loop's instructions, header to backward jump, every one of which reads and
//! writes Numbers in registers, depth-0 captures and dense elements of record arrays: it
//! reads present elements, and stores below an array's length or appends at it within the
//! capacity its backing already has.
//! Native code keeps each Number register in an SSA value: an `i32` where every value it
//! takes is one (`representations`), else an `f64`. Registers are the
//! interpreter's state at every instruction boundary the code leaves at, so leaving is
//! writing them back:
//!
//! - entry guards every register the region uses: a Number, or a record array the region
//!   reads (and, for one it writes, a dense array whose backing it alone owns and no
//!   prototype of which holds an index, so a store defines or replaces an own data element);
//!   otherwise nothing runs and the interpreter takes the jump;
//! - a jump out of the region resumes the lane at its target;
//! - an instruction whose operands leave the subset (a hole, a non-Number element, an index
//!   past the length, an integer conversion out of range) exits before its effects, and the
//!   general path executes it;
//! - every backward jump polls host termination, exiting at the jump so the interpreter's
//!   safepoint runs.
//!
//! Native code runs no guest code or collection. A guarded capacity helper can relocate
//! array element storage, refreshing all aliased entry views before native code continues.
//! Collection debt exits with live values restored before the VM can collect. Other effect
//! boundaries exit before their effects.

#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationExt};
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::lane_unit::{inlined_returning, renamed};
use quench_runtime::bytecode::{
    FieldBase, FieldLayout, FieldLookup, FieldSite, Function, ImmediateLayout, ImmediateRole, InstructionField, Op, Operand,
    OperandKind, Register, WideInstruction,
};
use quench_runtime::compile::liveness::RegisterSet;
use quench_runtime::value::Value;
use quench_runtime::staged::{DecodeContext, Operation};
use quench_runtime::vm::operations::{NumberArithmetic, NumberBitwise, NumberUnary};

use super::typed_ir::Type;
use quench_runtime::typed_ir::Representation;
use oxc_ast::ast::UnaryOperator;

use crate::compiled::tier_policy::{MAX_REGION_INSTRUCTIONS, MAX_REGION_REGISTERS, MAX_REGION_ARRAYS, MAX_REGION_CAPTURES, MAX_REGION_FIELDS, MAX_REGION_FIELD_STORES};

#[path = "osr_loop/inlining.rs"]
pub(crate) mod inlining;
#[path = "osr_loop/function.rs"]
pub(crate) mod function;
#[path = "osr_loop/iteration.rs"]
pub(crate) mod iteration;
#[path = "osr_loop/receivers.rs"]
pub(crate) mod receivers;
#[path = "osr_loop/lifetime.rs"]
pub(crate) mod lifetime;
#[path = "osr_loop/stores.rs"]
mod stores;
use stores::{admit_binding_store, admit_stored, admitted_store, stores_binding};

pub(crate) use quench_runtime::vm::engine_types::ArrayUse;

pub(crate) use quench_runtime::vm::engine_types::LoopRegion;

#[quench_macros::ext(LoopRegionOps1)]
impl LoopRegion {
    /// The function's full decoded code, which a loop unit keeps so its PCs stay the
    /// function's; only `header..=backedge` lowers.
    pub(crate) fn instructions(&self) -> &[WideInstruction] { &self.instructions }
    /// The Number constant the region reads at pool index `index`.
    pub(crate) fn number_constant(&self, index: usize) -> Option<f64> { self.constants.get(&index).copied() }
    /// The registers the region treats as Numbers.
    pub(crate) fn number_registers(&self) -> &[Register] { &self.numbers }
    /// The cached field reads, by offset from the header: the lane specializer's field
    /// sites, mapped from the observation's descriptors.
    pub(crate) fn field_reads(&self) -> &std::collections::BTreeMap<u32, usize> { &self.field_reads }

    /// Whether native code reads the heap itself, through the entry's heap views
    /// (`LoopEntry::push_native_heap`): a dynamic field read or a dynamic array.
    pub(crate) fn reads_heap_natively(&self) -> bool {
        !self.dynamic_field_reads.is_empty() || !self.dynamic_arrays.is_empty()
    }

    pub(crate) fn is_dynamic_field(&self, index: usize) -> bool {
        self.dynamic_field_reads.values().any(|field| *field == index)
    }

    /// Whether field `index` is a dynamic read through a register the loop defines or that is
    /// dead at the header: at entry that register holds no value of the loop's (it may name a
    /// dead record), so the entry seeds the field's placeholder without reading through it.
    pub(crate) fn seeds_without_receiver(&self, index: usize) -> bool {
        let Some(FieldUse { receiver: FieldReceiver::Register(register), .. }) = self.fields.get(index) else { return false };
        self.is_dynamic_field(index)
            && (!self.is_live(self.header, *register)
                || self.instructions.get(self.header as usize..=self.backedge as usize)
                    .is_some_and(|code| code.iter().any(|instruction| instruction.defined_register() == Some(*register))))
    }

    pub(crate) fn writes_fields(&self) -> bool {
        !self.field_store_sites.is_empty() || !self.dynamic_field_stores.is_empty()
    }

    /// The write contract checked by typed verification and admitted before pointer exposure.
    pub(crate) fn field_store_type(&self, store: &FieldStore) -> Type {
        if self.fields.iter().any(|field| store.matches(field)) { Type::NUMBER }
        else { Type::ANY }
    }
}

pub(crate) use quench_runtime::vm::engine_types::RegionRejection;

pub(crate) use quench_runtime::vm::engine_types::RegionAnalysisFailure;

#[path = "osr_loop/fields.rs"]
pub(crate) mod fields;
#[allow(unused_imports)]
pub(crate) use fields::*;
#[path = "osr_loop/tagging.rs"]
pub(crate) mod tagging;
#[allow(unused_imports)]
pub(crate) use tagging::*;
#[path = "osr_loop/inlined_callees.rs"]
pub(crate) mod inlined_callees;
#[allow(unused_imports)]
pub(crate) use inlined_callees::*;
#[path = "osr_loop/region_inlining.rs"]
pub(crate) mod region_inlining;
#[allow(unused_imports)]
pub(crate) use region_inlining::*;
#[path = "osr_loop/entry.rs"]
pub(crate) mod entry;
#[allow(unused_imports)]
pub(crate) use entry::*;
#[path = "osr_loop/operator_classes.rs"]
pub(crate) mod operator_classes;
#[allow(unused_imports)]
pub(crate) use operator_classes::*;
#[path = "osr_loop/admission.rs"]
pub(crate) mod admission;
#[allow(unused_imports)]
pub(crate) use admission::*;
#[path = "osr_loop/admit_roles.rs"]
pub(crate) mod admit_roles;
#[allow(unused_imports)]
pub(crate) use admit_roles::*;
#[path = "osr_loop/webs.rs"]
pub(crate) mod webs;
#[allow(unused_imports)]
pub(crate) use webs::*;

#[quench_macros::ext(RegionAnalysisFailureOps1)]
impl RegionAnalysisFailure {
    fn at(reason: RegionRejection, header: u32, offset: usize) -> Self {
        Self { reason, pc: header.saturating_add(u32::try_from(offset).unwrap_or(u32::MAX)) }
    }
}

pub(crate) use quench_runtime::vm::engine_types::FieldReceiver;

use quench_runtime::vm::engine_types::FieldPath;

#[quench_macros::ext(FieldPathOps1)]
impl FieldPath {
    /// Whether the two paths read one value in a region, which writes no field: the same
    /// root and names, whichever caches answer them.
    fn reads_as(&self, other: &FieldPath) -> bool {
        self.root == other.root && self.steps.iter().map(|step| step.0).eq(other.steps.iter().map(|step| step.0))
    }
}

pub(crate) use quench_runtime::vm::engine_types::FieldUse;

pub(crate) use quench_runtime::vm::engine_types::FieldStore;

#[quench_macros::ext(FieldStoreOps1)]
impl FieldStore {
    fn matches(&self, field: &FieldUse) -> bool {
        field.receiver == self.receiver && field.atom == self.atom
    }
}

use quench_runtime::vm::engine_types::IdentityBranch;

pub(crate) use quench_runtime::vm::engine_types::InlineGuard;

pub(crate) use quench_runtime::vm::engine_types::InlineFrame;

pub(crate) use quench_runtime::vm::engine_types::MAX_INLINE_PARAMS;

pub(crate) use quench_runtime::vm::engine_types::Origin;

pub(crate) use quench_runtime::vm::engine_types::Inlining;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

#[quench_macros::ext(InliningOps1)]
impl Inlining {
    /// The registers past the caller's that the inlined frames occupy: the scratch window a leave
    /// inside one writes and reads (`LoopEntry::scratch_address`, `LoopEntry::scratch_value`).
    pub(crate) fn window(&self) -> usize {
        super::frames_window(self.frames.iter().map(|frame| (frame.base, frame.registers)), self.caller_registers)
    }
}

#[quench_macros::ext(LoopRegionOps2)]
impl LoopRegion {
    fn entry_count(&self) -> usize {
        self.numbers.len() + self.tagged.len()
    }

    fn entry_register(&self, index: usize) -> Option<Register> {
        self.numbers
            .get(index)
            .or_else(|| self.tagged.get(index.saturating_sub(self.numbers.len())))
            .copied()
    }

    /// The cache sites of the stores whose receiver the region redefines.
    pub(crate) fn dynamic_store_sites(&self) -> impl Iterator<Item = u16> + '_ {
        self.dynamic_field_stores.values().map(|&(_, site)| site)
    }

    pub(crate) fn entry_registers(&self) -> impl Iterator<Item = Register> + '_ {
        self.numbers.iter().chain(&self.tagged).copied()
    }

    /// Whether the interpreter may read `register` resuming at `pc` before writing it; every
    /// register is, where liveness does not cover the function.
    pub(crate) fn is_live(&self, pc: u32, register: Register) -> bool {
        let Some(live) = &self.live else {
            return true;
        };
        // An inlining region's liveness is by its own PCs, from the header.
        let index = if self.inlining.is_some() { (pc - self.header) as usize } else { pc as usize };
        live.is_live(index, register)
    }

    /// The byte offset in the entry record of the element pointer, length and capacity of the array
    /// `register` holds throughout the region, when the entry pushes one for it.
    pub(crate) fn array_entry_offset(&self, register: Register) -> Option<u64> {
        let k = self.arrays.iter().position(|array| array.register == register)?;
        Some(((field_word(self, self.fields.len()) + k * ARRAY_WORDS) * std::mem::size_of::<u64>()) as u64)
    }

    /// The field whose value `register` holds at every read, when it is one the region
    /// writes only with that field's reads (`held_registers`).
    pub(crate) fn held_field(&self, register: Register) -> Option<usize> {
        self.held.get(&register).map(|(_, index)| *index)
    }
}
