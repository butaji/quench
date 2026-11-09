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

use crate::bytecode::{
    Class, FieldBase, FieldLayout, FieldLookup, FieldSite, Function, ImmediateLayout, ImmediateRole, InstructionField, Op, Operand,
    OperandKind, Register, WideInstruction,
};
use crate::compile::liveness::RegisterSet;
use crate::value::Value;
use super::super::operations::{NumberArithmetic, NumberBitwise, NumberUnary};

use super::typed_ir::Type;
use super::NativeFieldLoad;
use oxc_ast::ast::UnaryOperator;

/// The most instructions a region spans. Native code polls termination only at backward
/// jumps, so the bound keeps compile time and code size proportional to small loop bodies.
const MAX_REGION_INSTRUCTIONS: usize = 512;
/// The most registers, captures and arrays a region names, bounding entry guards and the
/// entry record.
const MAX_REGION_REGISTERS: usize = 128;
const MAX_REGION_ARRAYS: usize = 8;
const MAX_REGION_CAPTURES: usize = 16;
const MAX_REGION_FIELDS: usize = 16;
const MAX_REGION_FIELD_STORES: usize = 16;
pub(crate) use crate::vm::js_compiled::MAX_CODE_BYTES;

#[path = "osr_loop/iteration.rs"]
mod iteration;
#[path = "osr_loop/receivers.rs"]
mod receivers;

/// What native code writes to its result word: the outcome in the high word, a bytecode PC
/// in the low one.
/// The outcome of one native run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoopOutcome {
    /// An entry guard failed: registers are untouched, and the interpreter takes the jump.
    NotEntered,
    /// Control left the region by a jump to this PC, which the lane runs next.
    Resume(u32),
    /// The instruction at this PC needs the general path, which executes it.
    Exit(u32),
    /// The region completed its activation with `value` at the record `pc`: a `Return`
    /// inside the loop, or a returning call. The lane completes the activation as its own
    /// body would (`lane_return_value_general`), which is not an exit.
    Returned { pc: u32, value: Value },
    /// A call activated the callee's lane body, and the run left there (`LaneExit::Callee`):
    /// the lane resumes in the activation the call made, at `LaneContext::callee_resume`,
    /// not in this region's view.
    Callee,
    /// Termination was requested while the code ran: its poll faulted, so registers may
    /// hold values of an unfinished iteration, which nothing reads before the request ends
    /// the execution.
    Terminated,
}

/// An array a region indexes, by its register; `written` when the region stores into it.
/// The array is the register's value at entry, or, with `field`, the value of that cached
/// field (`LoopRegion::fields`), which the field's one `GetField` in the region writes to
/// the register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ArrayUse {
    pub(crate) register: Register,
    pub(crate) written: bool,
    pub(crate) field: Option<usize>,
}

/// A loop's admissible region and the facts its entry needs.
pub(crate) struct LoopRegion {
    pub(crate) header: u32,
    pub(crate) backedge: u32,
    /// The Number registers; a register's index here names its entry definition (`Webs`).
    numbers: Vec<Register>,
    /// Registers with a tagged value lifetime; each web owns its representation.
    tagged: Vec<Register>,
    /// Per-value roles when an overwritten physical slot carries unrelated types.
    /// The original slots and PCs remain the authority for entry and recovery.
    /// The arrays, in entry-record order. Entry admits one the region writes only when it is
    /// dense (`Vm::dense_array_length`), owns its backing alone, and no prototype of it holds
    /// an index (`Vm::prototypes_hold_no_index`).
    pub(crate) arrays: Vec<ArrayUse>,
    /// The depth-0 capture slots `LoadCapture` reads, in entry-record order.
    pub(crate) captures: Vec<u16>,
    /// The cached data fields the region reads, in entry-record order: a field read
    /// through another's value follows it (`intern_field`).
    pub(crate) fields: Vec<FieldUse>,
    /// The field each `GetField` of the region reads, by offset from the header.
    field_reads: std::collections::BTreeMap<u32, usize>,
    /// Reads whose direct register receiver is redefined in this loop. These use a
    /// side-effect-free field-cache callout at the instruction instead of an entry snapshot.
    dynamic_field_reads: std::collections::BTreeMap<u32, usize>,
    /// The array registers the region defines: each access reads the array its register
    /// holds then, from the array's record, and only a present
    /// element below the backing's length, which no prototype answers, stays native.
    dynamic_arrays: std::collections::BTreeSet<Register>,
    /// Existing own-data Number slots stored by the region, by offset from its header.
    field_stores: std::collections::BTreeMap<u32, usize>,
    /// Own-data stores whose receiver the region redefines, by offset from its header: each
    /// stores through the VM's cached store (`OSR_FIELD_STORE`), so every read of the same
    /// name in the region reads the receiver's current slot (`dynamic_field_reads`).
    dynamic_field_stores: std::collections::BTreeMap<u32, (crate::bytecode::Atom, u16)>,
    /// Receivers and cache sites whose flat slots are guarded before native entry.
    pub(crate) field_store_sites: Vec<FieldStore>,
    /// The registers that hold a field's value at every read (`held_registers`), with the
    /// path they hold and that field's index.
    held: std::collections::BTreeMap<Register, (FieldPath, usize)>,
    /// Nullish equality branches against `null` or `undefined` that read an unchanged
    /// tagged register directly from the activation's rooted register file.
    identity_branches: std::collections::BTreeMap<u32, IdentityBranch>,
    /// The region's instructions, from the header.
    instructions: Vec<WideInstruction>,
    /// The Number constants the region reads, by constant index.
    constants: std::collections::BTreeMap<usize, f64>,
    /// The function's live registers before each instruction (`compile::liveness`), when
    /// liveness covers it; for an inlining region, by its own PCs (`Inlining`). A plain
    /// region shares its function's masks (`ResidualProgram::shared_register_root_masks`).
    live: Option<std::rc::Rc<crate::compile::liveness::RegisterMasks>>,
    /// Where an inlining region's instructions come from, when the region inlines calls.
    pub(crate) inlining: Option<Box<Inlining>>,
}

impl LoopRegion {
    /// The function's full decoded code, which a loop unit keeps so its PCs stay the
    /// function's; only `header..=backedge` lowers.
    pub(crate) fn instructions(&self) -> &[WideInstruction] { &self.instructions }
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

    pub(crate) fn writes_fields(&self) -> bool {
        !self.field_store_sites.is_empty() || !self.dynamic_field_stores.is_empty()
    }

    /// The write contract checked by typed verification and admitted before pointer exposure.
    pub(crate) fn field_store_type(&self, store: &FieldStore) -> Type {
        if self.fields.iter().any(|field| store.matches(field)) { Type::NUMBER }
        else { Type::ANY }
    }


    pub(crate) fn call_boundary_pcs(&self) -> Vec<u32> {
        self.instructions
            .iter()
            .enumerate()
            .filter(|(_, instruction)| instruction.op().is_ordinary_call())
            .map(|(offset, _)| self.header.saturating_add(offset as u32))
            .collect()
    }

}

/// Why a backward jump was excluded from OSR before native code generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RegionRejection {
    RegionTooLarge,
    UnsupportedInstruction(Op),
    RegisterLimit,
    CaptureLimit,
    ArrayLimit,
    FieldLimit,
    FieldStoreLimit,
    FieldMutation,
    /// An array the region indexes is a register it defines other than by reading an
    /// element or field or moving one (`LoopRegion::dynamic_arrays`), as a capture's value.
    ArrayRedefined,
}

/// A loop analysis failure attributed to the function PC whose requirement blocked it.
/// Limits that apply to the whole region point at its backedge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RegionAnalysisFailure {
    pub(crate) reason: RegionRejection,
    pub(crate) pc: u32,
}

impl RegionAnalysisFailure {
    /// The region stops only at a call, which may run inlined once the site is hot.
    pub(crate) fn is_call_boundary(self) -> bool { self.reason == RegionRejection::UnsupportedInstruction(Op::Call) }

    fn at(reason: RegionRejection, header: u32, offset: usize) -> Self {
        Self { reason, pc: header.saturating_add(u32::try_from(offset).unwrap_or(u32::MAX)) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FieldReceiver {
    This,
    /// A register the region never writes.
    Register(Register),
    /// The value of the region's field at this index (`LoopRegion::fields`), an earlier one.
    Field(usize),
    /// The value of this depth-0 capture slot, which no region stores (`receivers::captured`).
    Capture(u16),
}

/// What a `GetField` reads: named fields one after another from a root receiver (`this` or
/// a register), each step with the cache site that answers it.
#[derive(Clone, Debug)]
struct FieldPath {
    root: FieldReceiver,
    steps: Vec<(crate::bytecode::Atom, u16)>,
}

impl FieldPath {
    /// Whether the two paths read one value in a region, which writes no field: the same
    /// root and names, whichever caches answer them.
    fn reads_as(&self, other: &FieldPath) -> bool {
        self.root == other.root && self.steps.iter().map(|step| step.0).eq(other.steps.iter().map(|step| step.0))
    }
}

/// The base a `GetField` reads its first field from, its own or its nested site's.
fn field_base(instruction: WideInstruction, sites: &[FieldSite]) -> Option<FieldBase> {
    match instruction.field_lookup()? {
        FieldLookup::Atom { base, .. } => Some(base),
        FieldLookup::Site(index) => Some(sites.get(index)?.base),
    }
}

/// The path a `GetField` reads, a register base that `held` names (`held_registers`) read
/// through the path that gave it its value; a nested site (`FieldLookup::Site`) is its steps.
fn field_path(
    instruction: WideInstruction,
    sites: &[FieldSite],
    held: &std::collections::BTreeMap<Register, FieldPath>,
) -> Option<FieldPath> {
    (instruction.op().is_get_field() && instruction.result_flags() == 0).then_some(())?;
    let from = |base: FieldBase| -> Option<FieldPath> {
        if base == FieldBase::THIS {
            return Some(FieldPath { root: FieldReceiver::This, steps: Vec::new() });
        }
        let register = base.register_index()?;
        Some(held.get(&register).cloned().unwrap_or(FieldPath { root: FieldReceiver::Register(register), steps: Vec::new() }))
    };
    match instruction.field_lookup()? {
        FieldLookup::Atom { atom, base, cache_site } => {
            let mut path = from(base)?;
            path.steps.push((atom, cache_site));
            Some(path)
        }
        FieldLookup::Site(index) => {
            let site = sites.get(index)?;
            site.sink.is_none().then_some(())?;
            let mut path = from(site.base)?;
            path.steps.push(site.first);
            path.steps.extend(site.second);
            Some(path)
        }
    }
}

/// The index in `fields` of the field whose value `path` reads, adding each step the
/// fields lack after the one it reads from: every step but the last is a receiver, so
/// tagged, and the last is `tagged` as its read's result.
fn intern_field(fields: &mut Vec<FieldUse>, path: &FieldPath, tagged: bool) -> usize {
    let mut receiver = path.root;
    let mut index = usize::MAX;
    for (step, (atom, cache_site)) in path.steps.iter().copied().enumerate() {
        let tagged = tagged || step + 1 < path.steps.len();
        index = match fields
            .iter()
            .position(|field| field.receiver == receiver && field.atom == atom && field.tagged == tagged)
        {
            Some(index) => index,
            None => {
                fields.push(FieldUse { receiver, atom, cache_site, tagged });
                fields.len() - 1
            }
        };
        receiver = FieldReceiver::Field(index);
    }
    index
}

/// The registers a region reads as objects (`object_reads`, and `objects`) that hold a
/// field's value at every read: each definition of one in the region is a `GetField` of
/// one path, and none is live at the header (`header_live`), so every read follows a
/// definition. As the region writes no field, the value is the one the path reads at
/// entry. With no liveness, none is. Each register holding a capture's value
/// (`receivers::captured`) comes too, rooted at the capture with no steps.
fn held_registers(
    instructions: &[WideInstruction],
    sites: &[FieldSite],
    header_live: Option<RegisterSet>,
    objects: &std::collections::BTreeSet<Register>,
) -> std::collections::BTreeMap<Register, FieldPath> {
    let Some(header_live) = header_live else { return std::collections::BTreeMap::new() };
    // A register holding a capture's value roots the paths read through it, with no steps.
    let mut held = receivers::captured(instructions, header_live);
    let definitions = |register: Register| {
        instructions.iter().copied().filter(move |instruction| number_definition(*instruction) == Some(register))
    };
    let mut pending: std::collections::BTreeSet<Register> = instructions
        .iter()
        .copied()
        .filter_map(object_reads)
        .chain(objects.iter().copied())
        .filter(|register| {
            !crate::compile::liveness::register_in(header_live, usize::from(*register))
                && definitions(*register).next().is_some()
                && definitions(*register).all(|instruction| instruction.op().is_get_field())
        })
        .collect();
    // A path through a register still pending waits for it; a register whose definitions
    // read different paths, or wait on one another, holds no field.
    loop {
        let mut decided = Vec::new();
        for register in pending.iter().copied() {
            let mut paths = Vec::new();
            for instruction in definitions(register) {
                let waits = field_base(instruction, sites).and_then(FieldBase::register_index);
                if waits.is_some_and(|base| pending.contains(&base)) {
                    paths.clear();
                    break;
                }
                paths.push(field_path(instruction, sites, &held));
            }
            if paths.is_empty() {
                continue;
            }
            let first = paths[0].clone();
            let one = first.as_ref().filter(|first| paths.iter().all(|path| path.as_ref().is_some_and(|path| path.reads_as(first))));
            decided.push((register, one.cloned()));
        }
        if decided.is_empty() {
            return held;
        }
        for (register, path) in decided {
            pending.remove(&register);
            if let Some(path) = path {
                held.insert(register, path);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FieldUse {
    pub(crate) receiver: FieldReceiver,
    pub(crate) atom: crate::bytecode::Atom,
    pub(crate) cache_site: u16,
    pub(crate) tagged: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FieldStore {
    /// `this`, or a register the region never writes.
    pub(crate) receiver: FieldReceiver,
    pub(crate) atom: crate::bytecode::Atom,
    pub(crate) cache_site: u16,
}

impl FieldStore {
    fn matches(&self, field: &FieldUse) -> bool {
        field.receiver == self.receiver && field.atom == self.atom
    }
}

#[derive(Clone, Copy)]
struct IdentityBranch {
    register: Register,
}

/// A nullish comparison branch that can be emitted from the tagged value word. Strict
/// identity compares one constant. Loose equality against null or undefined matches either
/// nullish value; Abstract Equality does not coerce an object when the other operand is null.
fn identity_branch(instruction: WideInstruction, constants: &[Value]) -> Option<IdentityBranch> {
    if !instruction.op().is_jump_binary_false() {
        return None;
    }
    super::super::operations::equality_operator(instruction.binary_operator_field())?;
    let (register, constant) = match (instruction.operand_b().kind()?, instruction.operand_c().kind()?) {
        (OperandKind::Register, OperandKind::Constant) => (
            instruction.operand_b().payload(),
            *constants.get(usize::from(instruction.operand_c().payload()))?,
        ),
        (OperandKind::Constant, OperandKind::Register) => (
            instruction.operand_c().payload(),
            *constants.get(usize::from(instruction.operand_b().payload()))?,
        ),
        _ => return None,
    };
    let nullish_constant = constant.is_null() || constant.is_undefined();
    nullish_constant.then_some(IdentityBranch { register })
}

/// Registers whose bytecode definitions must stay tagged because a nullish comparison
/// branch observes them. Propagate that fact along moves and through constants; other
/// definitions need a general value denotation before they can be admitted.
fn tagged_registers_with_failure(
    instructions: &[WideInstruction],
    constants: &[Value],
    held: &std::collections::BTreeSet<Register>,
    dynamic_arrays: &std::collections::BTreeSet<Register>,
    header: u32,
) -> Result<std::collections::BTreeSet<Register>, usize> {
    let numeric_uses: std::collections::BTreeSet<Register> = instructions
        .iter()
        .copied()
        .filter(|instruction| !instruction.op().is_move())
        .flat_map(number_reads)
        .collect();
    let mut tagged: std::collections::BTreeSet<Register> = instructions
        .iter()
        .filter_map(|instruction| identity_branch(*instruction, constants).map(|branch| branch.register))
        .chain(
            instructions
                .iter()
                .filter(|instruction| {
                    (instruction.op().is_unary()
                        && instruction.unary_operator() == crate::bytecode::UNARY_TYPEOF_INDEX)
                        || number_comparison(**instruction).is_some()
                })
                .map(|instruction| instruction.result_register()),
        )
        // An entry array's register; a dynamic array's (one the region defines) is tagged by
        // its values' webs instead (`representations`), as its register may hold Numbers too.
        .chain(instructions.iter().filter_map(|instruction| array_read(*instruction))
            .filter(|register| !dynamic_arrays.contains(register)))
        .chain(instructions.iter().enumerate().flat_map(|(offset, instruction)| {
            if !instruction.op().is_ordinary_call() {
                return Vec::new();
            }
            // Keep field-resolved calls under the existing inliner's representation and
            // ownership rules. Dynamic/direct calls leave at this exact PC, so their input
            // window must remain boxed through preceding Moves.
            if instructions[..offset]
                .iter()
                .rev()
                .find(|candidate| candidate.defined_register() == Some(instruction.register_b()))
                .is_some_and(|definition| definition.op().is_get_field())
            {
                return Vec::new();
            }
            let base = ImmediateLayout::call_window_base(instruction.imm());
            let count = if instruction.op() == Op::CallArray {
                1
            } else {
                usize::from(ImmediateLayout::argument_count(instruction.imm()))
            };
            let mut registers = vec![instruction.register_b()];
            for argument in (0..count).filter_map(|offset| {
                usize::from(base).checked_add(offset).and_then(|register| u16::try_from(register).ok())
            }) {
                let mut value = argument;
                let mut moved_argument = false;
                let mut numeric_use = false;
                let mut moved_chain = std::collections::BTreeSet::new();
                loop {
                    if !moved_chain.insert(value) {
                        break;
                    }
                    if numeric_uses.contains(&value) {
                        numeric_use = true;
                        break;
                    }
                    let numeric_move_alias = instructions.iter().any(|candidate| {
                        candidate.op().is_move()
                            && candidate.register_b() == value
                            && numeric_uses.contains(&candidate.result_register())
                    });
                    if numeric_move_alias {
                        numeric_use = true;
                        break;
                    }
                    let Some(definition) = instructions[..offset]
                        .iter()
                        .rev()
                        .find(|candidate| candidate.defined_register() == Some(value))
                    else {
                        break;
                    };
                    if !definition.op().is_move() {
                        break;
                    }
                    moved_argument = true;
                    value = definition.register_b();
                }
                if moved_argument && !numeric_use {
                    registers.push(argument);
                }
            }
            registers
        }))
        .chain(instructions.iter().filter_map(|instruction| {
            if !instruction.op().is_load_const() {
                return None;
            }
            constants
                .get(instruction.constant_index())?
                .as_number()
                .is_none()
                .then_some(instruction.result_register())
        }))
        .chain(held.iter().copied())
        // A field receiver the region's field reads give holds an object.
        .chain(instructions.iter().filter(|instruction| instruction.op().is_get_field()).filter_map(|instruction| {
            let receiver = object_reads(*instruction)?;
            field_reads_alone_define(instructions, receiver).then_some(receiver)
        }))
        // A branch on a value only field reads give tests its truthiness tagged:
        // a field may hold an object.
        .chain(instructions.iter().filter(|instruction| instruction.op().is_truthiness_jump()).filter_map(|instruction| {
            let value = instruction.register_a();
            field_reads_alone_define(instructions, value).then_some(value)
        }))
        // A store whose receiver the region redefines passes its receiver, and a value only
        // field reads give (which may be an object), tagged to the VM's cached store
        // (`LoopRegion::dynamic_field_stores`); a Number result is boxed at the store.
        .chain(instructions.iter().filter_map(|instruction| instruction.field_store_register().filter(|&receiver|
            instructions.iter().any(|candidate| candidate.defined_register() == Some(receiver)))))
        // Any store of a value only field reads give stores it tagged: it may be an object,
        // which the entry's barrier on the receiver covers (`lane_loop_entry`).
        .chain(instructions.iter().filter(|instruction| admitted_store(instruction.op())).filter_map(|instruction| {
            let value = instruction.register_a();
            field_reads_alone_define(instructions, value).then_some(value)
        }))
        .collect();
    loop {
        let before = tagged.len();
        for (offset, instruction) in instructions.iter().copied().enumerate().rev() {
            let Some(result) = number_definition(instruction) else {
                continue;
            };
            if instruction.op().is_move() {
                let source = instruction.register_b();
                if tagged.contains(&result) || tagged.contains(&source) {
                    tagged.insert(result);
                    tagged.insert(source);
                }
                continue;
            }
            if !tagged.contains(&result) {
                continue;
            }
            match instruction.op() {
                Op::LoadConst => {
                    if constants.get(instruction.constant_index()).is_none() {
                        if crate::trace::Channel::OsrReject.on() {
                            eprintln!(
                                "QUENCH_JS_JIT_OSR_TAGGED_REJECT header={header} offset={offset} reason=missing_constant result={result}"
                            );
                        }
                        return Err(offset);
                    }
                }
                // Entry captures and cached fields can carry arbitrary boxed values.
                Op::LoadCapture | Op::GetField | Op::GetIndex => {}
                Op::Binary if number_comparison(instruction).is_some() => {}
                // The type-name index is still a boxed Number at bytecode level. Its
                // lowering reads only immediate tags and exits before the operation for
                // heap values, whose language type depends on VM metadata.
                Op::Unary if instruction.unary_operator() == crate::bytecode::UNARY_TYPEOF_INDEX => {
                    tagged.insert(instruction.register_b());
                }
                _ => {
                    if crate::trace::Channel::OsrReject.on() {
                        eprintln!(
                            "QUENCH_JS_JIT_OSR_TAGGED_REJECT header={header} offset={offset} reason=unsupported_tagged_definition op={:?} result={result}",
                            instruction.op()
                        );
                    }
                    return Err(offset);
                }
            }
        }
        if tagged.len() == before {
            return Ok(tagged);
        }
    }
}

/// A call an inlining region runs in place (`inline_region`): the callee's instructions in
/// the caller's region, its registers above the caller's from `base`, its numeric
/// parameters the call's arguments and the others the caller registers they were moved
/// from, its receiver the caller's.
#[derive(Clone, Copy, Debug)]
pub(crate) enum InlineGuard {
    Field(usize),
    /// The callee's read at this region offset, kept in the region because its receiver
    /// changes in the loop: native code compares its value with the callee right after it,
    /// leaving before the read where it differs.
    AtRead,
    /// A depth-zero plain capture, checked by the VM at every native entry.
    Capture(u16),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct InlineFrame {
    /// The caller's PC of the call.
    pub(crate) call: u32,
    /// The function inlined; `guard` verifies its current field or capture at entry.
    pub(crate) callee: Value,
    /// The callee's function id.
    pub(crate) function: u32,
    /// The caller register the call read the callee from.
    pub(crate) callee_register: Register,
    /// The call's first argument register and count.
    pub(crate) window: Register,
    pub(crate) arguments: u16,
    /// The caller register the call's result lands in.
    pub(crate) result: Register,
    pub(crate) guard: InlineGuard,
    /// The caller register holding the call's receiver, which the region never writes.
    pub(crate) receiver: Register,
    /// The callee's register 0 in the region.
    pub(crate) base: Register,
    pub(crate) registers: u16,
    /// The callee's parameters bound to caller registers, which hold its values for them.
    pub(crate) bound: [Option<Register>; MAX_INLINE_PARAMS],
}

/// The parameters an inlined callee may have, each bound or numeric (`InlineFrame::bound`).
pub(crate) const MAX_INLINE_PARAMS: usize = 8;

/// Where an inlining region's PC comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// The caller's instruction at this PC.
    Caller(u32),
    /// The instruction at `pc` of `frame`'s callee.
    Inlined { frame: u16, pc: u32 },
    /// No instruction: a jump leaving the region, which the caller continues at this PC.
    Leave(u32),
}

/// An inlining region's PCs and frames. Its instructions take consecutive PCs from the
/// header, then each jump out of the region takes one past them (`Origin::Leave`).
pub(crate) struct Inlining {
    /// By offset from the header.
    pub(crate) origins: Vec<Origin>,
    pub(crate) frames: Vec<InlineFrame>,
    /// The captures inlined callees read, by the frame and its depth-0 slot: a callee's
    /// `LoadCapture` reads `CALLEE_CAPTURE` with its index here, which the entry resolves
    /// in the callee's own environment (`lane_loop_entry`).
    pub(crate) callee_captures: Vec<(usize, u16)>,
    /// The caller's register count: every register from it on is an inlined callee's.
    pub(crate) caller_registers: u16,
    /// The caller's backward jump PC: the last caller PC of the loop it closes.
    pub(crate) latch: u32,
}

/// The capture slot bit an inlined callee's capture read carries (`Inlining::callee_captures`):
/// no function's own depth-0 slot reaches it, which `inline_region` checks.
pub(crate) const CALLEE_CAPTURE: u16 = 1 << 15;
/// The capture slot that carries the activation's `this` into a region whose field reads of
/// `this` read its current slots; no function's slot reaches it.
pub(crate) const THIS_CAPTURE: u16 = CALLEE_CAPTURE - 1;

/// The exit PC native code reports for an inlined callee's instruction: its region PC with
/// this bit, which no function PC has (`MAX_REGION_INSTRUCTIONS`).
pub(crate) const INLINED_EXIT: u32 = 1 << 31;

/// What `inline_region` needs of a call's callee.
pub(crate) struct InlineCallee<'a> {
    pub(crate) callee: Value,
    pub(crate) function: u32,
    pub(crate) params: u16,
    pub(crate) registers: u16,
    pub(crate) instructions: Vec<WideInstruction>,
    pub(crate) live: &'a crate::compile::liveness::RegisterMasks,
}

/// A field read whose value the bytecode's rewrite also stores into a `this` field (the
/// `GetSetThis` fusion: `writes_current_this` with a site's sink), as the instructions it
/// fused: the read of the site's path into the result register, then the `SetThisField`.
/// `None` for any other instruction.
fn inlined_set_this(instruction: WideInstruction, sites: &[FieldSite]) -> Option<Vec<WideInstruction>> {
    (instruction.op().is_get_field() && instruction.result_flags() == crate::bytecode::SET_THIS_REGISTER)
        .then_some(())?;
    // A site's base names a register of the callee in the site table, which renaming does not
    // reach: the read runs as one named read per step from the renamed base.
    let FieldLookup::Site(index) = instruction.field_lookup()? else { return None };
    let site = *sites.get(index)?;
    let result = instruction.result_register();
    let mut parts = vec![WideInstruction::new(Op::GetField, result, site.base.0, site.first.1, site.first.0)];
    if let Some((atom, cache_site)) = site.second {
        parts.push(WideInstruction::new(Op::GetField, result, FieldBase::register(result).0, cache_site, atom));
    }
    let (sink_atom, sink_site) = site.sink?;
    parts.push(WideInstruction::new(Op::SetThisField, result, 0, sink_site, sink_atom));
    Some(parts)
}

/// Whether an inlined callee's instruction computes its result and returns it in one
/// (`RETURN_REGISTER` alone, of a value-producing instruction), which inlining splits into
/// the computation and the `Return` it makes.
pub(super) fn inlined_returning(instruction: WideInstruction) -> bool {
    instruction.result_flags() == crate::bytecode::RETURN_REGISTER
        && !instruction.op().is_return()
        && instruction.op().field_layout(InstructionField::A) == FieldLayout::ResultRegister
        && instruction.defined_register().is_some()
}

/// The ops an inlined callee's instructions may be: those `admit` lowers, its `Return`, and
/// none that names its function's own state (captures, frame, calls).
fn inlinable(op: Op) -> bool {
    op.traits().has(crate::bytecode::Traits::INLINABLE)
}


/// `instruction` with each register `register` renames, a `this` field base made the
/// `this` register, and a jump target `target` renames; `None` for a field it cannot
/// rename, or a register with flags.
pub(super) fn renamed(
    instruction: WideInstruction,
    register: &dyn Fn(Register) -> Register,
    this: Register,
    target: &dyn Fn(u32) -> Option<u32>,
) -> Option<WideInstruction> {
    let op = instruction.op();
    let rename = |field: InstructionField, raw: u16| -> Option<u16> {
        Some(match op.field_layout(field) {
            FieldLayout::Unused
            | FieldLayout::BooleanFlag
            | FieldLayout::BinaryOperator
            | FieldLayout::CacheSiteIndex
            | FieldLayout::FieldLookupCacheSiteIndex => raw,
            FieldLayout::ResultRegister
            | FieldLayout::Register
            | FieldLayout::WriteRegister
            | FieldLayout::ReadWriteRegister => {
                (raw <= Operand::PAYLOAD_MASK).then_some(())?;
                register(raw)
            }
            FieldLayout::Operand | FieldLayout::NumericIndexOperand => {
                let operand = Operand(raw);
                match operand.kind()? {
                    OperandKind::Register => Operand::register(register(operand.payload())).0,
                    OperandKind::Constant => raw,
                    OperandKind::Field | OperandKind::Local => return None,
                }
            }
            FieldLayout::FieldBase => {
                let base = FieldBase(raw);
                if base == FieldBase::THIS {
                    FieldBase::register(this).0
                } else {
                    FieldBase::register(register(base.register_index()?)).0
                }
            }
            _ => return None,
        })
    };
    let a = rename(InstructionField::A, instruction.a())?;
    let b = rename(InstructionField::B, instruction.b())?;
    let c = rename(InstructionField::C, instruction.c())?;
    let imm = match op.immediate_role() {
        ImmediateRole::JumpTarget => target(instruction.imm())?,
        // A call's argument window renames as one block: each of its registers to the one
        // after the base's rename.
        _ if op == Op::Call => {
            let imm = instruction.imm();
            let base = ImmediateLayout::call_window_base(imm);
            let count = ImmediateLayout::argument_count(imm);
            let renamed_base = register(base);
            (0..count).all(|offset| register(base + offset) == renamed_base + offset).then_some(())?;
            ImmediateLayout::call_immediate(renamed_base, count, ImmediateLayout::direct_eval(imm), ImmediateLayout::parameter_eval(imm))
        }
        _ => instruction.imm(),
    };
    Some(WideInstruction::new(op, a, b, c, imm))
}

/// The registers an instruction reads as objects: a field read's receiver, an indexed
/// access's array.
fn object_reads(instruction: WideInstruction) -> Option<Register> {
    match instruction.op() {
        Op::GetField => match instruction.field_lookup()? {
            FieldLookup::Atom { base, .. } => base.register_index(),
            _ => None,
        },
        _ if let Some(array) = array_read(instruction) => Some(array),
        _ => instruction.field_store_register(),
    }
}

/// Whether `register` has definitions in `instructions` and every one is a field read, which
/// may give an object.
fn field_reads_alone_define(instructions: &[WideInstruction], register: Register) -> bool {
    let mut definitions = instructions.iter().filter(|candidate| candidate.defined_register() == Some(register)).peekable();
    definitions.peek().is_some() && definitions.all(|definition| definition.op().is_get_field())
}

/// The register an indexed access, or the coercibility check before one, reads as its array.
fn array_read(instruction: WideInstruction) -> Option<Register> {
    match instruction.op() {
        Op::GetIndex => instruction.operand_b().register_index(),
        Op::SetIndex | Op::RequireObjectCoercible => Some(instruction.register_b()),
        _ => None,
    }
}

/// Whether a loop region admits `op` as a named field store: every one the opcode table
/// names (`Op::field_store`) but a strict store to `this`.
fn admitted_store(op: Op) -> bool {
    op.field_store().is_some_and(|store| !(store.strict && store.receiver == crate::bytecode::StoreReceiver::This))
}

/// Whether `instruction` names `register` (a read or a write); an instruction `renamed`
/// cannot rename names every register.
fn mentions(instruction: WideInstruction, register: Register, sites: &[FieldSite]) -> bool {
    // Only a GetField carries a field lookup immediate, so the op is checked first.
    if instruction.op().is_get_field() && matches!(instruction.field_lookup(), Some(FieldLookup::Site(_))) {
        return instruction.result_register() == register
            || field_base(instruction, sites).is_none_or(|base| base.register_index() == Some(register));
    }
    let named = std::cell::Cell::new(false);
    let rename = |name: Register| {
        named.set(named.get() || name == register);
        name
    };
    renamed(instruction, &rename, register, &|target| Some(target)).is_none() || named.get()
}

/// The ops a call's argument setup may run between reading its callee and the call: none
/// has an effect a second run could repeat, so leaving among them resumes at the read.
fn pure_setup(op: Op) -> bool {
    op.traits().has(crate::bytecode::Traits::PURE_SETUP)
}


/// The region of the loop from `header` to `backedge` with each `Call` in it inlined: the
/// call's callee (`resolve` of the cached field it is read from) runs in place, its registers
/// above the caller's, when it is a function of plain instructions whose every parameter is
/// a Number argument or an object a caller register holds unchanged, and the whole region
/// then is one the tier lowers.
///
/// The call's setup, from its callee's read to the call, is a straight run of `pure_setup`
/// instructions writing no register live at the read: leaving there resumes at the read,
/// which reruns them. The read itself and the moves of the object arguments drop out, so the
/// registers they write, which the setup's function may reuse otherwise, hold nothing for
/// the region. Leaving at a callee's instruction makes the callee's activation
/// (`INLINED_EXIT`).
#[allow(clippy::too_many_arguments)]
pub(super) fn inline_region<'a>(
    function: &Function,
    decoded: &[WideInstruction],
    constants: &[Value],
    sites: &[FieldSite],
    live: &crate::compile::liveness::RegisterMasks,
    header: u32,
    backedge: u32,
    resolve: &mut dyn FnMut(&[FieldUse], Option<u16>) -> Option<InlineCallee<'a>>,
) -> Option<LoopRegion> {
    let (start, end) = (header as usize, backedge as usize);
    (start <= end && end < decoded.len()).then_some(())?;
    let caller = &decoded[start..=end];
    let defines = |register: Register| caller.iter().any(|instruction| instruction.defined_register() == Some(register));
    let targets: std::collections::BTreeSet<u32> = caller
        .iter()
        .filter(|instruction| instruction.op().immediate_role() == ImmediateRole::JumpTarget)
        .map(|instruction| instruction.imm())
        .collect();
    let mask = |pc: u32| live.get(pc as usize);
    let caller_registers = function.registers;
    let mut total = u32::from(caller_registers);
    struct Site<'b> {
        frame: InlineFrame,
        callee: InlineCallee<'b>,
        /// The path the callee's read reads it through, from a root the region does not
        /// write (a receiver given its own register reads through its path).
        callee_path: Option<FieldPath>,
        /// Where leaving in the call's setup resumes: the receiver's own read, else the
        /// callee's.
        resume: u32,
        window: Register,
        result: Register,
        reached: crate::bitset::BitSet,
    }
    let mut calls: std::collections::BTreeMap<u32, Site<'a>> = std::collections::BTreeMap::new();
    // The caller PCs whose instructions drop out, the setup PCs by the PC they resume at, and
    // the caller instructions the region runs rewritten.
    let mut dropped = std::collections::BTreeSet::new();
    let mut setup: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
    let mut rewritten: std::collections::BTreeMap<u32, WideInstruction> = std::collections::BTreeMap::new();
    // The names the loop stores: a receiver read through one changes in the loop.
    let caller_stored: std::collections::BTreeSet<crate::bytecode::Atom> = caller
        .iter()
        .filter_map(|instruction| match instruction.op() {
            op if admitted_store(op) => Some(instruction.atom_index()),
            // A fused read the bytecode stores into a `this` field (`inlined_set_this`).
            Op::GetField if instruction.writes_current_this() => match instruction.field_lookup()? {
                FieldLookup::Site(index) => sites.get(index)?.sink.map(|(atom, _)| atom),
                FieldLookup::Atom { .. } => None,
            },
            _ => None,
        })
        .collect();
    let mut checked_reads: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
    let captured = receivers::captured(caller, mask(header)?);
    for (offset, instruction) in caller.iter().copied().enumerate() {
        if !instruction.op().is_call() {
            continue;
        }
        let pc = header + offset as u32;
        (instruction.result_flags() == 0 && !instruction.direct_eval() && !instruction.parameter_eval()).then_some(())?;
        let argc = ImmediateLayout::argument_count(instruction.imm());
        let window = ImmediateLayout::call_window_base(instruction.imm());
        let (function_register, mut receiver) = (instruction.register_b(), instruction.register_c());
        // The callee's read: the last write of its register before the call, in a straight run.
        let read_offset = (0..offset).rev().find(|index| caller[*index].defined_register() == Some(function_register))?;
        let read = header + read_offset as u32;
        let definition = caller[read_offset];
        let capture = (definition.op().is_load_capture()
            && definition.capture_depth() == 0 && definition.result_flags() == 0)
            .then(|| definition.capture_slot());
        let mut callee_path = if capture.is_some() { None } else {
            Some(field_path(definition, sites, &std::collections::BTreeMap::new())?)
        };
        // A receiver the region writes is the call's own: a field read right before the
        // callee's, which reads the callee from it (`this.m.am(…)`), and nothing else
        // reads until the call, nor after it. The region gives that read a register of its
        // own, holding the field's value (`held_registers`), and the setup resumes at it.
        let mut resume_offset = read_offset;
        let checked = defines(receiver) && receivers::stored(caller, read_offset, receiver, sites, &|atom| caller_stored.contains(&atom));
        if checked {
            checked_reads.insert(pc, read);
            // The callee resolves now through the receiver's own path; native code checks
            // the read each iteration, so the path is never an entry snapshot.
            let own = read_offset - 1;
            let receiver_path = field_path(caller[own], sites, &std::collections::BTreeMap::new())?;
            let callee_path = callee_path.as_mut()?;
            (callee_path.root == FieldReceiver::Register(receiver)).then_some(())?;
            callee_path.root = receiver_path.root;
            callee_path.steps.splice(0..0, receiver_path.steps);
        } else if let Some((root, held)) = receivers::rerooted(caller, read_offset, receiver, &defines, callee_path.as_ref(), &captured) {
            (callee_path.as_mut()?.root, receiver) = (root, held);
        } else if defines(receiver) {
            let callee_path = callee_path.as_mut()?;
            let own = read_offset.checked_sub(1)?;
            let receiver_path = field_path(caller[own], sites, &std::collections::BTreeMap::new())?;
            (caller[own].defined_register() == Some(receiver)
                && receiver_path.root != FieldReceiver::Register(receiver)
                && callee_path.root == FieldReceiver::Register(receiver)
                && callee_path.steps.len() == 1
                && caller[read_offset + 1..offset].iter().all(|instruction| !mentions(*instruction, receiver, sites))
                && !crate::compile::liveness::register_in(mask(pc + 1)?, usize::from(receiver)))
            .then_some(())?;
            let fresh = Register::try_from(total).ok()?;
            total += 1;
            rewritten.insert(header + own as u32, WideInstruction::new(Op::GetField, fresh, caller[own].b(), caller[own].c(), caller[own].imm()));
            callee_path.root = receiver_path.root;
            callee_path.steps.splice(0..0, receiver_path.steps);
            receiver = fresh;
            resume_offset = own;
        }
        let resume = header + resume_offset as u32;
        let at_resume = mask(resume)?;
        let run = &caller[resume_offset + 1..offset];
        (run.iter().all(|instruction| pure_setup(instruction.op()))
            && (resume + 1..=pc).all(|pc| !targets.contains(&pc))
            && run.iter().all(|instruction| {
                instruction
                    .defined_register()
                    .is_none_or(|register| !crate::compile::liveness::register_in(at_resume, usize::from(register)))
            }))
        .then_some(())?;
        // The callee as the read finds it now.
        let mut chain = Vec::new();
        if let Some(path) = &callee_path { intern_field(&mut chain, path, true); }
        let callee = resolve(&chain, capture)?;
        (callee.params == argc && usize::from(argc) <= MAX_INLINE_PARAMS).then_some(())?;
        let reached = crate::bytecode::reachable_from(&callee.instructions, [0]);
        let body = || callee.instructions.iter().copied().enumerate().filter(|(pc, _)| reached.contains(*pc)).map(|(_, instruction)| instruction);
        body().all(|instruction| inlinable(instruction.op()) && (instruction.result_flags() == 0 || inlined_returning(instruction) || inlined_set_this(instruction, sites).is_some())).then_some(())?;
        // A captured call may bind undefined to the global receiver. Keep this slice
        // receiver-independent; recovery still performs the ordinary binding below.
        if capture.is_some() {
            body().all(|instruction| !instruction.op().is_get_field()
                || field_base(instruction, sites) != Some(FieldBase::THIS)).then_some(())?;
        }
        // A parameter read as an object is bound to the caller register its argument was
        // moved from in the setup, which the region never writes; any other is a Number it
        // copies.
        let mut bound = [None; MAX_INLINE_PARAMS];
        for parameter in 0..argc {
            if !body().any(|instruction| object_reads(instruction) == Some(parameter)) {
                continue;
            }
            body().all(|instruction| instruction.defined_register() != Some(parameter)).then_some(())?;
            let argument = window + parameter;
            let moved = (read_offset + 1..offset).rev().find(|index| caller[*index].defined_register() == Some(argument))?;
            (caller[moved].op() == Op::Move && !defines(caller[moved].register_b())).then_some(())?;
            bound[usize::from(parameter)] = Some(caller[moved].register_b());
            dropped.insert(header + moved as u32);
        }
        if !checked {
            dropped.insert(read);
        }
        for setup_pc in resume..pc {
            setup.insert(setup_pc, resume);
        }
        let base = Register::try_from(total).ok()?;
        total += u32::from(callee.registers);
        (total <= RegisterSet::BITS).then_some(())?;
        calls.insert(
            pc,
            Site {
                frame: InlineFrame {
                    call: pc,
                    callee: callee.callee,
                    function: callee.function,
                    callee_register: function_register,
                    window,
                    arguments: argc,
                    result: instruction.result_register(),
                    guard: capture.map_or(InlineGuard::Field(usize::MAX), InlineGuard::Capture),
                    receiver,
                    base,
                    registers: callee.registers,
                    bound,
                },
                callee_path: callee_path.clone(),
                resume,
                window,
                result: instruction.result_register(),
                reached,
                callee,
            },
        );
    }
    (!calls.is_empty()).then_some(())?;
    // Region PCs: each kept caller instruction takes one, a call its numeric parameters'
    // moves and its callee's reached instructions, each `Return` two (the result's move and
    // the jump past the call). A dropped instruction's PC is the next instruction's.
    let mut caller_flat = Vec::with_capacity(caller.len());
    let mut callee_flat: std::collections::BTreeMap<u32, Vec<u32>> = std::collections::BTreeMap::new();
    let mut length = 0u32;
    for (offset, caller_instruction) in caller.iter().enumerate() {
        let pc = header + offset as u32;
        caller_flat.push(header + length);
        if dropped.contains(&pc) {
            continue;
        }
        let Some(site) = calls.get(&pc) else {
            // A fused field read the caller stores into its `this` field runs as its parts,
            // each leaving at the caller's PC, which reruns the pure read.
            length += inlined_set_this(*caller_instruction, sites).map_or(1, |parts| parts.len() as u32);
            continue;
        };
        length += site.frame.bound[..usize::from(site.callee.params)].iter().filter(|bound| bound.is_none()).count() as u32;
        let mut flat = Vec::with_capacity(site.callee.instructions.len());
        for (index, instruction) in site.callee.instructions.iter().enumerate() {
            flat.push(header + length);
            if site.reached.contains(index) {
                length += if instruction.op().is_return() {
                    2
                } else if inlined_returning(*instruction) {
                    3
                } else {
                    inlined_set_this(*instruction, sites).map_or(1, |parts| parts.len() as u32)
                };
            }
        }
        callee_flat.insert(pc, flat);
    }
    (length as usize <= MAX_REGION_INSTRUCTIONS).then_some(())?;
    let mut callee_captures: Vec<(usize, u16)> = Vec::new();
    let mut origins: Vec<Origin> = Vec::with_capacity(length as usize);
    let mut masks: Vec<RegisterSet> = Vec::with_capacity(length as usize);
    let mut leaves: Vec<u32> = Vec::new();
    let mut instructions = Vec::with_capacity(length as usize);
    let caller_target = |target: u32, leaves: &mut Vec<u32>| -> u32 {
        if (header..=backedge).contains(&target) {
            return caller_flat[(target - header) as usize];
        }
        let index = leaves.iter().position(|leave| *leave == target).unwrap_or_else(|| {
            leaves.push(target);
            leaves.len() - 1
        });
        header + length + index as u32
    };
    for (offset, instruction) in caller.iter().copied().enumerate() {
        let pc = header + offset as u32;
        if dropped.contains(&pc) {
            continue;
        }
        // A setup instruction leaves as the first of its run does.
        let resumes = setup.get(&pc).copied().unwrap_or(pc);
        let instruction = rewritten.get(&pc).copied().unwrap_or(instruction);
        let Some(site) = calls.get(&pc) else {
            // A caller instruction keeps its registers; a jump takes the region PC of its target.
            let renamed = if instruction.op().immediate_role() == ImmediateRole::JumpTarget {
                let target = caller_target(instruction.imm(), &mut leaves);
                WideInstruction::new(instruction.op(), instruction.a(), instruction.b(), instruction.c(), target)
            } else {
                instruction
            };
            let parts = inlined_set_this(renamed, sites).unwrap_or_else(|| vec![renamed]);
            let resume_mask = mask(resumes)?;
            for part in parts {
                instructions.push(part);
                origins.push(Origin::Caller(resumes));
                masks.push(resume_mask);
            }
            continue;
        };
        let frame = site.frame;
        let index = calls.keys().position(|key| *key == pc)? as u16;
        let flat = &callee_flat[&pc];
        let register = |register: Register| match frame.bound.get(usize::from(register)).copied().flatten() {
            Some(source) => source,
            None => frame.base + register,
        };
        // A callee's live registers there, in the region, with the caller's live past the call.
        let after = mask(pc + 1)? & !RegisterSet::bit(usize::from(site.result));
        let frame_mask = |callee_pc: usize| -> Option<RegisterSet> {
            let own = site.callee.live.get(callee_pc)?;
            let mut set = after;
            for callee_register in 0..site.callee.registers {
                if crate::compile::liveness::register_in(own, usize::from(callee_register)) {
                    set |= RegisterSet::bit(usize::from(register(callee_register)));
                }
            }
            Some(set)
        };
        for parameter in 0..site.callee.params {
            if frame.bound[usize::from(parameter)].is_some() {
                continue;
            }
            instructions.push(WideInstruction::new(Op::Move, frame.base + parameter, site.window + parameter, 0, 0));
            origins.push(Origin::Caller(site.resume));
            masks.push(mask(site.resume)?);
        }
        let continuation = caller_flat.get(offset + 1).copied().unwrap_or_else(|| caller_target(pc + 1, &mut leaves));
        for (callee_pc, instruction) in site.callee.instructions.iter().copied().enumerate() {
            if !site.reached.contains(callee_pc) {
                continue;
            }
            let origin = Origin::Inlined { frame: index, pc: callee_pc as u32 };
            let set = frame_mask(callee_pc)?;
            if instruction.op().is_return() {
                instructions.push(WideInstruction::new(Op::Move, site.result, register(instruction.register_a()), 0, 0));
                instructions.push(WideInstruction::new(Op::Jump, 0, 0, 0, continuation));
                origins.extend([origin, origin]);
                masks.extend([set, set]);
                continue;
            }
            let target = |target: u32| flat.get(target as usize).copied().filter(|_| site.reached.contains(target as usize));
            // A returning instruction (`result = op; return result`) computes into its own
            // register, then completes the call as the callee's `Return` does.
            if inlined_returning(instruction) {
                let plain = WideInstruction::new(instruction.op(), instruction.result_register(), instruction.b(), instruction.c(), instruction.imm());
                instructions.push(renamed(plain, &register, frame.receiver, &target)?);
                instructions.push(WideInstruction::new(Op::Move, site.result, register(instruction.result_register()), 0, 0));
                instructions.push(WideInstruction::new(Op::Jump, 0, 0, 0, continuation));
                origins.extend([origin, origin, origin]);
                masks.extend([set, set, set]);
                continue;
            }
            // A fused read stored into a `this` field runs as its read, then the store.
            let parts = inlined_set_this(instruction, sites).unwrap_or_else(|| vec![instruction]);
            for instruction in parts {
                // A callee's own capture reads its environment, which its identity guard fixes
                // and no instruction of the region stores: the entry snapshots it.
                let instruction = if instruction.op().is_load_capture() {
                    (instruction.capture_depth() == 0 && instruction.capture_slot() < CALLEE_CAPTURE).then_some(())?;
                    let capture = (usize::from(index), instruction.capture_slot());
                    let position = callee_captures.iter().position(|existing| *existing == capture).unwrap_or_else(|| {
                        callee_captures.push(capture);
                        callee_captures.len() - 1
                    });
                    let slot = CALLEE_CAPTURE | u16::try_from(position).ok().filter(|position| *position < CALLEE_CAPTURE)?;
                    WideInstruction::new(Op::LoadCapture, instruction.a(), instruction.b(), instruction.c(), ImmediateLayout::capture_immediate(0, slot))
                } else {
                    instruction
                };
                // The callee's `this` is the call's receiver, a caller register: its store of a
                // `this` field is a store to that register's object.
                let renamed_instruction = if instruction.op().is_set_this_field() {
                    let store = renamed(WideInstruction::new(Op::SetField, instruction.a(), 0, instruction.c(), instruction.imm()), &register, frame.receiver, &target)?;
                    WideInstruction::new(Op::SetField, store.a(), frame.receiver, store.c(), store.imm())
                } else {
                    renamed(instruction, &register, frame.receiver, &target)?
                };
                instructions.push(renamed_instruction);
                origins.push(origin);
                masks.push(set);
            }
        }
    }
    debug_assert_eq!(instructions.len(), length as usize);
    for target in &leaves {
        origins.push(Origin::Leave(*target));
        masks.push(mask(*target)?);
    }
    // A checked callee's register holds the function its read gives, an object.
    let receivers = calls
        .iter()
        .flat_map(|(pc, site)| {
            let callee = checked_reads.contains_key(pc).then(|| caller[(*pc - header) as usize].register_b());
            std::iter::once(site.frame.receiver).chain(callee)
        })
        .collect();
    let header_live = masks.first().copied();
    let mut region = admit_region(
        instructions,
        total as usize,
        constants,
        sites,
        Some(std::rc::Rc::new(crate::compile::liveness::RegisterMasks::new(masks))),
        header_live,
        header,
        &receivers,
    )
    .ok()?;
    // Dynamic receiver reads use the loop's caller registers and exact caller PCs. Keep
    // inlined regions on the established entry-snapshot contract until their multi-frame
    // recovery maps can represent per-iteration cache misses.
    if !region.dynamic_field_reads.is_empty() {
        return None;
    }
    // Each callee's field, which the entry reads and guards still holds it, read through
    // the region's held registers.
    let held: std::collections::BTreeMap<Register, FieldPath> =
        region.held.iter().map(|(register, (path, _))| (*register, path.clone())).chain(captured).collect();
    let mut frames = Vec::with_capacity(calls.len());
    for (call_pc, site) in calls.into_iter() {
        let mut frame = site.frame;
        if checked_reads.contains_key(&call_pc) {
            frame.guard = InlineGuard::AtRead;
            frames.push(frame);
            continue;
        }
        let written = frame.receiver >= caller_registers || defines(frame.receiver);
        (!written || held.contains_key(&frame.receiver)).then_some(())?;
        let Some(FieldPath { root, steps }) = site.callee_path else {
            frames.push(frame);
            continue;
        };
        let path = match root {
            FieldReceiver::Register(register) => match held.get(&register) {
                Some(path) => FieldPath { root: path.root, steps: path.steps.iter().copied().chain(steps).collect() },
                None => FieldPath { root, steps },
            },
            _ => FieldPath { root, steps },
        };
        // The root is `this`, or a register the region does not write (`field_path`).
        if let FieldReceiver::Register(register) = path.root {
            (!defines(register)).then_some(())?;
        }
        frame.guard = InlineGuard::Field(intern_field(&mut region.fields, &path, true));
        frames.push(frame);
    }
    (region.fields.len() <= MAX_REGION_FIELDS).then_some(())?;
    region.inlining = Some(Box::new(Inlining { origins, frames, callee_captures, caller_registers, latch: backedge }));
    Some(region)
}

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

    pub(super) fn entry_registers(&self) -> impl Iterator<Item = Register> + '_ {
        self.numbers.iter().chain(&self.tagged).copied()
    }

    /// Whether the interpreter may read `register` resuming at `pc` before writing it; every
    /// register is, where liveness does not cover the function.
    pub(super) fn is_live(&self, pc: u32, register: Register) -> bool {
        let Some(live) = &self.live else {
            return true;
        };
        // An inlining region's liveness is by its own PCs, from the header.
        let index = if self.inlining.is_some() { (pc - self.header) as usize } else { pc as usize };
        live.is_live(index, register)
    }

    /// The field whose value `register` holds at every read, when it is one the region
    /// writes only with that field's reads (`held_registers`).
    pub(crate) fn held_field(&self, register: Register) -> Option<usize> {
        self.held.get(&register).map(|(_, index)| *index)
    }

    /// Whether native code leaving at the `INLINED_EXIT` PC `pc` wrote back `register`.
    pub(crate) fn wrote_back(&self, pc: u32, register: Register) -> bool {
        self.is_live(pc & !INLINED_EXIT, register)
    }

    /// The frame and callee PC of an `INLINED_EXIT` PC native code reported.
    pub(crate) fn inlined_exit(&self, pc: u32) -> Option<(&InlineFrame, u32)> {
        let inlining = self.inlining.as_ref()?;
        (pc & INLINED_EXIT != 0).then_some(())?;
        match inlining.origins.get(((pc & !INLINED_EXIT) - self.header) as usize)? {
            Origin::Inlined { frame, pc } => Some((&inlining.frames[usize::from(*frame)], *pc)),
            Origin::Caller(_) | Origin::Leave(_) => None,
        }
    }
}

/// The entry record native code reads: each capture's value, then each cached field's,
/// then each array's element pointer, length and capacity, then each guarded field-store
/// pointer. Only the words pushed are ever read, by native code and by `captures` and
/// `field_value`, so the rest stay uninitialized.
pub(crate) struct LoopEntry {
    words: [std::mem::MaybeUninit<u64>; ENTRY_WORDS],
    length: usize,
    /// An inlining region's callee registers, which leaving at an inlined instruction
    /// writes back here for its activation.
    scratch: [std::mem::MaybeUninit<u64>; MAX_REGION_REGISTERS],
}

/// The longest array a region indexes: a negative `i32` index zero-extends past it, so one
/// unsigned comparison bounds every `i32` index.
pub(super) const MAX_INDEXED_LENGTH: usize = i32::MAX as usize;
/// The first record word counts executions of the region's own backward edge: the iterations
/// a run completed natively, which weigh what its entry paid for (`LoopSite::note_run`).
const ENTRY_BACKEDGE_COUNT: usize = 0;
pub(super) const ENTRY_CAPTURES: usize = 1;

/// The entry word of the region's `index`th field: the single home of the record's field
/// layout, shared by the verified lowering and the lane region's snapshot reads.
pub(crate) fn field_word(region: &LoopRegion, index: usize) -> usize {
    ENTRY_CAPTURES + region.captures.len() + index
}
/// An array's entry words: its element pointer, its length, then its capacity (which only
/// a written array's appends read).
pub(super) const ARRAY_WORDS: usize = 3;
const FIELD_STORE_WORDS: usize = 1;
/// The heap views native reads use (`LoopEntry::push_native_heap`): the object records'
/// base, the property values' base and count, and the array records' base.
const NATIVE_HEAP_WORDS: usize = 4;
/// The scratch registers' address an inlining region's record ends with.
const SCRATCH_WORDS: usize = 1;
const ENTRY_WORDS: usize =
    ENTRY_CAPTURES
        + MAX_REGION_CAPTURES
        + ARRAY_WORDS * MAX_REGION_ARRAYS
        + MAX_REGION_FIELDS
        + FIELD_STORE_WORDS * MAX_REGION_FIELD_STORES
        + NATIVE_HEAP_WORDS
        + SCRATCH_WORDS;

impl LoopEntry {
    pub(crate) fn new() -> Self {
        let mut words = [std::mem::MaybeUninit::uninit(); ENTRY_WORDS];
        // The backward-edge counter is the one leading word native code reads before any push.
        words[ENTRY_BACKEDGE_COUNT].write(0);
        Self { words, length: ENTRY_CAPTURES, scratch: [std::mem::MaybeUninit::uninit(); MAX_REGION_REGISTERS] }
    }

    /// Number of taken executions of this region's backward edge in the most recent run.
    pub(crate) fn backedge_count(&self) -> u64 {
        // SAFETY: `new` initializes the counter word.
        unsafe { self.words[ENTRY_BACKEDGE_COUNT].assume_init() }
    }

    /// The record's words. Native OSR may also update the private first counter word.
    pub(super) fn words_mut(&mut self) -> *mut u64 {
        debug_assert!(self.length <= ENTRY_WORDS);
        self.words.as_mut_ptr().cast()
    }

    fn push(&mut self, word: u64) {
        self.words[self.length].write(word);
        self.length += 1;
    }

    /// Captures come first, in the region's order.
    pub(crate) fn push_capture(&mut self, value: Value) {
        self.push(value.to_bits());
    }

    /// The first `count` capture values, as pushed.
    pub(crate) fn captures(&self, count: usize) -> Vec<Value> {
        debug_assert!(ENTRY_CAPTURES + count <= self.length);
        self.words[ENTRY_CAPTURES..ENTRY_CAPTURES + count]
            .iter()
            // SAFETY: the first `count` words after the leading ones are the pushed captures.
            .map(|word| Value::from_bits(unsafe { word.assume_init() }))
            .collect()
    }

    /// Arrays follow the fields, in the region's order. An array longer than
    /// `MAX_INDEXED_LENGTH` declines the entry.
    pub(crate) fn push_array(&mut self, elements: *mut Value, length: usize, capacity: usize) -> Option<()> {
        (length <= MAX_INDEXED_LENGTH).then_some(())?;
        let end = self.length + ARRAY_WORDS;
        self.words[self.length..end].copy_from_slice(&[
            std::mem::MaybeUninit::new(elements.addr() as u64),
            std::mem::MaybeUninit::new(length as u64),
            std::mem::MaybeUninit::new(capacity.min(MAX_INDEXED_LENGTH) as u64),
        ]);
        self.length = end;
        Some(())
    }


    /// A warmed own-data field slot guarded by the VM during entry setup.
    pub(crate) fn push_field_store(&mut self, slot: *mut Value) {
        self.push(slot.addr() as u64);
    }

    /// Heap views for native field reads and array accesses (`NATIVE_HEAP_WORDS`). The
    /// pointers are valid only for this synchronous run and are never retained in compiled
    /// code.
    pub(crate) fn push_native_heap(
        &mut self,
        objects: *const crate::heap::Object,
        properties: *const Value,
        property_len: usize,
        arrays: *const crate::heap::ArrayObject,
    ) {
        self.push(objects.addr() as u64);
        self.push(properties.addr() as u64);
        self.push(property_len as u64);
        self.push(arrays.addr() as u64);
    }

    /// The value of the region's `index`th field as pushed: its word follows the captures
    /// and the fields before it.
    pub(crate) fn field_value(&self, region: &LoopRegion, index: usize) -> Value {
        let word = field_word(region, index);
        debug_assert!(word < self.length);
        // SAFETY: the caller pushed the field, after the captures and the fields before it.
        Value::from_bits(unsafe { self.words[word].assume_init() })
    }

    /// An inlining region's last word: its scratch registers' address.
    pub(crate) fn push_scratch(&mut self) {
        let address = self.scratch.as_mut_ptr().addr() as u64;
        self.push(address);
    }

    /// The value an inlined callee's register `register` of an inlining region took when
    /// native code left at an inlined instruction where it is live.
    ///
    /// # Safety
    /// The exit wrote the register back: it is live at the exit's PC (`LoopRegion::is_live`).
    pub(crate) unsafe fn scratch_value(&self, inlining: &Inlining, register: Register) -> Value {
        // SAFETY: as the caller promises.
        Value::from_bits(unsafe { self.scratch[usize::from(register - inlining.caller_registers)].assume_init() })
    }

    /// A field's value; a Number field's must be a Number.
    pub(crate) fn push_field(&mut self, value: Value, tagged: bool) -> Option<()> {
        (tagged || value.as_number().is_some()).then_some(())?;
        self.push(value.to_bits());
        Some(())
    }
}

/// The arithmetic and comparison a region lowers, by the operator's tag.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Arithmetic {
    Float(NumberArithmetic),
    Int32(super::super::operations::NumberBitwise),
    Remainder,
}

fn arithmetic(instruction: Op, operator: u32) -> Option<Arithmetic> {
    if instruction == Op::Binary && operator == super::super::operations::REMAINDER {
        return Some(Arithmetic::Remainder);
    }
    if let Some(operation) = NumberArithmetic::from_js_instruction(instruction, operator) {
        return Some(Arithmetic::Float(operation));
    }
    super::super::operations::NumberBitwise::from_js_instruction(instruction, operator)
        .map(Arithmetic::Int32)
}

/// Fused numeric mask truthiness. Other arithmetic/heap truthiness stays on its
/// existing path; both operands must satisfy the region's Number guards.
fn bitwise_condition(instruction: WideInstruction) -> Option<NumberBitwise> {
    (instruction.op().is_jump_binary_false()).then_some(())?;
    NumberBitwise::from_operator(instruction.binary_operator_field())
        .filter(|operation| *operation == NumberBitwise::And)
}

fn number_comparison(instruction: WideInstruction) -> Option<super::super::operations::NumberRelation> {
    (instruction.op().is_binary())
        .then(|| super::super::operations::NumberRelation::from_operator(instruction.binary_operator()))
        .flatten()
}


/// A unary operator a region lowers on a Number.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Unary {
    Number(NumberUnary),
    BitwiseNot,
}

fn unary(instruction: Op, operator: u32) -> Option<Unary> {
    if let Some(operation) = NumberUnary::from_js_instruction(instruction, operator) {
        return Some(Unary::Number(operation));
    }
    (instruction == Op::Unary && operator == UnaryOperator::BitwiseNot as u32)
        .then_some(Unary::BitwiseNot)
}

/// What each register of a region holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Number,
    Tagged,
    Array { written: bool },
    TaggedArray { written: bool },
}

struct Admission<'a> {
    constants: &'a [Value],
    roles: std::collections::BTreeMap<Register, Role>,
    captures: Vec<u16>,
    pub(crate) fields: Vec<FieldUse>,
    numbers: std::collections::BTreeMap<usize, f64>,
    values: std::collections::BTreeMap<usize, Value>,
    /// The arrays registers hold that hold a field's value (`held_registers`), each with
    /// whether the region stores into it.
    field_arrays: std::collections::BTreeMap<Register, bool>,
}

impl Admission<'_> {
    /// Notes `register` as an indexed array: one a field read defines, else an entry array.
    fn array(&mut self, register: Register, written: bool) -> Option<()> {
        match self.field_arrays.get_mut(&register) {
            Some(stored) => {
                *stored |= written;
                Some(())
            }
            None => self.note(register, Role::Array { written }),
        }
    }

    fn note(&mut self, register: Register, role: Role) -> Option<()> {
        match (self.roles.get(&register).copied(), role) {
            (None, role) => {
                self.roles.insert(register, role);
            }
            (Some(Role::Number), Role::Number) => {}
            (Some(Role::Tagged), Role::Tagged) => {}
            (Some(Role::Array { written }), Role::Array { written: more }) => {
                self.roles.insert(register, Role::Array { written: written || more });
            }
            (Some(Role::Array { written }), Role::Tagged)
            | (Some(Role::TaggedArray { written }), Role::Tagged) => {
                self.roles.insert(register, Role::TaggedArray { written });
            }
            (Some(Role::Tagged), Role::Array { written }) | (Some(Role::Tagged), Role::TaggedArray { written }) => {
                self.roles.insert(register, Role::TaggedArray { written });
            }
            (Some(Role::Array { written }), Role::TaggedArray { written: more })
            | (Some(Role::TaggedArray { written }), Role::Array { written: more })
            | (Some(Role::TaggedArray { written }), Role::TaggedArray { written: more }) => {
                self.roles.insert(register, Role::TaggedArray { written: written || more });
            }
            _ => return None,
        }
        Some(())
    }

    fn number(&mut self, register: Register) -> Option<()> {
        self.note(register, Role::Number)
    }

    fn tagged(&mut self, register: Register) -> Option<()> {
        self.note(register, Role::Tagged)
    }

    fn any_constant(&mut self, index: usize) -> Option<Value> {
        let value = *self.constants.get(index)?;
        self.values.insert(index, value);
        Some(value)
    }

    fn constant(&mut self, index: usize) -> Option<f64> {
        let number = self.any_constant(index)?.as_number()?;
        self.numbers.insert(index, number);
        Some(number)
    }

    /// Whether the operand names a Number the region reads: a register, noted as one, or a
    /// Number constant.
    fn source(&mut self, operand: Operand) -> Option<()> {
        match operand.kind()? {
            OperandKind::Register => self.number(operand.payload()),
            OperandKind::Constant => self.constant(usize::from(operand.payload())).map(|_| ()),
            OperandKind::Field | OperandKind::Local => None,
        }
    }

    /// The result register of an instruction whose result is only a register write.
    fn result(&mut self, instruction: WideInstruction) -> Option<Register> {
        (instruction.result_flags() == 0).then_some(())?;
        let register = instruction.result_register();
        self.number(register)?;
        Some(register)
    }
}

/// The region of the loop from `header` to its backward jump at `backedge`, when every
/// instruction in it is one the tier lowers.
pub(super) fn region(
    function: &Function,
    decoded: &[WideInstruction],
    constants: &[Value],
    sites: &[FieldSite],
    live: Option<&std::rc::Rc<crate::compile::liveness::RegisterMasks>>,
    header: u32,
    backedge: u32,
) -> Result<LoopRegion, RegionAnalysisFailure> {
    let (start, end) = (header as usize, backedge as usize);
    if start > end || end >= decoded.len() || end - start >= MAX_REGION_INSTRUCTIONS {
        return Err(RegionAnalysisFailure {
            reason: RegionRejection::RegionTooLarge,
            pc: backedge,
        });
    }
    let header_live = live.and_then(|live| live.get(start));
    admit_region(
        decoded[start..=end].to_vec(),
        usize::from(function.registers),
        constants,
        sites,
        live.cloned(),
        header_live,
        header,
        &std::collections::BTreeSet::new(),
    )
}

/// The region of `instructions`, at PCs from `header`, of a function with `registers`
/// registers whose liveness by PC is `live` (`header_live` at the header); `objects` are
/// registers read as objects beyond those the instructions show, so tagged
/// (`tagged_registers`) and holding a field's value where they can (`held_registers`).
#[allow(clippy::too_many_arguments)]
fn admit_region(
    instructions: Vec<WideInstruction>,
    registers: usize,
    constants: &[Value],
    sites: &[FieldSite],
    live: Option<std::rc::Rc<crate::compile::liveness::RegisterMasks>>,
    header_live: Option<RegisterSet>,
    header: u32,
    objects: &std::collections::BTreeSet<Register>,
) -> Result<LoopRegion, RegionAnalysisFailure> {
    if let Err(offset) = iteration::iterates_natively(&instructions, header) {
        return Err(RegionAnalysisFailure::at(RegionRejection::UnsupportedInstruction(instructions[offset].op()), header, offset));
    }
    match admit_register_roles(
        instructions.clone(), registers, constants, sites, live.clone(), header_live, header, objects,
    ) {
        Ok(region) => Ok(region),
        Err(failure) => {
            if objects.is_empty()
                && let Some(region) = admit_lifetime_roles(instructions, registers, constants, live, header)
            {
                return Ok(region);
            }
            Err(failure)
        }
    }
}

/// Whether a lifetime region admits `instruction`: number and element operations, jumps, a
/// field store through a register, and a field read by name from a register or `this`, whose
/// base renames as a register does. A site's lookup names its base in the site table, which
/// keeps VM slots.
fn lifetime_instruction(instruction: WideInstruction) -> bool {
    matches!(instruction.op(),
        Op::LoadConst | Op::Move | Op::Binary | Op::NumericAdd | Op::NumericMultiply
        | Op::IncDec | Op::ToNumeric | Op::Unary | Op::GetIndex | Op::SetIndex
        | Op::RequireObjectCoercible | Op::Jump | Op::JumpFalse | Op::JumpTrue
        | Op::JumpBinaryFalse)
        || instruction.field_store_register().is_some()
        || (instruction.op().is_get_field()
            && matches!(instruction.field_lookup(), Some(FieldLookup::Atom { base, .. })
                if base == FieldBase::THIS || base.register_index().is_some()))
}

/// Retry scalar/array admission on value identities, using the same checks and emitter.
/// This is analysis-only renaming: generated code and recovery still name VM slots.
fn admit_lifetime_roles(
    instructions: Vec<WideInstruction>,
    registers: usize,
    constants: &[Value],
    live: Option<std::rc::Rc<crate::compile::liveness::RegisterMasks>>,
    header: u32,
) -> Option<LoopRegion> {
    use std::collections::{BTreeMap, BTreeSet};
    if live.is_none() || instructions.is_empty() || registers > MAX_REGION_REGISTERS
        || !instructions.iter().all(|instruction| lifetime_instruction(*instruction))
    {
        return None;
    }
    let mut flow = LoopRegion {
        header, backedge: header + instructions.len() as u32 - 1,
        numbers: (0..registers).map(|register| register as Register).collect(),
        tagged: Vec::new(), arrays: Vec::new(), captures: Vec::new(),
        fields: Vec::new(), field_reads: BTreeMap::new(), dynamic_field_reads: BTreeMap::new(),
        dynamic_arrays: BTreeSet::new(),
        field_stores: BTreeMap::new(), dynamic_field_stores: BTreeMap::new(), field_store_sites: Vec::new(), held: BTreeMap::new(),
        identity_branches: BTreeMap::new(), instructions, constants: BTreeMap::new(),
        live, inlining: None,
    };
    let values = webs(&flow);
    // A temporary receiver may be copied from an entry array, but its backing view
    // must never be captured from the unrelated value left in that slot at entry.
    fn array_root(
        flow: &LoopRegion, values: &Webs, pc: u32, register: Register,
        visiting: &mut BTreeSet<(u32, Register)>,
    ) -> Option<Register> {
        if !visiting.insert((pc, register)) {
            return None;
        }
        let definitions = values.reaching[(pc - flow.header) as usize].get(&register)?;
        let mut root = None;
        for definition in definitions {
            let candidate = if let Some(entry) = flow.entry_register(*definition) {
                if flow.instructions.iter().any(|instruction| instruction.defined_register() == Some(entry)) {
                    return None;
                }
                entry
            } else {
                let offset = definition.checked_sub(flow.entry_count())?;
                let instruction = *flow.instructions.get(offset)?;
                if !instruction.op().is_move() || instruction.result_flags() != 0 {
                    return None;
                }
                array_root(flow, values, flow.header + offset as u32, instruction.register_b(), visiting)?
            };
            if root.is_some_and(|root| root != candidate) {
                return None;
            }
            root = Some(candidate);
        }
        visiting.remove(&(pc, register));
        root
    }
    let mut canonical = flow.instructions.clone();
    for (offset, instruction) in canonical.iter_mut().enumerate() {
        // Field receivers rename as values do; only array accesses read through an entry
        // array's root.
        if !instruction.op().is_indexed_access() {
            continue;
        }
        if let Some(receiver) = object_reads(*instruction) {
            let root = match array_root(&flow, &values, header + offset as u32, receiver, &mut BTreeSet::new()) {
                Some(root) => root,
                None if instruction.op().is_get_index()
                    && values.reaching[offset].get(&receiver).is_some_and(|definitions| {
                        !definitions.is_empty() && definitions.iter().all(|definition| {
                            definition.checked_sub(flow.entry_count())
                                .and_then(|index| flow.instructions.get(index))
                                .is_some_and(|definition| definition.op().is_get_index())
                        })
                    }) => continue,
                None => return None,
            };
            let base = if instruction.op().is_get_index() { Operand::register(root).0 } else { root };
            *instruction = WideInstruction::new(instruction.op(), instruction.a(), base, instruction.c(), instruction.imm());
        }
    }
    let roots: BTreeSet<Definition> = (0..values.parent.len()).map(|definition| values.find(definition)).collect();
    // Unused instruction definitions have no register and need no analysis identity.
    let roots: Vec<Definition> = roots.into_iter().filter(|definition| {
        *definition < flow.entry_count()
            || number_definition(flow.instructions[*definition - flow.entry_count()]).is_some()
    }).collect();
    if roots.len() > MAX_REGION_REGISTERS {
        return None;
    }
    let identities: BTreeMap<Definition, Register> = roots.iter().enumerate()
        .map(|(index, root)| (*root, index as Register)).collect();
    let original: Vec<Register> = roots.iter().map(|root| values.register(&flow, *root)).collect();
    let mut analysis = Vec::with_capacity(canonical.len());
    for (offset, instruction) in canonical.iter().copied().enumerate() {
        let pc = header + offset as u32;
        let read = |register| identities.get(&values.read(&flow, pc, register)).copied();
        let field = |field, raw| -> Option<u16> {
            Some(match instruction.op().field_layout(field) {
                FieldLayout::ResultRegister => {
                    if instruction.result_flags() != 0 { return None; }
                    *identities.get(&values.defined(&flow, pc))?
                }
                FieldLayout::Register => read(raw)?,
                FieldLayout::Operand | FieldLayout::NumericIndexOperand => {
                    let operand = Operand(raw);
                    match operand.kind()? {
                        OperandKind::Register => Operand::register(read(operand.payload())?).0,
                        OperandKind::Constant => raw,
                        _ => return None,
                    }
                }
                FieldLayout::FieldBase => {
                    let base = FieldBase(raw);
                    if base == FieldBase::THIS { raw } else { FieldBase::register(read(base.register_index()?)?).0 }
                }
                FieldLayout::WriteRegister | FieldLayout::ReadWriteRegister
                | FieldLayout::RegisterWindowBase => return None,
                _ => raw,
            })
        };
        analysis.push(WideInstruction::new(instruction.op(),
            field(InstructionField::A, instruction.a())?,
            field(InstructionField::B, instruction.b())?,
            field(InstructionField::C, instruction.c())?, instruction.imm()));
    }
    let mut region = admit_register_roles(analysis, roots.len(), constants, &[], None, None, header, &BTreeSet::new())
        .inspect_err(|&failure| {
            if crate::trace::Channel::OsrReject.on() {
                eprintln!("QUENCH_JS_JIT_OSR_LIFETIME_REJECT header={header} failure={failure:?}");
            }
        }).ok()?;
    let tagged: BTreeSet<Register> = region.tagged.iter().map(|register| original[usize::from(*register)]).collect();
    region.numbers = region.numbers.iter().map(|register| original[usize::from(*register)])
        .filter(|register| !tagged.contains(register)).collect::<BTreeSet<_>>().into_iter().collect();
    region.tagged = tagged.into_iter().collect();
    for array in &mut region.arrays { array.register = original[usize::from(array.register)]; }
    region.dynamic_arrays = region.dynamic_arrays.iter().map(|register| original[usize::from(*register)]).collect();
    for branch in region.identity_branches.values_mut() { branch.register = original[usize::from(branch.register)]; }
    // Field receivers name VM slots again: an entry receiver's identity is its slot at entry,
    // and a redefined one's reads go through the canonical instructions' registers.
    for field in &mut region.fields {
        if let FieldReceiver::Register(register) = &mut field.receiver {
            *register = original[usize::from(*register)];
        }
    }
    for store in &mut region.field_store_sites {
        if let FieldReceiver::Register(register) = &mut store.receiver { *register = original[usize::from(*register)]; }
    }
    region.held = std::mem::take(&mut region.held).into_iter()
        .map(|(register, (mut path, index))| {
            if let FieldReceiver::Register(root) = &mut path.root {
                *root = original[usize::from(*root)];
            }
            (original[usize::from(register)], (path, index))
        }).collect();
    region.instructions = canonical;
    region.live = flow.live.take();
    Some(region)
}

#[allow(clippy::too_many_arguments)]
fn admit_register_roles(
    instructions: Vec<WideInstruction>,
    registers: usize,
    constants: &[Value],
    sites: &[FieldSite],
    live: Option<std::rc::Rc<crate::compile::liveness::RegisterMasks>>,
    header_live: Option<RegisterSet>,
    header: u32,
    objects: &std::collections::BTreeSet<Register>,
) -> Result<LoopRegion, RegionAnalysisFailure> {
    let backedge = header + instructions.len() as u32 - 1;
    // A held register keeps the value its path read at entry, which a store to any name on
    // the path could change: a path through a name the region stores is not held.
    let stored_names: std::collections::BTreeSet<crate::bytecode::Atom> = instructions
        .iter()
        .filter(|instruction| admitted_store(instruction.op()))
        .map(|instruction| instruction.atom_index())
        .collect();
    let rooted: std::collections::BTreeMap<Register, FieldPath> = held_registers(&instructions, sites, header_live, objects)
        .into_iter()
        .filter(|(_, path)| path.steps.iter().all(|(atom, _)| !stored_names.contains(atom)))
        .collect();
    let paths: std::collections::BTreeMap<Register, FieldPath> = rooted.iter().filter(|(_, path)| !path.steps.is_empty()).map(|(register, path)| (*register, path.clone())).collect();
    // An indexed register the region defines, other than one holding a field's value at
    // every read (`paths`), holds no entry array: each access reads the array it holds then
    // (`LoopRegion::dynamic_arrays`). Its every definition reads an element, field, capture or
    // environment local, or moves one, whose value stays tagged; any other but an exit's is refused.
    let mut dynamic_arrays = std::collections::BTreeSet::new();
    for register in instructions
        .iter()
        .filter(|instruction| instruction.op().is_indexed_access())
        .filter_map(|instruction| object_reads(*instruction))
        .filter(|register| !paths.contains_key(register))
    {
        let mut definitions = instructions
            .iter()
            .enumerate()
            .filter(|(_, instruction)| native_definition(**instruction) == Some(register))
            .peekable();
        if definitions.peek().is_none() {
            continue;
        }
        if let Some((offset, _)) =
            definitions.find(|(_, instruction)| !matches!(instruction.op(), Op::GetIndex | Op::GetField | Op::Move | Op::LoadCapture | Op::LoadEnvLocal))
        {
            return Err(RegionAnalysisFailure::at(RegionRejection::ArrayRedefined, header, offset));
        }
        dynamic_arrays.insert(register);
    }
    let mut tagged = match tagged_registers_with_failure(
        &instructions,
        constants,
        &objects.iter().chain(paths.keys()).copied().collect(),
        &dynamic_arrays,
        header,
    ) {
        Ok(tagged) => tagged,
        Err(failure_offset) => {
            if crate::trace::Channel::OsrReject.on() {
                for (offset, instruction) in instructions.iter().copied().enumerate().filter(|(_, instruction)| {
                    instruction.op().is_jump_binary_false()
                }) {
                    let operand = |operand: Operand| {
                        let constant = (operand.kind() == Some(OperandKind::Constant))
                            .then(|| constants.get(usize::from(operand.payload())).copied());
                        (operand.kind(), operand.payload(), constant.flatten())
                    };
                    eprintln!(
                        "QUENCH_JS_JIT_OSR_TAGGED_REJECT header={header} offset={offset} binary_operator={} equality={:?} number_relation={:?} operand_b={:?} operand_c={:?}",
                        instruction.binary_operator_field(),
                        super::super::operations::equality_operator(instruction.binary_operator_field()),
                        super::super::operations::NumberRelation::from_operator(instruction.binary_operator_field()),
                        operand(instruction.operand_b()),
                        operand(instruction.operand_c()),
                    );
                }
            }
            return Err(RegionAnalysisFailure::at(
                RegionRejection::UnsupportedInstruction(instructions[failure_offset].op()),
                header,
                failure_offset,
            ));
        }
    };
    tagged.extend(paths.keys().copied());
    let mut admission = Admission {
        constants,
        roles: std::collections::BTreeMap::new(),
        captures: Vec::new(),
        fields: Vec::new(),
        numbers: std::collections::BTreeMap::new(),
        values: std::collections::BTreeMap::new(),
        field_arrays: std::collections::BTreeMap::new(),
    };
    let mut field_reads = std::collections::BTreeMap::new();
    let (mut dynamic_field_reads, mut field_stores) = (std::collections::BTreeMap::new(), std::collections::BTreeMap::new());
    // The names the region stores on a receiver it never rewrites (`this`, or an unchanged
    // register): a read of one through that receiver reads its current slot at the read.
    let written: std::collections::BTreeSet<Register> =
        instructions.iter().filter_map(|instruction| instruction.defined_register()).collect();
    let stored_fixed: std::collections::BTreeSet<(crate::bytecode::Atom, FieldReceiver)> = instructions
        .iter()
        .filter(|instruction| admitted_store(instruction.op()))
        .filter_map(|instruction| match instruction.field_store_register() {
            None => Some((instruction.atom_index(), FieldReceiver::This)),
            Some(receiver) if !written.contains(&receiver) => Some((instruction.atom_index(), FieldReceiver::Register(receiver))),
            Some(_) => None,
        })
        .collect();
    let mut dynamic_field_stores = std::collections::BTreeMap::new();
    let mut field_store_sites = Vec::new();
    let defined_registers: std::collections::BTreeSet<Register> = instructions
        .iter()
        .filter_map(|instruction| instruction.defined_register())
        .collect();
    for (offset, instruction) in instructions.iter().copied().enumerate() {
        if instruction.op().is_get_field() {
            let path = field_path(instruction, sites, &rooted).ok_or_else(|| {
                RegionAnalysisFailure::at(
                    RegionRejection::UnsupportedInstruction(Op::GetField),
                    header,
                    offset,
                )
            })?;
            let index = intern_field(&mut admission.fields, &path, tagged.contains(&instruction.result_register()));
            field_reads.insert(offset as u32, index);
            if path.steps.len() == 1
                && let FieldReceiver::Register(receiver) = path.root
                && defined_registers.contains(&receiver)
                // A field result used as an array base is entry-pinned separately by the
                // array lowering, so leave that combination on the conservative path.
                && !instructions.iter().any(|candidate| {
                    candidate.op().is_indexed_access()
                        && object_reads(*candidate) == Some(instruction.result_register())
                })
            {
                dynamic_field_reads.insert(offset as u32, index);
            } else if path.steps.len() == 1 && stored_fixed.contains(&(path.steps[0].0, path.root)) {
                dynamic_field_reads.insert(offset as u32, index);
                if path.root == FieldReceiver::This && !admission.captures.contains(&THIS_CAPTURE) {
                    admission.captures.push(THIS_CAPTURE);
                }
            }
        }
        if admitted_store(instruction.op()) {
            if instruction.result_flags() != 0 || instruction.returns_from_frame() {
                return Err(RegionAnalysisFailure::at(
                    RegionRejection::UnsupportedInstruction(instruction.op()),
                    header,
                    offset,
                ));
            }
            let receiver = instruction.field_store_register().map_or(FieldReceiver::This, FieldReceiver::Register);
            let atom = instruction.atom_index();
            let cache_site = instruction.cache_site_index();
            // A receiver the region redefines stores through the VM's cached store at the
            // instruction; every read of its name must then read the current slot too.
            if let FieldReceiver::Register(register) = receiver
                && defined_registers.contains(&register)
            {
                let current = |candidate: &WideInstruction| {
                    field_path(*candidate, sites, &rooted).is_some_and(|path| {
                        path.steps.len() == 1
                            && matches!(path.root, FieldReceiver::Register(register) if defined_registers.contains(&register))
                    })
                };
                if instructions.iter().any(|candidate| {
                    candidate.op().is_get_field()
                        && matches!(candidate.field_lookup(), Some(FieldLookup::Atom { atom: read, .. }) if read == atom)
                        && !current(candidate)
                }) {
                    return Err(RegionAnalysisFailure::at(RegionRejection::FieldMutation, header, offset));
                }
                dynamic_field_stores.insert(offset as u32, (atom, cache_site));
            } else {
                // The slot pointer is captured at entry, of a receiver the region never
                // rewrites. A read of the name through that receiver reads the current slot
                // (`stored_fixed`); any other read of it could alias and keep a stale value.
                if instructions.iter().any(|candidate| {
                    candidate.op().is_get_field()
                        && matches!(candidate.field_lookup(), Some(FieldLookup::Atom { atom: read, .. }) if read == atom)
                        && !field_path(*candidate, sites, &rooted)
                            .is_some_and(|path| path.steps.len() == 1 && path.root == receiver)
                }) {
                    return Err(RegionAnalysisFailure::at(RegionRejection::FieldMutation, header, offset));
                }
                let site = FieldStore { receiver, atom, cache_site };
                let index = field_store_sites.iter().position(|existing| *existing == site).unwrap_or_else(|| {
                    field_store_sites.push(site);
                    field_store_sites.len() - 1
                });
                field_stores.insert(offset as u32, index);
            }
        }
        // A field-resolved method call is owned by the existing inliner. Falling back to
        // the generic exact-PC gateway here would run its loop out of line and regress the
        // inliner's behavior and coverage. Dynamic/direct calls have no such owner.
        // An inlining region (one with call receivers, `objects`) leaves at a call it kept.
        if objects.is_empty() && instruction.op().is_ordinary_call() {
            let callee_register = instruction.register_b();
            if instructions[..offset]
                .iter()
                .rev()
                .find(|candidate| candidate.defined_register() == Some(callee_register))
                .is_some_and(|definition| definition.op().is_get_field())
            {
                return Err(RegionAnalysisFailure::at(
                    RegionRejection::UnsupportedInstruction(instruction.op()),
                    header,
                    offset,
                ));
            }
        }
        if let Some(register) = object_reads(instruction).filter(|register| paths.contains_key(register)) {
            admission.field_arrays.entry(register).or_insert(false);
        }
        if admit(&mut admission, instruction, constants, &tagged, &defined_registers, &dynamic_arrays).is_none() {
            if crate::trace::Channel::OsrReject.on() {
                let constant = (instruction.op().is_load_const())
                    .then(|| constants.get(instruction.constant_index()).copied());
                let result = instruction.defined_register();
                eprintln!(
                    "QUENCH_JS_JIT_OSR_REJECT header={header} offset={offset} op={:?} a={} b={} c={} imm={} constants_len={} result={result:?} tagged_result={} result_role={:?} constant={:?}",
                    instruction.op(),
                    instruction.a(),
                    instruction.b(),
                    instruction.c(),
                    instruction.imm(),
                    constants.len(),
                    result.is_some_and(|register| tagged.contains(&register)),
                    result.and_then(|register| admission.roles.get(&register)),
                    constant.flatten(),
                );
            }
            return Err(RegionAnalysisFailure::at(
                RegionRejection::UnsupportedInstruction(instruction.op()),
                header,
                offset,
            ));
        }
    }
    let held: std::collections::BTreeMap<Register, (FieldPath, usize)> = paths
        .into_iter()
        .map(|(register, path)| {
            let index = intern_field(&mut admission.fields, &path, true);
            (register, (path, index))
        })
        .collect();
    // A mutable field is one own-data Number cell. Every same-name read must
    // name that exact unchanged receiver; a different receiver may alias it.
    // Its stores must remain Numbers, so no write can invalidate its view or
    // require a reference barrier. Nested/tagged paths retain snapshot semantics.
    for (offset, index) in &field_stores {
        let store = &field_store_sites[*index];
        let reads = admission.fields.iter().filter(|field| field.atom == store.atom);
        for field in reads {
            if !store.matches(field) || field.tagged
                || admission.roles.get(&instructions[*offset as usize].register_a()) != Some(&Role::Number)
            {
                return Err(RegionAnalysisFailure::at(RegionRejection::FieldMutation, header, *offset as usize));
            }
        }
    }
    let mut identity_branches = std::collections::BTreeMap::new();
    for (offset, instruction) in instructions.iter().copied().enumerate() {
        let Some(branch) = identity_branch(instruction, constants) else {
            continue;
        };
        identity_branches.insert(header + offset as u32, branch);
    }
    if admission.fields.len() > MAX_REGION_FIELDS {
        return Err(RegionAnalysisFailure { reason: RegionRejection::FieldLimit, pc: backedge });
    }
    if field_store_sites.len() > MAX_REGION_FIELD_STORES {
        return Err(RegionAnalysisFailure { reason: RegionRejection::FieldStoreLimit, pc: backedge });
    }
    // Entry-snapshot reads require a stable receiver. Direct one-step reads can instead
    // probe the current receiver at each instruction; nested paths, held paths, and
    // field-backed arrays retain the entry-snapshot contract.
    if let Some((field, write_offset)) = admission.fields.iter().enumerate().find_map(|(index, field)| {
        let FieldReceiver::Register(register) = field.receiver else { return None };
        let write_offset = instructions
            .iter()
            .position(|instruction| instruction.defined_register() == Some(register))?;
        let uses: Vec<u32> = field_reads
            .iter()
            .filter_map(|(offset, used)| (*used == index).then_some(*offset))
            .collect();
        (!uses.is_empty() && uses.iter().any(|offset| !dynamic_field_reads.contains_key(offset)))
            .then_some((field, write_offset))
    }) {
        if crate::trace::Channel::OsrReject.on() {
            eprintln!(
                "QUENCH_JS_JIT_OSR_FIELD_REJECT header={header} reason=read_receiver_redefined atom={} receiver={:?} write_offset={write_offset}",
                field.atom,
                field.receiver,
            );
        }
        return Err(RegionAnalysisFailure::at(RegionRejection::FieldMutation, header, write_offset));
    }
    if admission.roles.keys().any(|register| usize::from(*register) >= registers)
        || admission.roles.len() > MAX_REGION_REGISTERS
    {
        return Err(RegionAnalysisFailure { reason: RegionRejection::RegisterLimit, pc: backedge });
    }
    if admission.captures.len() > MAX_REGION_CAPTURES {
        return Err(RegionAnalysisFailure { reason: RegionRejection::CaptureLimit, pc: backedge });
    }
    let numbers = admission
        .roles
        .iter()
        .filter(|(_, role)| **role == Role::Number)
        .map(|(register, _)| *register)
        .collect();
    let tagged: Vec<Register> = admission
        .roles
        .iter()
        .filter(|(_, role)| matches!(role, Role::Tagged | Role::TaggedArray { .. }))
        .map(|(register, _)| *register)
        .collect();
    let mut arrays: Vec<ArrayUse> = admission
        .roles
        .iter()
        .filter_map(|(register, role)| match role {
            Role::Array { written } | Role::TaggedArray { written }
                if !dynamic_arrays.contains(register)
                    && instructions.iter().any(|instruction| {
                    instruction.op().is_indexed_access()
                        && object_reads(*instruction) == Some(*register)
                }) => Some(ArrayUse { register: *register, written: *written, field: None }),
            Role::Number | Role::Tagged => None,
            Role::Array { .. } | Role::TaggedArray { .. } => None,
        })
        .collect();
    for (register, written) in &admission.field_arrays {
        // A held register read as an object only by field reads indexes nothing.
        if !instructions.iter().any(|instruction| {
            instruction.op().is_indexed_access()
                && object_reads(*instruction) == Some(*register)
        }) {
            continue;
        }
        arrays.push(ArrayUse { register: *register, written: *written, field: Some(held[register].1) });
    }
    if arrays.len() > MAX_REGION_ARRAYS {
        return Err(RegionAnalysisFailure { reason: RegionRejection::ArrayLimit, pc: backedge });
    }
    Ok(LoopRegion {
        header,
        backedge,
        numbers,
        tagged,
        arrays,
        captures: admission.captures,
        fields: admission.fields,
        field_reads,
        dynamic_field_reads,
        dynamic_arrays,
        field_stores,
        dynamic_field_stores,
        field_store_sites,
        held,
        identity_branches,
        instructions,
        constants: admission.numbers,

        live,
        inlining: None,
    })
}

fn admit(
    admission: &mut Admission<'_>,
    instruction: WideInstruction,
    constants: &[Value],
    tagged: &std::collections::BTreeSet<Register>,
    defined_registers: &std::collections::BTreeSet<Register>,
    dynamic_arrays: &std::collections::BTreeSet<Register>,
) -> Option<()> {
    match instruction.op() {
        Op::LoadConst => {
            let index = instruction.constant_index();
            if tagged.contains(&instruction.result_register()) {
                admission.any_constant(index)?;
                admission.tagged(instruction.result_register())?;
            } else {
                admission.constant(index)?;
                admission.result(instruction)?;
            }
        }
        Op::Move => {
            if tagged.contains(&instruction.result_register()) {
                admission.tagged(instruction.result_register())?;
                admission.tagged(instruction.register_b())?;
            } else {
                admission.result(instruction)?;
                admission.number(instruction.register_b())?;
            }
        }
        Op::LoadCapture => {
            (instruction.capture_depth() == 0 && instruction.result_flags() == 0).then_some(())?;
            if tagged.contains(&instruction.result_register()) {
                admission.tagged(instruction.result_register())?;
            } else {
                admission.result(instruction)?;
            }
            let slot = instruction.capture_slot();
            if !admission.captures.contains(&slot) {
                admission.captures.push(slot);
            }
        }
        Op::Binary | Op::NumericAdd | Op::NumericMultiply => {
            if number_comparison(instruction).is_some() {
                (instruction.result_flags() == 0).then_some(())?;
                admission.tagged(instruction.result_register())?;
            } else {
                arithmetic(instruction.op(), instruction.binary_operator())?;
                admission.result(instruction)?;
            }
            admission.source(instruction.operand_b())?;
            admission.source(instruction.operand_c())?;
        }
        Op::ToNumeric => {
            admission.result(instruction)?;
            admission.number(instruction.register_b())?;
        }
        Op::IncDec => {
            instruction.boolean_flag()?;
            admission.result(instruction)?;
            admission.number(instruction.register_b())?;
        }
        Op::Unary => {
            if instruction.unary_operator() == crate::bytecode::UNARY_TYPEOF_INDEX
                && tagged.contains(&instruction.result_register())
            {
                admission.tagged(instruction.result_register())?;
                admission.tagged(instruction.register_b())?;
            } else {
                unary(instruction.op(), instruction.unary_operator())?;
                admission.result(instruction)?;
                admission.number(instruction.register_b())?;
            }
        }
        // An array the entry guard proved is an object passes.
        Op::RequireObjectCoercible => {
            if !dynamic_arrays.contains(&instruction.register_b()) {
                admission.array(instruction.register_b(), false)?;
            }
        }
        Op::GetIndex => {
            if tagged.contains(&instruction.result_register()) {
                admission.tagged(instruction.result_register())?;
            } else {
                admission.result(instruction)?;
            }
            let base = instruction.operand_b().register_index()?;
            // A dynamic array takes no register role (`LoopRegion::dynamic_arrays`).
            if !dynamic_arrays.contains(&base) {
                admission.array(base, false)?;
            }
            admission.source(instruction.operand_c())?;
        }
        // Its field is the region's (`LoopRegion::field_reads`).
        Op::GetField => {
            if tagged.contains(&instruction.result_register()) {
                admission.tagged(instruction.result_register())?;
            } else {
                admission.result(instruction)?;
            }
        }
        Op::SetIndex => {
            admission.number(instruction.register_a())?;
            if !dynamic_arrays.contains(&instruction.register_b()) {
                admission.array(instruction.register_b(), true)?;
            }
            admission.number(instruction.register_c())?;
        }
        op if admitted_store(op) => {
            let value_register = instruction.register_a();
            // An unchanged entry value can be any tagged value. The VM logs a register
            // receiver in the generational barrier before native writes begin.
            if tagged.contains(&value_register) || !defined_registers.contains(&value_register) {
                admission.tagged(value_register)?;
            } else {
                admission.number(value_register)?;
            }
            if let Some(receiver) = instruction.field_store_register() {
                admission.tagged(receiver)?;
            }
        }
        Op::LoadEnvLocal => {
            if instruction.result_flags() != 0
                || instruction.returns_from_frame()
                || instruction.numeric_local_target().is_some()
            {
                return None;
            }
            if tagged.contains(&instruction.result_register()) {
                admission.tagged(instruction.result_register())?;
            } else {
                admission.result(instruction)?;
            }
        }
        // The interpreter runs every other instruction at its exact PC, with the whole state
        // written back (binding lookup, calls, allocation, GC, throws).
        op if exits_before_effects(op) => {}
        // A return leaves the loop: the lane resumes at it with the state written back, and
        // runs it.
        Op::Return | Op::Jump => {}
        Op::JumpBinaryFalse => {
            if let Some(branch) = identity_branch(instruction, constants) {
                admission.tagged(branch.register)?;
            } else if let Some((register, constant)) = match (
                instruction.operand_b().kind(),
                instruction.operand_c().kind(),
            ) {
                (Some(OperandKind::Register), Some(OperandKind::Constant)) => Some((
                    instruction.operand_b().payload(),
                    usize::from(instruction.operand_c().payload()),
                )),
                (Some(OperandKind::Constant), Some(OperandKind::Register)) => Some((
                    instruction.operand_c().payload(),
                    usize::from(instruction.operand_b().payload()),
                )),
                _ => None,
            }
            .filter(|(register, constant)| {
                super::super::operations::equality_operator(instruction.binary_operator_field())
                    .is_some_and(|(strict, _)| strict)
                    && tagged.contains(register)
                    && constants.get(*constant).and_then(|value| value.as_number()).is_some()
            })
            {
                admission.tagged(register)?;
                admission.constant(constant)?;
            } else if let (Some(left), Some(right)) =
                (instruction.operand_b().register_index(), instruction.operand_c().register_index())
                && super::super::operations::equality_operator(instruction.binary_operator_field()).is_some()
                && [left, right].iter().any(|register| {
                    tagged.contains(register) || matches!(admission.roles.get(register), Some(Role::Tagged))
                })
            {
                // An (in)equality of tagged values, which boxes a Number operand.
                for register in [left, right] {
                    if !matches!(admission.roles.get(&register), Some(Role::Number)) {
                        admission.tagged(register)?;
                    }
                }
            } else if super::super::operations::NumberRelation::from_operator(
                instruction.binary_operator_field(),
            )
            .is_some() || bitwise_condition(instruction).is_some()
            {
                admission.source(instruction.operand_b())?;
                admission.source(instruction.operand_c())?;
            } else {
                return None;
            }
        }
        Op::JumpFalse | Op::JumpTrue => {
            let register = instruction.register_a();
            // A tagged value's truthiness is decided natively where the value alone
            // decides it; any other value exits.
            if tagged.contains(&register) {
                admission.tagged(register)?;
            } else {
                admission.number(register)?;
            }
        }
        _ => return None,
    }
    Some(())
}

/// The values a compilation starts from: what the region's registers and captures held when
/// it compiled, and what earlier compilations found was no `i32` (`Widening`).
/// The observations a compilation starts from: the holderless own-data field reads
/// the native OSR reader may load directly, by region field index.
pub(crate) struct Observation<'a> {
    pub(crate) native_field_loads: &'a [Vec<NativeFieldLoad>],
}


/// A definition of a tracked register: its value at entry, by its index in
/// `LoopRegion::entry_registers`, or the instruction at a PC offset from the header.
type Definition = usize;

/// The region's values as webs: the definitions that reach a common read (an operand, or a
/// register written back where native code leaves) form one web, which native code holds in
/// one SSA variable with one representation. A register reused for unrelated values (an
/// index, then an element) gets a web for each, so one does not widen the other.
struct Webs {
    /// Instructions reachable from the header before an exact-PC VM exit.
    reachable: Vec<bool>,
    /// Union-find parents over definitions.
    parent: Vec<Definition>,
    /// The definitions reaching each instruction, by PC offset and register.
    reaching: Vec<std::collections::BTreeMap<Register, std::collections::BTreeSet<Definition>>>,
    /// Per leave PC and live register, a definition of the web written back there.
    leaves: std::collections::BTreeMap<(u32, Register), Definition>,
    /// Whether the value a register holds at entry is read (its web reaches a read).
    entry_read: Vec<bool>,
}

impl Webs {
    fn find(&self, mut definition: Definition) -> Definition {
        while self.parent[definition] != definition {
            definition = self.parent[definition];
        }
        definition
    }

    fn union(&mut self, left: Definition, right: Definition) {
        let (left, right) = (self.find(left), self.find(right));
        if left != right {
            self.parent[right] = left;
        }
    }

    /// The web `register` holds before the instruction at `pc`; a PC no path from the header
    /// reaches reads the entry value.
    fn read(&self, region: &LoopRegion, pc: u32, register: Register) -> Definition {
        let entry = || region.entry_registers().position(|number| number == register).unwrap_or(0);
        let definition = self.reaching[(pc - region.header) as usize]
            .get(&register)
            .and_then(|definitions| definitions.first().copied())
            .unwrap_or_else(entry);
        self.find(definition)
    }


    /// The register a web's definitions write: every definition reaching a common read
    /// writes the register read.
    fn register(&self, region: &LoopRegion, web: Definition) -> Register {
        match region.entry_register(web) {
            Some(register) => register,
            None => number_definition(region.instructions[web - region.entry_count()])
                .expect("a web's definitions write Number registers"),
        }
    }

    /// The web of the instruction at `pc`'s result.
    fn defined(&self, region: &LoopRegion, pc: u32) -> Definition {
        self.find(region.entry_count() + (pc - region.header) as usize)
    }
}

/// The Number registers an instruction reads.
pub(super) fn number_reads(instruction: WideInstruction) -> Vec<Register> {
    let register = |operand: Operand| operand.register_index();
    match instruction.op() {
        Op::Unary if instruction.unary_operator() == crate::bytecode::UNARY_TYPEOF_INDEX => Vec::new(),
        Op::Move | Op::IncDec | Op::Unary | Op::ToNumeric => vec![instruction.register_b()],
        Op::Binary | Op::NumericAdd | Op::NumericMultiply | Op::JumpBinaryFalse => [
            register(instruction.operand_b()),
            register(instruction.operand_c()),
        ]
        .into_iter()
        .flatten()
        .collect(),
        Op::JumpFalse | Op::JumpTrue => vec![instruction.register_a()],
        Op::GetIndex => register(instruction.operand_c()).into_iter().collect(),
        Op::SetIndex => vec![instruction.register_a(), instruction.register_c()],
        _ => Vec::new(),
    }
}

/// SSA operands and their semantic requirements. Entry-pinned heap views have
/// their own guard plan; other receivers read a guest register here.
fn value_reads(region: &LoopRegion, offset: usize) -> Vec<(Register, Type)> {
    let instruction = region.instructions[offset];
    let pc = region.header + offset as u32;
    let strict_number_test = instruction.op().is_jump_binary_false()
        && super::super::operations::equality_operator(instruction.binary_operator_field()).is_some_and(|(strict, _)| strict)
        && [instruction.operand_b(), instruction.operand_c()].iter().any(|operand|
            operand.kind() == Some(OperandKind::Constant) && region.constants.contains_key(&usize::from(operand.payload())));
    let expected = if matches!(instruction.op(), Op::Move | Op::JumpFalse | Op::JumpTrue)
        || region.identity_branches.contains_key(&pc) || strict_number_test {
        Type::ANY
    } else {
        Type::NUMBER
    };
    let mut reads: Vec<_> = number_reads(instruction).into_iter().map(|register| (register, expected)).collect();
    if instruction.op().is_unary() && instruction.unary_operator() == crate::bytecode::UNARY_TYPEOF_INDEX {
        reads.push((instruction.register_b(), Type::ANY));
    }
    if let Some(register) = object_reads(instruction) {
        let dynamic = match instruction.op() {
            Op::GetField => region.dynamic_field_reads.contains_key(&(offset as u32)),
            Op::GetIndex => region.dynamic_arrays.contains(&register)
                || !region.arrays.iter().any(|array| array.register == register),
            _ if instruction.field_store_register().is_some() => region.dynamic_field_stores.contains_key(&(offset as u32)),
            _ => region.dynamic_arrays.contains(&register),
        };
        if dynamic { reads.push((register, Type::ANY)); }
    }
    if admitted_store(instruction.op()) {
        let store = region.field_stores.get(&(offset as u32)).map(|index| &region.field_store_sites[*index]);
        let expected = store.map_or(Type::ANY, |store| region.field_store_type(store));
        reads.push((instruction.register_a(), expected));
    }
    reads
}

/// The value register an instruction defines, numeric or tagged.
fn number_definition(instruction: WideInstruction) -> Option<Register> {
    matches!(instruction.op().class(), Class::LoadConst | Class::Move | Class::LoadCapture | Class::Binary
        | Class::NumericAdd | Class::NumericMultiply | Class::IncDec | Class::Unary | Class::ToNumeric
        | Class::GetIndex | Class::GetField | Class::LoadEnvLocal)
    .then(|| instruction.result_register())
}

/// The instructions this tier lowers to native code (`admit`).
/// Every other one leaves at its exact PC before its effects and the interpreter runs its own
/// definition, so the tier covers every opcode; analysis and emission keep that boundary.
fn lowered_natively(op: Op) -> bool {
    admitted_store(op) || matches!(op.class(),
        Class::LoadConst | Class::Move | Class::LoadCapture | Class::Binary | Class::NumericAdd
        | Class::NumericMultiply | Class::ToNumeric | Class::IncDec | Class::Unary
        | Class::RequireObjectCoercible | Class::GetIndex | Class::GetField | Class::SetIndex
        | Class::LoadEnvLocal | Class::Return | Class::Jump | Class::JumpBinaryFalse
        | Class::JumpFalse | Class::JumpTrue)
}

fn exits_before_effects(op: Op) -> bool { !lowered_natively(op) }
/// The register a native run defines at an instruction: an exit's definition is the VM's.
fn native_definition(at: WideInstruction) -> Option<Register> { at.defined_register().filter(|_| lowered_natively(at.op())) }

/// The region PCs control continues at from the instruction at `pc`, and the PCs outside it
/// where the lane resumes; as lowered.
fn successors(region: &LoopRegion, pc: u32) -> Vec<u32> {
    let instruction = region.instructions[(pc - region.header) as usize];
    if exits_before_effects(instruction.op()) {
        return Vec::new();
    }
    let mut successors = Vec::with_capacity(2);
    crate::bytecode::for_each_instruction_successor(pc as usize, instruction, |successor| {
        successors.push(successor as u32);
    });
    successors
}

fn webs(region: &LoopRegion) -> Webs {
    use std::collections::{BTreeMap, BTreeSet};
    let count = region.instructions.len();
    let entries = region.entry_count();
    let inside = |pc: u32| (region.header..=region.backedge).contains(&pc);
    let mut reachable = vec![false; count];
    let mut pending = vec![region.header];
    while let Some(pc) = pending.pop() {
        let offset = (pc - region.header) as usize;
        if std::mem::replace(&mut reachable[offset], true) {
            continue;
        }
        pending.extend(successors(region, pc).into_iter().filter(|pc| inside(*pc)));
    }
    let mut reaching: Vec<BTreeMap<Register, BTreeSet<Definition>>> = vec![BTreeMap::new(); count];
    for (index, register) in region.entry_registers().enumerate() {
        reaching[0].entry(register).or_default().insert(index);
    }
    // Forward reaching definitions to a fixpoint; regions are small.
    loop {
        let mut changed = false;
        for offset in 0..count {
            if !reachable[offset] {
                continue;
            }
            let pc = region.header + offset as u32;
            let mut out = reaching[offset].clone();
            if let Some(register) = number_definition(region.instructions[offset]) {
                out.insert(register, BTreeSet::from([entries + offset]));
            }
            for successor in successors(region, pc).into_iter().filter(|pc| inside(*pc)) {
                let target = &mut reaching[(successor - region.header) as usize];
                for (register, definitions) in &out {
                    let set = target.entry(*register).or_default();
                    let before = set.len();
                    set.extend(definitions.iter().copied());
                    changed |= set.len() != before;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let mut webs = Webs {
        reachable,
        parent: (0..entries + count).collect(),
        reaching,
        leaves: BTreeMap::new(),
        entry_read: vec![false; entries],
    };
    let mut reads: Vec<Definition> = Vec::new();
    fn unify(
        webs: &mut Webs,
        definitions: &std::collections::BTreeSet<Definition>,
        reads: &mut Vec<Definition>,
    ) -> Option<Definition> {
        let mut definitions = definitions.iter().copied();
        let first = definitions.next()?;
        for other in definitions {
            webs.union(first, other);
        }
        reads.push(first);
        Some(first)
    }
    for offset in 0..count {
        if !webs.reachable[offset] {
            continue;
        }
        let pc = region.header + offset as u32;
        let instruction = region.instructions[offset];
        for (register, _) in value_reads(region, offset) {
            let definitions = webs.reaching[offset].get(&register).cloned().unwrap_or_default();
            unify(&mut webs, &definitions, &mut reads);
        }
        // Native code may leave at any instruction of the region, before its effects, and
        // resumes the lane where a jump leaves the region; each writes back what is live.
        let leave = |webs: &mut Webs, at: u32, after: bool, reads: &mut Vec<Definition>| {
            for register in region.entry_registers().filter(|register| region.is_live(at, *register)) {
                let mut definitions = if after && number_definition(instruction) == Some(register) {
                    BTreeSet::from([entries + offset])
                } else {
                    webs.reaching[offset].get(&register).cloned().unwrap_or_default()
                };
                if let Some(previous) = webs.leaves.get(&(at, register)) {
                    definitions.insert(*previous);
                }
                if let Some(first) = unify(webs, &definitions, reads) {
                    webs.leaves.insert((at, register), first);
                }
            }
        };
        leave(&mut webs, pc, false, &mut reads);
        for successor in successors(region, pc).into_iter().filter(|pc| !inside(*pc)) {
            // A fallthrough exit sees this instruction's completed definition;
            // an exit before its effects sees the incoming web instead.
            leave(&mut webs, successor, true, &mut reads);
        }
    }
    let read_webs: BTreeSet<Definition> = reads.iter().map(|definition| webs.find(*definition)).collect();
    webs.entry_read = (0..entries).map(|entry| read_webs.contains(&webs.find(entry))).collect();
    webs
}

