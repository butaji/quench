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
    FieldBase, FieldLayout, FieldLookup, FieldSite, Function, ImmediateLayout, ImmediateRole, InstructionField, Op, Operand,
    OperandKind, Register, WideInstruction,
};
use crate::compile::liveness::RegisterSet;
use crate::value::Value;
use crate::staged::{DecodeContext, Operation};
use super::super::operations::{NumberArithmetic, NumberBitwise, NumberUnary};

use super::typed_ir::Type;
use crate::typed_ir::Representation;
use super::NativeFieldLoad;
use oxc_ast::ast::UnaryOperator;

use crate::vm::compiled::tier_policy::{MAX_REGION_INSTRUCTIONS, MAX_REGION_REGISTERS, MAX_REGION_ARRAYS, MAX_REGION_CAPTURES, MAX_REGION_FIELDS, MAX_REGION_FIELD_STORES};

#[path = "osr_loop/inlining.rs"]
mod inlining;
#[path = "osr_loop/function.rs"]
mod function;
#[path = "osr_loop/iteration.rs"]
mod iteration;
#[path = "osr_loop/receivers.rs"]
mod receivers;
#[path = "osr_loop/lifetime.rs"]
mod lifetime;

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
    /// The depth-0 capture slots `LoadCapture` reads and the region does not store, in
    /// entry-record order.
    pub(crate) captures: Vec<u16>,
    /// The depth-0 capture slots the region both reads and stores: no entry snapshot holds
    /// them, so each read takes the lane's own lookup where it runs.
    pub(crate) stored_captures: Vec<u16>,
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
    /// The Number constant the region reads at pool index `index`.
    pub(super) fn number_constant(&self, index: usize) -> Option<f64> { self.constants.get(&index).copied() }
    /// The registers the region treats as Numbers.
    pub(super) fn number_registers(&self) -> &[Register] { &self.numbers }
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


    pub(crate) fn call_boundary_pcs(&self) -> Vec<u32> {
        self.instructions
            .iter()
            .enumerate()
            .filter(|(_, instruction)| crate::staged::calls_ordinary(instruction.op()))
            .map(|(offset, _)| self.header.saturating_add(offset as u32))
            .collect()
    }

}

/// Why a backward jump was excluded from OSR before native code generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RegionRejection {
    RegionTooLarge,
    UnsupportedInstruction(u16),
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

/// What a plain region does at a call whose callee a field read gives (a method call): leave
/// the loop to the inliner, which makes its region once the site is hot (a call boundary,
/// `RegionAnalysisFailure::is_call_boundary`), or leave at the call as at any other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MethodCalls {
    Inliner,
    Exit,
}

impl MethodCalls {
    /// Whether a region with call receivers `objects` (an inlining one, which leaves at a
    /// call it kept) leaves its method calls to the inliner.
    fn inlined(self, objects: &std::collections::BTreeSet<Register>) -> bool {
        self == Self::Inliner && objects.is_empty()
    }
}

impl RegionAnalysisFailure {
    /// The region stops only at a call, which may run inlined once the site is hot.
    pub(crate) fn is_call_boundary(self) -> bool {
        matches!(self.reason, RegionRejection::UnsupportedInstruction(opcode) if super::meaning_compiled::is_call_rejection(opcode))
    }

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
    (crate::staged::reads_named_field(instruction.op()) && instruction.result_flags() == 0).then_some(())?;
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
                && definitions(*register).all(|instruction| crate::staged::reads_named_field(instruction.op()))
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

/// Whether `instruction` is a comparison branch (`JumpBinaryFalse`), by its staged decode.
fn comparison_jump(instruction: WideInstruction) -> bool {
    // merge: kept as a helper because several admission arms test it; it now asks the staged query.
    crate::staged::compare_jump(instruction.op())
}

/// A nullish comparison branch that can be emitted from the tagged value word. Strict
/// identity compares one constant. Loose equality against null or undefined matches either
/// nullish value; Abstract Equality does not coerce an object when the other operand is null.
fn identity_branch(instruction: WideInstruction, constants: &[Value]) -> Option<IdentityBranch> {
    if !crate::staged::compare_jump(instruction.op()) {
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

/// The register and constant index of a strict (in)equality jump of a register and a constant that is
/// no Number (`=== "o"`): the value a string the region compares holds by its identity or its units,
/// which its tagged word carries (`admit`, `tagged_registers_with_failure`).
fn strict_constant_test(instruction: WideInstruction, constants: &[Value]) -> Option<(Register, usize)> {
    if !comparison_jump(instruction)
        || !super::super::operations::equality_operator(instruction.binary_operator_field()).is_some_and(|(strict, _)| strict)
    {
        return None;
    }
    let (register, index) = match (instruction.operand_b().kind()?, instruction.operand_c().kind()?) {
        (OperandKind::Register, OperandKind::Constant) => (instruction.operand_b().payload(), instruction.operand_c().payload()),
        (OperandKind::Constant, OperandKind::Register) => (instruction.operand_c().payload(), instruction.operand_b().payload()),
        _ => return None,
    };
    let index = usize::from(index);
    constants.get(index)?.as_number().is_none().then_some((register, index))
}

/// Whether `register` has definitions in `instructions` and every one is a `load`, which may
/// give an object, or a move of a register only such loads define (`const p = a[i]; p.x`: the
/// element read's register reaches the receiver through a `Move`) or of an entry value.
fn loads_alone_define(instructions: &[WideInstruction], register: Register, load: fn(WideInstruction) -> bool) -> bool {
    loads_define(instructions, register, load, &mut Vec::new())
}

/// `loads_alone_define`, through the moves not yet `followed`.
fn loads_define(instructions: &[WideInstruction], register: Register, load: fn(WideInstruction) -> bool, followed: &mut Vec<Register>) -> bool {
    if followed.contains(&register) {
        return false;
    }
    followed.push(register);
    let mut definitions = instructions.iter().filter(|candidate| candidate.defined_register() == Some(register)).peekable();
    definitions.peek().is_some() && definitions.all(|definition| load(*definition)
        || (matches!(crate::vm::compiled::decoded(*definition), Some(crate::staged::Operation::Move { .. })) && moved_from_load(instructions, definition.register_b(), load, followed)))
}

/// Whether a move's `source` may hold what a load gives: loads alone define it, or the loop
/// defines it not at all, so it holds its entry value (`let p = head; while (p) p = p.next`).
fn moved_from_load(instructions: &[WideInstruction], source: Register, load: fn(WideInstruction) -> bool, followed: &mut Vec<Register>) -> bool {
    !instructions.iter().any(|candidate| candidate.defined_register() == Some(source)) || loads_define(instructions, source, load, followed)
}

fn field_reads_alone_define(instructions: &[WideInstruction], register: Register) -> bool {
    loads_alone_define(instructions, register, is_field_read)
}

/// A named field read (`GetField`).
fn is_field_read(instruction: WideInstruction) -> bool {
    crate::staged::reads_named_field(instruction.op())
}

/// An element read (`GetIndex`).
// merge: HEAD also matched `ElementReadOperand`; theirs' staged query covers `GetIndex` only.
fn is_element_read(instruction: WideInstruction) -> bool {
    crate::staged::reads_element(instruction.op())
}

/// A field read or an element read: the loads whose result a field read may take as its receiver.
fn is_receiver_load(instruction: WideInstruction) -> bool {
    is_field_read(instruction) || is_element_read(instruction)
}

/// Traces a tagged-register rejection at `offset` (`QUENCH_TRACE=osr-reject`).
fn trace_tagged_reject(header: u32, offset: usize, result: Register, reason: &str) {
    if crate::trace::Channel::OsrReject.on() {
        eprintln!("QUENCH_JS_JIT_OSR_TAGGED_REJECT header={header} offset={offset} reason={reason} result={result}");
    }
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
        .filter(|instruction| !is_move_row(*instruction))
        .flat_map(number_reads)
        .collect();
    let mut tagged: std::collections::BTreeSet<Register> = instructions
        .iter()
        .filter_map(|instruction| identity_branch(*instruction, constants).map(|branch| branch.register))
        .chain(
            instructions
                .iter()
                .filter(|instruction| reads_type(**instruction) || instruction.op().allocates()
                    || number_comparison(**instruction).is_some())
                .map(|instruction| instruction.result_register()),
        )
        // An entry array's register; a dynamic array's (one the region defines) is tagged by
        // its values' webs instead (`representations`), as its register may hold Numbers too.
        .chain(instructions.iter().filter_map(|instruction| array_read(*instruction))
            .filter(|register| !dynamic_arrays.contains(register)))
        .chain(instructions.iter().enumerate().flat_map(|(offset, instruction)| call_window_tagged(instructions, &numeric_uses, offset, instruction)))
        // The register a strict test against a non-Number constant compares is tagged: its value may be a string.
        .chain(instructions.iter().filter_map(|instruction| strict_constant_test(*instruction, constants).map(|(register, _)| register)))
        .chain(instructions.iter().filter_map(|instruction| {
            if !crate::staged::loads_constant(instruction.op()) {
                return None;
            }
            let (destination, index) = (instruction.result_register(), instruction.constant_index());
            constants
                .get(index)?
                .as_number()
                .is_none()
                .then_some(destination)
        }))
        .chain(held.iter().copied())
        // A call's callee that only field or element reads define holds a function (a native one the
        // region calls in place); a register the loop also computes numbers into stays untagged.
        .chain(instructions.iter().filter(|instruction| crate::staged::calls_ordinary(instruction.op())).map(|instruction| instruction.register_b())
            .filter(|callee| loads_alone_define(instructions, *callee, is_receiver_load)))
        // A field receiver the region's field or element reads give holds an object.
        .chain(instructions.iter().filter(|instruction| crate::staged::reads_named_field(instruction.op())).filter_map(|instruction| {
            let receiver = object_reads(*instruction)?;
            loads_alone_define(instructions, receiver, is_receiver_load).then_some(receiver)
        }))
        // A branch on a value only field reads give tests its truthiness tagged:
        // a field may hold an object.
        .chain(instructions.iter().filter(|instruction| is_truthiness_row(**instruction)).filter_map(|instruction| {
            let value = instruction.register_a();
            field_reads_alone_define(instructions, value).then_some(value)
        }))
        // A store whose receiver the region redefines passes its receiver, and a value only
        // field reads give (which may be an object), tagged to the VM's cached store
        // (`LoopRegion::dynamic_field_stores`); a Number result is boxed at the store.
        .chain(instructions.iter().filter_map(|instruction| instruction.field_store_register().filter(|&receiver|
            instructions.iter().any(|candidate| candidate.defined_register() == Some(receiver)))))
        // Any store of a value only field reads give stores it tagged: it may be an object,
        // which the entry's barrier on the receiver covers (`loop_entry_in_lane`).
        .chain(instructions.iter().filter(|instruction| admitted_store(**instruction)).filter_map(|instruction| {
            let value = instruction.register_a();
            field_reads_alone_define(instructions, value).then_some(value)
        }))
        .collect();
    propagate_tagged(instructions, constants, header, &mut tagged)?;
    Ok(tagged)
}

/// Extends `tagged` along moves and type tests to the fixed point where every number definition
/// of a tagged register is one that may carry a boxed value; the offset of one that cannot.
fn propagate_tagged(instructions: &[WideInstruction], constants: &[Value], header: u32, tagged: &mut std::collections::BTreeSet<Register>) -> Result<(), usize> {
    let boxed = boxed(instructions);
    loop {
        let before = tagged.len();
        for (offset, instruction) in instructions.iter().copied().enumerate().rev() {
            tag_definition(instruction, offset, (constants, header, &boxed), tagged)?;
        }
        if tagged.len() == before {
            return Ok(());
        }
    }
}

/// What one number definition adds to `tagged`, or the offset where it cannot be admitted.
fn tag_definition(instruction: WideInstruction, offset: usize, (constants, header, boxed): (&[Value], u32, &std::collections::BTreeSet<Register>), tagged: &mut std::collections::BTreeSet<Register>) -> Result<(), usize> {
    let Some(result) = number_definition(instruction) else {
        return Ok(());
    };
    if is_move_row(instruction) {
        let source = instruction.register_b();
        if tagged.contains(&result) || tagged.contains(&source) {
            tagged.insert(result);
            tagged.insert(source);
        }
        return Ok(());
    }
    // A register an allocation also defines is held boxed throughout (`boxed`).
    if !tagged.contains(&result) || boxed.contains(&result) {
        return Ok(());
    }
    match tagged_operand(instruction, constants) {
        Ok(operand) => tagged.extend(operand),
        Err(reason) => {
            trace_tagged_reject(header, offset, result, &reason);
            return Err(offset);
        }
    }
    Ok(())
}

/// The register a tagged number definition `instruction` tags in turn (a type test's operand), or the
/// reason it cannot be admitted: only a definition that may carry a boxed value can be tagged.
fn tagged_operand(instruction: WideInstruction, constants: &[Value]) -> Result<Option<Register>, String> {
    let op = instruction.op();
    if crate::staged::loads_constant(op) {
        return constants.get(instruction.constant_index()).map(|_| None).ok_or_else(|| "missing_constant".to_owned());
    }
    if carries_boxed(instruction) {
        return Ok(None);
    }
    // A type test reads its operand's tag, and the VM's cell of a heap value
    // (the type-name index exits for one instead).
    if crate::staged::computes_unary(op) && reads_type(instruction) {
        return Ok(Some(instruction.register_b()));
    }
    Err(format!("unsupported_tagged_definition op={op:?}"))
}

/// Whether `instruction` defines a register that may carry an arbitrary boxed value: an entry
/// capture, a cached field, a global binding, an element, a fresh allocation or a comparison.
fn carries_boxed(instruction: WideInstruction) -> bool {
    let op = instruction.op();
    op.boxes_result() || op.allocates() || (crate::staged::computes_binary(op) && number_comparison(instruction).is_some())
}

/// The registers of an ordinary call at `offset` that stay boxed: its callee and the arguments
/// moved into its window from registers no number use reads. A call whose callee a field read
/// defined stays under the existing inliner's representation and ownership rules; dynamic and
/// direct calls leave at this exact PC, so their input window must remain boxed through
/// preceding Moves.
fn call_window_tagged(instructions: &[WideInstruction], numeric_uses: &std::collections::BTreeSet<Register>, offset: usize, instruction: &WideInstruction) -> Vec<Register> {
    if !crate::staged::calls_ordinary(instruction.op()) || callee_read_by_field(instructions, offset, instruction) {
        return Vec::new();
    }
    let base = ImmediateLayout::call_window_base(instruction.imm());
    let count = if crate::staged::calls_array(instruction.op()) {
        1
    } else {
        usize::from(ImmediateLayout::argument_count(instruction.imm()))
    };
    let mut registers = vec![instruction.register_b()];
    registers.extend((0..count)
        .filter_map(|offset| usize::from(base).checked_add(offset).and_then(|register| u16::try_from(register).ok()))
        .filter(|argument| moved_without_numeric_use(instructions, numeric_uses, offset, *argument)));
    registers
}

/// Whether the call at `offset` reads its callee from a register a field read defined.
fn callee_read_by_field(instructions: &[WideInstruction], offset: usize, call: &WideInstruction) -> bool {
    instructions[..offset]
        .iter()
        .rev()
        .find(|candidate| candidate.defined_register() == Some(call.register_b()))
        .is_some_and(|definition| crate::staged::reads_named_field(definition.op()))
}

/// Whether `argument` of the call at `offset` is the end of a chain of moves none of whose registers a
/// number use reads, or an alias of one a number use reads.
fn moved_without_numeric_use(instructions: &[WideInstruction], numeric_uses: &std::collections::BTreeSet<Register>, offset: usize, argument: Register) -> bool {
    let mut value = argument;
    let mut moved_argument = false;
    let mut moved_chain = std::collections::BTreeSet::new();
    loop {
        if !moved_chain.insert(value) {
            break;
        }
        if numeric_uses.contains(&value) {
            return false;
        }
        let numeric_move_alias = instructions.iter().any(|candidate| {
            is_move_row(*candidate)
                && candidate.register_b() == value
                && numeric_uses.contains(&candidate.result_register())
        });
        if numeric_move_alias {
            return false;
        }
        let Some(definition) = instructions[..offset]
            .iter()
            .rev()
            .find(|candidate| candidate.defined_register() == Some(value))
        else {
            break;
        };
        if !is_move_row(*definition) {
            break;
        }
        moved_argument = true;
        value = definition.register_b();
    }
    moved_argument
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
    /// A caller register the loop never writes, live at its header (`const f = …; for (…) f(…)`),
    /// checked by the VM at every native entry.
    Register(Register),
}

/// Where an inlined call's callee is read from: a cached field path, a depth-zero capture, or
/// a caller register the loop never writes (`inline_region`).
#[derive(Clone, Copy, Debug)]
pub(crate) enum CalleeSource {
    Path,
    Capture(u16),
    Register(Register),
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
    /// in the callee's own environment (`loop_entry_in_lane`).
    pub(crate) callee_captures: Vec<(usize, u16)>,
    /// The caller's register count: every register from it on is an inlined callee's.
    pub(crate) caller_registers: u16,
    /// The caller's backward jump PC: the last caller PC of the loop it closes.
    pub(crate) latch: u32,
}

impl Inlining {
    /// The registers past the caller's that the inlined frames occupy: the scratch window a leave
    /// inside one writes and reads (`LoopEntry::scratch_address`, `LoopEntry::scratch_value`).
    pub(crate) fn window(&self) -> usize {
        super::frames_window(self.frames.iter().map(|frame| (frame.base, frame.registers)), self.caller_registers)
    }
}

/// The capture slot bit an inlined callee's capture read carries (`Inlining::callee_captures`):
/// no function's own depth-0 slot reaches it, which `inline_region` checks.
pub(crate) const CALLEE_CAPTURE: u16 = 1 << 15;
/// The capture slot that carries the activation's `this` into a region whose field reads of
/// `this` read its current slots; no function's slot reaches it.
pub(crate) const THIS_CAPTURE: u16 = CALLEE_CAPTURE - 1;

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
    (crate::staged::reads_named_field(instruction.op()) && instruction.result_flags() == crate::bytecode::SET_THIS_REGISTER)
        .then_some(())?;
    // A site's base names a register of the callee in the site table, which renaming does not
    // reach: the read runs as one named read per step from the renamed base.
    let FieldLookup::Site(index) = instruction.field_lookup()? else { return None };
    let site = *sites.get(index)?;
    let result = instruction.result_register();
    let mut parts = vec![super::meaning_compiled::field_read_record(result, site.base.0, site.first.1, site.first.0)];
    if let Some((atom, cache_site)) = site.second {
        parts.push(super::meaning_compiled::field_read_record(result, FieldBase::register(result).0, cache_site, atom));
    }
    let (sink_atom, sink_site) = site.sink?;
    parts.push(super::meaning_compiled::this_field_store_record(result, 0, sink_site, sink_atom));
    Some(parts)
}

/// Whether an inlined callee's instruction computes its result and returns it in one
/// (`RETURN_REGISTER` alone, of a value-producing instruction), which inlining splits into
/// the computation and the `Return` it makes.
pub(super) fn inlined_returning(instruction: WideInstruction) -> bool {
    instruction.result_flags() == crate::bytecode::RETURN_REGISTER
        && !crate::staged::returns(instruction.op())
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
        _ if crate::staged::calls(op) => {
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
        opcode if crate::staged::reads_named_field(opcode) => match instruction.field_lookup()? {
            FieldLookup::Atom { base, .. } => base.register_index(),
            _ => None,
        },
        _ if let Some(array) = array_read(instruction) => Some(array),
        _ => instruction.field_store_register(),
    }
}

/// The register an indexed access, or the coercibility check before one, reads as its array.
fn array_read(instruction: WideInstruction) -> Option<Register> {
    match instruction.op() {
        opcode if crate::staged::reads_element(opcode) => instruction.operand_b().register_index(),
        opcode if crate::staged::indexes_receiver(opcode) => Some(instruction.register_b()),
        _ => None,
    }
}

/// Whether a loop region admits `instruction` as a named field store: every named store the
/// decoder makes (`SetField`, its `base` the receiver register or `None` for `this`) but a strict
/// store to `this`.
fn admitted_store(instruction: WideInstruction) -> bool {
    matches!(crate::vm::compiled::decoded_completed(instruction),
        Some(crate::staged::Operation::SetField { base, strict, .. }) if !(strict && base.is_none()))
}

/// Whether `instruction` names `register` (a read or a write); an instruction `renamed`
/// cannot rename names every register.
fn mentions(instruction: WideInstruction, register: Register, sites: &[FieldSite]) -> bool {
    // Only a GetField carries a field lookup immediate, so the op is checked first.
    if crate::staged::reads_named_field(instruction.op()) && matches!(instruction.field_lookup(), Some(FieldLookup::Site(_))) {
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

/// The registers of an inlining region that hold objects: each call's receiver and, where its callee's read is
/// checked, the callee's register, which holds the function its read gives.
fn object_registers(calls: &inlining::PcMap<inlining::Site<'_>>, checked_reads: &inlining::PcMap<u32>, caller: &[WideInstruction], header: u32) -> std::collections::BTreeSet<Register> {
    calls
        .iter()
        .flat_map(|(pc, site)| {
            let callee = checked_reads.contains(pc).then(|| caller[(pc - header) as usize].register_b());
            std::iter::once(site.frame.receiver).chain(callee)
        })
        .collect()
}

/// Whether the setup `run` before the call at `pc` can resume at `resume`: it only computes, no jump
/// lands inside it, and it defines no register live at the resume.
fn setup_resumes(run: &[WideInstruction], (resume, pc): (u32, u32), at_resume: RegisterSet, targets: &std::collections::BTreeSet<u32>) -> bool {
    run.iter().all(|instruction| pure_setup(instruction.op()))
        && (resume + 1..=pc).all(|pc| !targets.contains(&pc))
        && run.iter().all(|instruction| {
            instruction
                .defined_register()
                .is_none_or(|register| !crate::compile::liveness::register_in(at_resume, usize::from(register)))
        })
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
/// (`LaneExit::Inlined`).
#[allow(clippy::too_many_arguments)]
pub(super) fn inline_region<'a>(
    function: &Function,
    decoded: &[WideInstruction],
    constants: &[Value],
    sites: &[FieldSite],
    live: &crate::compile::liveness::RegisterMasks,
    header: u32,
    backedge: u32,
    resolve: &mut dyn FnMut(&[FieldUse], CalleeSource) -> Option<InlineCallee<'a>>,
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
    let mut calls: inlining::PcMap<inlining::Site<'a>> = inlining::PcMap::new(header, caller.len());
    // The caller PCs whose instructions drop out, the setup PCs by the PC they resume at, and
    // the caller instructions the region runs rewritten.
    let mut dropped = inlining::PcMap::new(header, caller.len());
    let mut setup = inlining::PcMap::new(header, caller.len());
    let mut rewritten = inlining::PcMap::new(header, caller.len());
    let caller_stored = inlining::stored_names(caller, sites);
    let mut checked_reads = inlining::PcMap::new(header, caller.len());
    let captured = receivers::captured(caller, mask(header)?);
    for (offset, instruction) in caller.iter().copied().enumerate() {
        if !crate::staged::calls(instruction.op()) {
            continue;
        }
        let pc = header + offset as u32;
        (instruction.result_flags() == 0 && !instruction.direct_eval() && !instruction.parameter_eval()).then_some(())?;
        let argc = ImmediateLayout::argument_count(instruction.imm());
        let window = ImmediateLayout::call_window_base(instruction.imm());
        let (function_register, mut receiver) = (instruction.register_b(), instruction.register_c());
        if !defines(function_register) {
            // A callee the loop never writes: the closure the register holds at entry, which the VM
            // checks at every native entry. Leaving in the call's setup, the straight run of pure
            // moves before it, resumes at the run's first instruction.
            (function_register < caller_registers
                && crate::compile::liveness::register_in(mask(header)?, usize::from(function_register))).then_some(())?;
            let start = (0..offset).rev().take_while(|index| pure_setup(caller[*index].op())).last().unwrap_or(offset);
            let resume = header + start as u32;
            let run = &caller[start..offset];
            setup_resumes(run, (resume, pc), mask(resume)?, &targets).then_some(())?;
            let (callee, reached) = inlining::admitted_callee(resolve, None, CalleeSource::Register(function_register), argc, sites)?;
            let bound = inlining::bound_parameters(run, resume, window, &callee, &reached, &defines, &mut dropped)?;
            for setup_pc in resume..pc {
                setup.insert(setup_pc, resume);
            }
            let base = Register::try_from(total).ok()?;
            total += u32::from(callee.registers);
            (total <= RegisterSet::BITS).then_some(())?;
            calls.insert(pc, inlining::Site {
                frame: InlineFrame {
                    call: pc, callee: callee.callee, function: callee.function, callee_register: function_register,
                    window, arguments: argc, result: instruction.result_register(),
                    guard: InlineGuard::Register(function_register), receiver, base, registers: callee.registers, bound,
                },
                callee_path: None, resume, window, result: instruction.result_register(), reached, callee,
            });
            continue;
        }
        // The callee's read: the last write of its register before the call, in a straight run.
        let read_offset = (0..offset).rev().find(|index| caller[*index].defined_register() == Some(function_register))?;
        let read = header + read_offset as u32;
        let definition = caller[read_offset];
        let capture = (is_load_capture_row(definition)
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
        let checked = defines(receiver) && receivers::stored(caller, read_offset, receiver, sites, &|atom| caller_stored.contains(atom as usize));
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
            rewritten.insert(header + own as u32, super::meaning_compiled::field_read_record(fresh, caller[own].b(), caller[own].c(), caller[own].imm()));
            callee_path.root = receiver_path.root;
            callee_path.steps.splice(0..0, receiver_path.steps);
            receiver = fresh;
            resume_offset = own;
        }
        let resume = header + resume_offset as u32;
        let at_resume = mask(resume)?;
        let run = &caller[resume_offset + 1..offset];
        setup_resumes(run, (resume, pc), at_resume, &targets).then_some(())?;
        let source = capture.map_or(CalleeSource::Path, CalleeSource::Capture);
        let (callee, reached) = inlining::admitted_callee(resolve, callee_path.as_ref(), source, argc, sites)?;
        let first = read + 1;
        let bound = inlining::bound_parameters(&caller[read_offset + 1..offset], first, window, &callee, &reached, &defines, &mut dropped)?;
        if !checked {
            dropped.insert(read, ());
        }
        for setup_pc in resume..pc {
            setup.insert(setup_pc, resume);
        }
        let base = Register::try_from(total).ok()?;
        total += u32::from(callee.registers);
        (total <= RegisterSet::BITS).then_some(())?;
        calls.insert(
            pc,
            inlining::Site {
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
    let layout = inlining::layout(caller, header, &dropped, &calls, sites);
    (layout.length as usize <= MAX_REGION_INSTRUCTIONS).then_some(())?;
    let mut emission = inlining::Emission::new(inlining::Plan {
        header,
        backedge,
        caller,
        sites,
        live,
        calls: &calls,
        dropped: &dropped,
        setup: &setup,
        rewritten: &rewritten,
        layout: &layout,
    });
    emission.run()?;
    let inlining::Emission { instructions, origins, masks, callee_captures, .. } = emission;
    let receivers = object_registers(&calls, &checked_reads, caller, header);
    let header_live = masks.first().copied();
    let mut region = admit_region(
        instructions,
        total as usize,
        constants,
        sites,
        Some(std::rc::Rc::new(crate::compile::liveness::RegisterMasks::new(masks))),
        header_live,
        header,
        &receivers, MethodCalls::Inliner, &|_| false,
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
    let frames = inlining::guarded_frames(calls, &checked_reads, &|register| held.get(&register), caller_registers, &defines, &mut region)?;
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

    /// The cache sites of the stores whose receiver the region redefines.
    pub(in crate::vm) fn dynamic_store_sites(&self) -> impl Iterator<Item = u16> + '_ {
        self.dynamic_field_stores.values().map(|&(_, site)| site)
    }

    pub(in crate::vm) fn entry_registers(&self) -> impl Iterator<Item = Register> + '_ {
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
    /// What the budget word was primed with for the current run (`set_backedge_budget`; `BACKEDGE_BUDGET` by default).
    primed: i64,
}

/// The longest array a region indexes: a negative `i32` index zero-extends past it, so one
/// unsigned comparison bounds every `i32` index.
pub(super) const MAX_INDEXED_LENGTH: usize = i32::MAX as usize;
/// The first record word counts executions of the region's own backward edge: the iterations
/// a run completed natively, which weigh what its entry paid for (`LoopSite::note_run`).
const ENTRY_BACKEDGE_COUNT: usize = 0;
/// The backward edges one native run may take before it leaves to the interpreter, which re-enters the loop (a run's
/// budget is the record's first word, primed here and debited by the stencils' `count_edge` and the lane's flush).
pub(crate) const BACKEDGE_BUDGET: i64 = 1 << 20;
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
const ENTRY_WORDS: usize =
    ENTRY_CAPTURES
        + MAX_REGION_CAPTURES
        + ARRAY_WORDS * MAX_REGION_ARRAYS
        + MAX_REGION_FIELDS
        + FIELD_STORE_WORDS * MAX_REGION_FIELD_STORES
        + NATIVE_HEAP_WORDS;

impl LoopEntry {
    pub(crate) fn new() -> Self {
        let mut words = [std::mem::MaybeUninit::uninit(); ENTRY_WORDS];
        // The backward-edge budget is the one leading word native code reads before any push.
        words[ENTRY_BACKEDGE_COUNT].write(BACKEDGE_BUDGET as u64);
        Self { words, length: ENTRY_CAPTURES, scratch: [std::mem::MaybeUninit::uninit(); MAX_REGION_REGISTERS], primed: BACKEDGE_BUDGET }
    }

    /// Number of taken executions of this region's backward edge in the most recent run: the budget the word was
    /// primed with less what it has left.
    pub(crate) fn backedge_count(&self) -> u64 {
        // SAFETY: `new` initializes the budget word.
        let left = unsafe { self.words[ENTRY_BACKEDGE_COUNT].assume_init() } as i64;
        (self.primed - left).max(0) as u64
    }

    /// Lets a b-jit region's run take at most `budget` backward edges (`LoopTier::backedge_budget`): the word is
    /// primed with it, and the stencil edge that debits it to zero leaves at the jump, so a loop due to move to u-jit
    /// leaves its stencil code mid-run. Never above `BACKEDGE_BUDGET`, the run's own bound.
    pub(crate) fn set_backedge_budget(&mut self, budget: u64) {
        self.primed = i64::try_from(budget).unwrap_or(BACKEDGE_BUDGET).clamp(1, BACKEDGE_BUDGET);
        self.words[ENTRY_BACKEDGE_COUNT].write(self.primed as u64);
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
    pub(crate) fn captures(&self, count: usize) -> impl Iterator<Item = Value> + '_ {
        debug_assert!(ENTRY_CAPTURES + count <= self.length);
        self.words[ENTRY_CAPTURES..ENTRY_CAPTURES + count]
            .iter()
            // SAFETY: the first `count` words after the leading ones are the pushed captures.
            .map(|word| Value::from_bits(unsafe { word.assume_init() }))
    }

    /// Arrays follow the fields, in the region's order. An array longer than
    /// `MAX_INDEXED_LENGTH` declines the entry.
    pub(crate) fn push_array<T>(&mut self, elements: *mut T, length: usize, capacity: usize) -> Option<()> {
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

    /// The scratch window an inlining region's leave inside an inlined callee writes the
    /// callee's live registers to, at each register's offset past the caller's
    /// (`JsStepDispatch::inlined`).
    /// The window's first `width` words start `undefined`, so a register the leave did not write
    /// reads as a value even if the callee's and the unit's liveness ever disagreed; only inlining
    /// regions ask for the window, and `scratch_value` reads no word past `Inlining::window`.
    pub(super) fn scratch_address(&mut self, width: usize) -> usize {
        let filled = width.min(self.scratch.len());
        self.scratch[..filled].fill(std::mem::MaybeUninit::new(Value::UNDEFINED.to_bits()));
        self.scratch.as_mut_ptr().addr()
    }

    /// The value an inlined callee's register `register` of an inlining region took when
    /// native code left at an inlined instruction: `undefined` where it was not live.
    ///
    /// # Safety
    /// The exit wrote the register back: it is live at the callee's PC the exit names.
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

fn arithmetic(instruction: WideInstruction, operator: u32) -> Option<Arithmetic> {
    if crate::staged::computes_binary(instruction.op()) && operator == super::super::operations::REMAINDER {
        return Some(Arithmetic::Remainder);
    }
    if let Some(operation) = NumberArithmetic::from_js_instruction(instruction.op(), operator) {
        return Some(Arithmetic::Float(operation));
    }
    super::super::operations::NumberBitwise::from_js_instruction(instruction.op(), operator)
        .map(Arithmetic::Int32)
}

/// Fused numeric mask truthiness. Other arithmetic/heap truthiness stays on its
/// existing path; both operands must satisfy the region's Number guards.
fn bitwise_condition(instruction: WideInstruction) -> Option<NumberBitwise> {
    (crate::staged::compare_jump(instruction.op())).then_some(())?;
    NumberBitwise::from_operator(instruction.binary_operator_field())
        .filter(|operation| *operation == NumberBitwise::And)
}

fn number_comparison(instruction: WideInstruction) -> Option<super::super::operations::NumberRelation> {
    (crate::staged::computes_binary(instruction.op()))
        .then(|| super::super::operations::NumberRelation::from_operator(instruction.binary_operator()))
        .flatten()
}


/// A unary operator a region lowers on a Number.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Unary {
    Number(NumberUnary),
    BitwiseNot,
}

fn unary(instruction: WideInstruction, operator: u32) -> Option<Unary> {
    if let Some(operation) = NumberUnary::from_js_instruction(instruction.op(), operator) {
        return Some(Unary::Number(operation));
    }
    (crate::staged::computes_unary(instruction.op()) && operator == UnaryOperator::BitwiseNot as u32)
        .then_some(Unary::BitwiseNot)
}

/// What each register of a region holds: a scalar in a representation, or an indexed array whose
/// elements are held in `elements`, and whether the region stores into it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Scalar(Representation),
    Array { elements: Representation, written: bool },
}

impl Role {
    /// The role that holds both `self` and `other`, or `None` when no single role does. Only
    /// Float64 and Tagged elements join; a Tagged scalar widens an array, and an array joins a
    /// Tagged scalar's register as a Tagged array.
    fn join(self, other: Role) -> Option<Role> {
        use Representation::{Float64, Tagged};
        let boxed = |elements: Representation| matches!(elements, Float64 | Tagged);
        match (self, other) {
            (Role::Scalar(Float64), Role::Scalar(Float64)) => Some(self),
            (Role::Scalar(Tagged), Role::Scalar(Tagged)) => Some(self),
            (Role::Array { elements: a, written }, Role::Array { elements: b, written: more })
                if boxed(a) && boxed(b) =>
            {
                Some(Role::Array { elements: a.max(b), written: written || more })
            }
            (Role::Array { elements, written }, Role::Scalar(Tagged)) if boxed(elements) => {
                Some(Role::Array { elements: Tagged, written })
            }
            (Role::Scalar(Tagged), Role::Array { written, .. }) => Some(Role::Array { elements: Tagged, written }),
            _ => None,
        }
    }
}

struct Admission<'a> {
    constants: &'a [Value],
    /// The registers an allocation defines (`boxed`).
    boxed: std::collections::BTreeSet<Register>,
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
            None => self.note(register, Role::Array { elements: Representation::Float64, written }),
        }
    }

    fn note(&mut self, register: Register, role: Role) -> Option<()> {
        let joined = match self.roles.get(&register).copied() {
            None => role,
            Some(old) => old.join(role)?,
        };
        self.roles.insert(register, joined);
        Some(())
    }

    /// A Number a register an allocation also defines holds boxed: the lane gives each of
    /// its values its own representation.
    fn number(&mut self, register: Register) -> Option<()> {
        if self.boxed.contains(&register) { return self.tagged(register); }
        self.note(register, Role::Scalar(Representation::Float64))
    }

    fn tagged(&mut self, register: Register) -> Option<()> {
        self.note(register, Role::Scalar(Representation::Tagged))
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
    method_calls: MethodCalls,
    native_call: &dyn Fn(usize) -> bool,
) -> Result<LoopRegion, RegionAnalysisFailure> {
    let (start, end) = (header as usize, backedge as usize);
    if start > end || end >= decoded.len() || end - start >= MAX_REGION_INSTRUCTIONS {
        return Err(RegionAnalysisFailure {
            reason: RegionRejection::RegionTooLarge,
            pc: backedge,
        });
    }
    let header_live = live.and_then(|live| live.get(start));
    let admitted = admit_region(
        decoded[start..=end].to_vec(),
        usize::from(function.registers),
        constants,
        sites,
        live.cloned(),
        header_live,
        header,
        &std::collections::BTreeSet::new(),
        method_calls,
        native_call,
    );
    // The BaseJIT tier proves each operand where its row reads it and needs no roles: its plan
    // lowers every instruction, so a loop the role analysis rejects is still its span.
    match admitted {
        Err(_) if crate::jit_mode::base_only() => LoopRegion::span(decoded[start..=end].to_vec(), constants, live, header).ok_or(RegionAnalysisFailure { reason: RegionRejection::RegionTooLarge, pc: backedge }),
        admitted => admitted,
    }
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
    method_calls: MethodCalls,
    native_call: &dyn Fn(usize) -> bool,
) -> Result<LoopRegion, RegionAnalysisFailure> {
    // Under `--b-jit` a loop whose every iteration leaves (at a call, a throw) is still a region:
    // BaseJIT lowers each instruction (`base_region::steps`), and Cranelift never compiles the loop.
    if !crate::jit_mode::base_only()
        && let Err(offset) = iteration::iterates_natively(&instructions, header, method_calls, native_call)
    {
        return Err(RegionAnalysisFailure::at(RegionRejection::UnsupportedInstruction(instructions[offset].op() as u16), header, offset));
    }
    match admit_register_roles(
        instructions.clone(), registers, constants, sites, live.clone(), header_live, header, objects, method_calls,
    ) {
        Ok(region) => Ok(region),
        // A loop the register analysis rejects is admitted on value identities where it can be; a
        // call the renamed loop stops at is the inliner's (`is_call_boundary`).
        Err(failure) => match lifetime::admit_lifetime_roles(instructions, registers, constants, sites, live, header, objects, method_calls) {
            Some(Ok(region)) => Ok(region),
            Some(Err(renamed)) if renamed.is_call_boundary() => Err(renamed),
            _ => Err(failure),
        },
    }
}

/// The region offsets a jump lands on.
fn block_starts(instructions: &[WideInstruction], header: u32) -> std::collections::BTreeSet<usize> {
    instructions.iter()
        .filter(|instruction| instruction.op().control_flow_layout().jumps())
        .map(|instruction| (instruction.jump_target() as usize).wrapping_sub(header as usize)).collect()
}

/// Whether an access of `register` is reached by a definition that makes it no array: `None`
/// where the region defines it nowhere, else the offset of such a definition, if any. Only the
/// definitions that reach an access matter: a register a later statement reuses for a number is
/// no array there. An access reached by a definition earlier in its own straight-line code is
/// reached by that one alone; any other access by every definition.
fn redefinition(instructions: &[WideInstruction], register: Register,
    block_starts: &std::collections::BTreeSet<usize>) -> Option<Option<usize>> {
    let definitions: Vec<usize> = instructions.iter().enumerate()
        .filter(|(_, instruction)| native_definition(**instruction) == Some(register)).map(|(offset, _)| offset).collect();
    if definitions.is_empty() { return None; }
    let accesses = instructions.iter().enumerate()
        .filter(|(_, instruction)| crate::staged::indexes_receiver(instruction.op()) && object_reads(**instruction) == Some(register))
        .map(|(offset, _)| offset);
    let mut reaching = std::collections::BTreeSet::new();
    for access in accesses {
        let local = (0..access).rev().find(|at| definitions.contains(at) || block_starts.contains(at))
            .filter(|at| definitions.contains(at));
        match local {
            Some(at) => { reaching.insert(at); }
            None => reaching.extend(definitions.iter().copied()),
        }
    }
    Some(reaching.into_iter().find(|offset| {
        let instruction = instructions[*offset];
        !instruction.op().carries_boxed_value()
    }))
}

/// The registers the region indexes that it defines (other than one holding a field's value at
/// every read, `paths`): they hold no entry array, each access reading the array it holds then
/// (`LoopRegion::dynamic_arrays`). Every definition reaching an access reads an element, field,
/// capture or environment local, or moves one, whose value stays tagged; any other but an exit's
/// is refused.
fn dynamic_arrays(instructions: &[WideInstruction], paths: &std::collections::BTreeMap<Register, FieldPath>,
    header: u32) -> Result<std::collections::BTreeSet<Register>, RegionAnalysisFailure> {
    let mut dynamic_arrays = std::collections::BTreeSet::new();
    let block_starts = block_starts(instructions, header);
    for register in instructions
        .iter()
        .filter(|instruction| crate::staged::indexes_receiver(instruction.op()))
        .filter_map(|instruction| object_reads(*instruction))
        .filter(|register| !paths.contains_key(register))
        .collect::<std::collections::BTreeSet<_>>()
    {
        match redefinition(instructions, register, &block_starts) {
            None => continue,
            Some(Some(offset)) => return Err(RegionAnalysisFailure::at(RegionRejection::ArrayRedefined, header, offset)),
            Some(None) => {}
        }
        dynamic_arrays.insert(register);
    }
    Ok(dynamic_arrays)
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
    method_calls: MethodCalls,
) -> Result<LoopRegion, RegionAnalysisFailure> {
    let backedge = header + instructions.len() as u32 - 1;
    // A held register keeps the value its path read at entry, which a store to any name on
    // the path could change: a path through a name the region stores is not held.
    let stored_names: std::collections::BTreeSet<crate::bytecode::Atom> = instructions
        .iter()
        .filter(|instruction| admitted_store(**instruction))
        .map(|instruction| instruction.atom_index())
        .collect();
    let rooted: std::collections::BTreeMap<Register, FieldPath> = held_registers(&instructions, sites, header_live, objects)
        .into_iter()
        .filter(|(_, path)| path.steps.iter().all(|(atom, _)| !stored_names.contains(atom)))
        .collect();
    let paths: std::collections::BTreeMap<Register, FieldPath> = rooted.iter().filter(|(_, path)| !path.steps.is_empty()).map(|(register, path)| (*register, path.clone())).collect();
    let dynamic_arrays = dynamic_arrays(&instructions, &paths, header)?;
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
                    crate::staged::compare_jump(instruction.op())
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
                RegionRejection::UnsupportedInstruction(instructions[failure_offset].op() as u16),
                header,
                failure_offset,
            ));
        }
    };
    tagged.extend(paths.keys().copied());
    let mut admission = Admission {
        constants,
        boxed: boxed(&instructions),
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
        .filter(|instruction| admitted_store(**instruction))
        .filter_map(|instruction| match instruction.field_store_register() {
            None => Some((instruction.atom_index(), FieldReceiver::This)),
            Some(receiver) if !written.contains(&receiver) => Some((instruction.atom_index(), FieldReceiver::Register(receiver))),
            Some(_) => None,
        })
        .collect();
    // A call the region continues past may write any field, so a read through a receiver the
    // region never rewrites reads its current slot, as one of the region's own stored names does.
    let calls = instructions.iter().any(|instruction| crate::staged::calls_ordinary(instruction.op()));
    let mut dynamic_field_stores = std::collections::BTreeMap::new();
    let mut field_store_sites = Vec::new();
    let defined_registers: std::collections::BTreeSet<Register> = instructions
        .iter()
        .filter_map(|instruction| instruction.defined_register())
        .collect();
    for (offset, instruction) in instructions.iter().copied().enumerate() {
        if crate::staged::reads_named_field(instruction.op()) {
            let path = field_path(instruction, sites, &rooted).ok_or_else(|| {
                RegionAnalysisFailure::at(
                    RegionRejection::UnsupportedInstruction(instruction.op() as u16),
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
                    crate::staged::indexes_receiver(candidate.op())
                        && object_reads(*candidate) == Some(instruction.result_register())
                })
            {
                dynamic_field_reads.insert(offset as u32, index);
            } else if path.steps.len() == 1 && (calls || stored_fixed.contains(&(path.steps[0].0, path.root))) {
                dynamic_field_reads.insert(offset as u32, index);
                if path.root == FieldReceiver::This && !admission.captures.contains(&THIS_CAPTURE) {
                    admission.captures.push(THIS_CAPTURE);
                }
            }
        }
        if admitted_store(instruction) {
            if instruction.result_flags() != 0 || instruction.returns_from_frame() {
                return Err(RegionAnalysisFailure::at(
                    RegionRejection::UnsupportedInstruction(instruction.op() as u16),
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
                    crate::staged::reads_named_field(candidate.op())
                        && matches!(candidate.field_lookup(), Some(FieldLookup::Atom { atom: read, .. }) if read == atom)
                        && !current(candidate)
                }) {
                    return Err(RegionAnalysisFailure::at(RegionRejection::FieldMutation, header, offset));
                }
                dynamic_field_stores.insert(offset as u32, (atom, cache_site));
            } else {
                // The slot pointer is captured at entry, of a receiver the region never
                // rewrites. A read of the name through that receiver reads the current slot
                // (`stored_fixed`), as does one through another receiver the region stores the
                // name on, which may be the same object; any other read of it could alias and
                // keep a stale value.
                if instructions.iter().any(|candidate| {
                    crate::staged::reads_named_field(candidate.op())
                        && matches!(candidate.field_lookup(), Some(FieldLookup::Atom { atom: read, .. }) if read == atom)
                        && !field_path(*candidate, sites, &rooted).is_some_and(|path| path.steps.len() == 1
                            && (path.root == receiver || stored_fixed.contains(&(atom, path.root))))
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
        // A field-resolved method call is the inliner's (`MethodCalls::inlined`), unless the
        // inliner declined it. Dynamic/direct calls have no such owner.
        if method_calls.inlined(objects) && crate::staged::calls_ordinary(instruction.op()) {
            let callee_register = instruction.register_b();
            if instructions[..offset]
                .iter()
                .rev()
                .find(|candidate| candidate.defined_register() == Some(callee_register))
                .is_some_and(|definition| crate::staged::reads_named_field(definition.op()))
            {
                return Err(RegionAnalysisFailure::at(
                    RegionRejection::UnsupportedInstruction(instruction.op() as u16),
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
                let constant = (crate::staged::loads_constant(instruction.op()))
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
                RegionRejection::UnsupportedInstruction(instruction.op() as u16),
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
        // A read through another receiver the name is stored on is that store's to check.
        let reads = admission.fields.iter().filter(|field| field.atom == store.atom
            && (field.receiver == store.receiver || !stored_fixed.contains(&(field.atom, field.receiver))));
        for field in reads {
            if !store.matches(field) || field.tagged
                || admission.roles.get(&instructions[*offset as usize].register_a()) != Some(&Role::Scalar(Representation::Float64))
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
        .filter(|(_, role)| **role == Role::Scalar(Representation::Float64))
        .map(|(register, _)| *register)
        .collect();
    let tagged: Vec<Register> = admission
        .roles
        .iter()
        .filter(|(_, role)| matches!(role, Role::Scalar(Representation::Tagged) | Role::Array { elements: Representation::Tagged, .. }))
        .map(|(register, _)| *register)
        .collect();
    let mut arrays: Vec<ArrayUse> = admission
        .roles
        .iter()
        .filter_map(|(register, role)| match role {
            Role::Array { written, .. }
                if !dynamic_arrays.contains(register)
                    && instructions.iter().any(|instruction| {
                    crate::staged::indexes_receiver(instruction.op())
                        && object_reads(*instruction) == Some(*register)
                }) => Some(ArrayUse { register: *register, written: *written, field: None }),
            Role::Scalar(_) | Role::Array { .. } => None,
        })
        .collect();
    for (register, written) in &admission.field_arrays {
        // A held register read as an object only by field reads indexes nothing.
        if !instructions.iter().any(|instruction| {
            crate::staged::indexes_receiver(instruction.op())
                && object_reads(*instruction) == Some(*register)
        }) {
            continue;
        }
        arrays.push(ArrayUse { register: *register, written: *written, field: Some(held[register].1) });
    }
    if arrays.len() > MAX_REGION_ARRAYS {
        return Err(RegionAnalysisFailure { reason: RegionRejection::ArrayLimit, pc: backedge });
    }
    let stored: std::collections::BTreeSet<u16> = instructions.iter()
        .filter(|instruction| is_store_capture_row(**instruction) && instruction.capture_depth() == 0)
        .map(|instruction| instruction.capture_slot())
        .collect();
    let (stored_captures, captures) = admission.captures.into_iter().partition(|slot| stored.contains(slot));
    Ok(LoopRegion {
        header,
        backedge,
        numbers,
        tagged,
        arrays,
        captures,
        stored_captures,
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

/// A loaded value's role: boxed where its register is tagged, else the number it defines.
fn admit_loaded(admission: &mut Admission<'_>, instruction: WideInstruction,
    tagged: &std::collections::BTreeSet<Register>) -> Option<()> {
    if tagged.contains(&instruction.result_register()) { admission.tagged(instruction.result_register()) }
    else { admission.result(instruction).map(drop) }
}

/// A stored value's role: boxed where its register is tagged or holds an unchanged entry value
/// (any tagged value), else a number.
fn admit_stored(admission: &mut Admission<'_>, register: Register, tagged: &std::collections::BTreeSet<Register>,
    defined_registers: &std::collections::BTreeSet<Register>) -> Option<()> {
    if tagged.contains(&register) || !defined_registers.contains(&register) { admission.tagged(register) }
    else { admission.number(register) }
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
        opcode if crate::staged::loads_constant(opcode) => {
            let index = instruction.constant_index();
            if tagged.contains(&instruction.result_register()) {
                admission.any_constant(index)?;
                admission.tagged(instruction.result_register())?;
            } else {
                admission.constant(index)?;
                admission.result(instruction)?;
            }
        }
        _ if is_move_row(instruction) => {
            if tagged.contains(&instruction.result_register()) {
                admission.tagged(instruction.result_register())?;
                admission.tagged(instruction.register_b())?;
            } else {
                admission.result(instruction)?;
                admission.number(instruction.register_b())?;
            }
        }
        opcode if crate::staged::reads_capture(opcode) => {
            (instruction.capture_depth() == 0 && instruction.result_flags() == 0).then_some(())?;
            admit_loaded(admission, instruction, tagged)?;
            let slot = instruction.capture_slot();
            if !admission.captures.contains(&slot) {
                admission.captures.push(slot);
            }
        }
        opcode if crate::staged::computes_arithmetic(opcode) => {
            if number_comparison(instruction).is_some() {
                (instruction.result_flags() == 0).then_some(())?;
                admission.tagged(instruction.result_register())?;
            } else {
                arithmetic(instruction, instruction.binary_operator())?;
                admission.result(instruction)?;
            }
            admission.source(instruction.operand_b())?;
            admission.source(instruction.operand_c())?;
        }
        opcode if crate::staged::converts_numeric(opcode) => {
            admission.result(instruction)?;
            admission.number(instruction.register_b())?;
        }
        opcode if crate::staged::increments(opcode) => {
            instruction.boolean_flag()?;
            admission.result(instruction)?;
            admission.number(instruction.register_b())?;
        }
        opcode if crate::staged::computes_unary(opcode) => {
            if reads_type(instruction) && tagged.contains(&instruction.result_register())
            {
                admission.tagged(instruction.result_register())?;
                admission.tagged(instruction.register_b())?;
            } else {
                unary(instruction, instruction.unary_operator())?;
                admission.result(instruction)?;
                admission.number(instruction.register_b())?;
            }
        }
        // An array the entry guard proved is an object passes.
        opcode if crate::staged::requires_object_coercible(opcode) => {
            if !dynamic_arrays.contains(&instruction.register_b()) {
                admission.array(instruction.register_b(), false)?;
            }
        }
        opcode if crate::staged::reads_element(opcode) => {
            admit_loaded(admission, instruction, tagged)?;
            let base = instruction.operand_b().register_index()?;
            // A dynamic array takes no register role (`LoopRegion::dynamic_arrays`).
            if !dynamic_arrays.contains(&base) {
                admission.array(base, false)?;
            }
            admission.source(instruction.operand_c())?;
        }
        // Its field is the region's (`LoopRegion::field_reads`).
        opcode if crate::staged::reads_named_field(opcode) => {
            admit_loaded(admission, instruction, tagged)?;
        }
        // A tagged value is stored as the lane's element store takes it. A value the loop does not define is an
        // unchanged entry value, which may be any tagged value (`admit_stored`), as a field store's is.
        opcode if crate::staged::writes_element(opcode) => {
            admit_stored(admission, instruction.register_a(), tagged, defined_registers)?;
            if !dynamic_arrays.contains(&instruction.register_b()) {
                admission.array(instruction.register_b(), true)?;
            }
            admission.number(instruction.register_c())?;
        }
        _ if admitted_store(instruction) => {
            let value_register = instruction.register_a();
            // An unchanged entry value can be any tagged value. The VM logs a register
            // receiver in the generational barrier before native writes begin.
            admit_stored(admission, value_register, tagged, defined_registers)?;
            if let Some(receiver) = instruction.field_store_register() {
                admission.tagged(receiver)?;
            }
        }
        // The lane stores through the binding's own store, and a binding it does not take
        // leaves; the entry snapshot holds no slot the region stores (`stored_captures`).
        _ if is_store_capture_row(instruction) => {
            let value = instruction.register_a();
            admit_stored(admission, value, tagged, defined_registers)?;
        }
        // A global binding's read (`Vm::global_reads_in_lane`), a Number where arithmetic reads it.
        opcode if crate::staged::reads_name(opcode) => {
            (instruction.result_flags() == 0).then_some(())?;
            admit_loaded(admission, instruction, tagged)?;
        }
        // A literal or construction allocates without collecting, a due collection leaving
        // first; its arguments and elements pass boxed.
        _ if instruction.op().allocates() => {
            (instruction.result_flags() == 0).then_some(())?;
            admission.tagged(instruction.result_register())?;
        }
        // Its literal's allocation writes it (`Allocation::ArrayOf`).
        opcode if crate::staged::defines_array_element(opcode) => {}
        opcode if crate::staged::reads_environment_slot(opcode) => {
            if instruction.result_flags() != 0
                || instruction.returns_from_frame()
                || instruction.numeric_local_target().is_some()
            {
                return None;
            }
            admit_loaded(admission, instruction, tagged)?;
        }
        // The interpreter runs every other instruction at its exact PC, with the whole state
        // written back (binding lookup, calls, allocation, GC, throws).
        _ if exits_before_effects(instruction) => {}
        // A return leaves the loop: the lane resumes at it with the state written back, and
        // runs it.
        opcode if crate::staged::returns(opcode) || crate::staged::jumps(opcode) => {}
        _ if comparison_jump(instruction) => {
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
            } else if let Some((register, constant)) = strict_constant_test(instruction, constants) {
                admission.tagged(register)?;
                admission.any_constant(constant)?;
            } else if let (Some(left), Some(right)) =
                (instruction.operand_b().register_index(), instruction.operand_c().register_index())
                && super::super::operations::equality_operator(instruction.binary_operator_field()).is_some()
                && [left, right].iter().any(|register| {
                    tagged.contains(register) || matches!(admission.roles.get(register), Some(Role::Scalar(Representation::Tagged)))
                })
            {
                // An (in)equality of tagged values, which boxes a Number operand.
                for register in [left, right] {
                    if !matches!(admission.roles.get(&register), Some(Role::Scalar(Representation::Float64))) {
                        admission.tagged(register)?;
                    }
                }
            } else {
                admission.source(instruction.operand_b())?;
                admission.source(instruction.operand_c())?;
            }
        }
        _ if is_truthiness_row(instruction) => {
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

/// The observations a compilation starts from: the holderless own-data field reads
/// the native OSR reader may load directly, by region field index.
pub(crate) struct Observation<'a> {
    pub(crate) native_field_loads: &'a [Vec<NativeFieldLoad>],
    /// The own entries of each store site whose receiver the region redefines
    /// (`LoopRegion::dynamic_field_stores`), by cache site: the shapes its in-place arms take.
    pub(crate) store_loads: &'a [(u16, Vec<NativeFieldLoad>)],
    /// The registers holding arrays whose sampled elements were all int32s, or all Numbers, at
    /// entry, with that type: reads through them have it, leaving where one does not.
    pub(crate) number_arrays: &'a [(Register, crate::typed_ir::Type)],
    /// The element reads that left again after their observation widened to Numbers: typed no more.
    pub(crate) abandoned: &'a [usize],
    /// The registers holding int32s at entry, whose loop may keep them as int32s where every
    /// definition in it yields one (`RegionSpecializer::int_entries`).
    pub(crate) int_entries: &'a [(Register, crate::typed_ir::IntRange)],
    /// The depth-zero capture slots holding typed arrays when compiled: a loop's array read
    /// through such a capture is viewed as a typed array (`LoopArray::views`).
    pub(crate) typed_captures: &'a [u16],
    /// The type each register held where the compile began, by register (`Type::INT32` for an int32,
    /// `Type::NUMBER` for another Number, `Type::ANY` otherwise); a register past the slice is `Type::ANY`.
    pub(crate) entry_types: &'a [Type],
    /// The registers holding strings where the compile began: a loop's element read through one
    /// takes the string row, with no hoisted view (`RegionSpecializer::string_entries`).
    pub(crate) string_entries: &'a [Register],
    /// What each instruction of the region, from its header, met where its int32 handler
    /// declined (`Vm::region_operand_kinds`): the feedback the representations start from,
    /// which the sample of the live registers above completes.
    pub(crate) operand_kinds: &'a [crate::vm::dispatch_fast::operand_feedback::OperandKinds],
    /// The depth-zero capture slots holding int32s when compiled: a read of such a capture
    /// takes the checked integer view where its uses are integer arithmetic.
    pub(crate) int_captures: &'a [u16],
    /// Whether loops may version their bounds on entry (`RegionPlan::entry_bounds`).
    pub(crate) entry_bounds: bool,
    /// Per inlined frame of the region (`Inlining::frames`), the dense capture slots of its
    /// callee's environment (`Vm::callee_capture_slots`): where the frame's own capture reads
    /// load, its identity guard fixing the environment.
    pub(crate) frame_captures: &'a [(usize, usize)],
    /// The construct plan each construction record holds now, by function and PC
    /// (`VmFacts::construct_plans`).
    pub(in crate::vm) construct_plans: Option<&'a dyn Fn(u32, u32) -> Option<super::ScalarConstructPlan>>,
}


/// A definition of a tracked register: its value at entry, by its index in
/// `LoopRegion::entry_registers`, or the instruction at a PC offset from the header.
type Definition = usize;

/// The region's values as webs: the definitions that reach a common read (an operand, or a
/// register written back where native code leaves) form one web, which native code holds in
/// one SSA variable with one representation. A register reused for unrelated values (an
/// index, then an element) gets a web for each, so one does not widen the other. Definitions
/// are bits of one state (`crate::webs`): the entry values first, then one per instruction.
struct Webs {
    /// The definitions reaching each instruction, by PC offset (`Reaching::reached`: those
    /// reachable from the header before an exact-PC VM exit).
    flow: crate::webs::Reaching,
    /// Per register, the definitions that write it, as a state's bits.
    written: std::collections::BTreeMap<Register, Vec<u64>>,
    /// Union-find over definitions.
    sets: crate::webs::DefSets,
    /// Per leave PC and live register, a definition of the web written back there.
    leaves: std::collections::BTreeMap<(u32, Register), Definition>,
    /// Whether the value a register holds at entry is read (its web reaches a read).
    entry_read: Vec<bool>,
}

impl Webs {
    fn find(&self, definition: Definition) -> Definition {
        self.sets.find(definition)
    }

    fn union(&mut self, left: Definition, right: Definition) {
        self.sets.union(left, right);
    }

    /// The definitions there are.
    fn definitions(&self) -> usize {
        self.sets.len()
    }

    /// Whether control from the header reaches the instruction at `offset`.
    fn reachable(&self, offset: usize) -> bool {
        self.flow.reached[offset]
    }

    /// The definitions of `register` reaching the instruction at `offset`, ascending.
    fn reaching(&self, offset: usize, register: Register) -> Vec<Definition> {
        let Some(written) = self.written.get(&register) else { return Vec::new() };
        let row = self.flow.row(offset);
        crate::webs::members(written).filter(|definition| crate::webs::has(row, *definition)).collect()
    }

    /// The web `register` holds before the instruction at `pc`; a PC no path from the header
    /// reaches reads the entry value.
    fn read(&self, region: &LoopRegion, pc: u32, register: Register) -> Definition {
        let entry = || region.entry_registers().position(|number| number == register).unwrap_or(0);
        let definition = self.reaching((pc - region.header) as usize, register).first().copied().unwrap_or_else(entry);
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
    if exits_before_effects(instruction) || reads_type(instruction) || !instruction.op().reads_values() {
        return Vec::new();
    }
    instruction.value_registers().collect()
}

/// SSA operands and their semantic requirements. Entry-pinned heap views have
/// their own guard plan; other receivers read a guest register here.
fn value_reads(region: &LoopRegion, offset: usize) -> Vec<(Register, Type)> {
    let instruction = region.instructions[offset];
    let pc = region.header + offset as u32;
    let strict_number_test = crate::staged::compare_jump(instruction.op())
        && super::super::operations::equality_operator(instruction.binary_operator_field()).is_some_and(|(strict, _)| strict)
        && [instruction.operand_b(), instruction.operand_c()].iter().any(|operand| operand.kind() == Some(OperandKind::Constant));
    let expected = if is_move_row(instruction) || is_truthiness_row(instruction)
        || region.identity_branches.contains_key(&pc) || strict_number_test {
        Type::ANY
    } else {
        Type::NUMBER
    };
    let mut reads: Vec<_> = number_reads(instruction).into_iter().map(|register| (register, expected)).collect();
    if reads_type(instruction) {
        reads.push((instruction.register_b(), Type::ANY));
    }
    if let Some(register) = object_reads(instruction) {
        let dynamic = match instruction.op() {
            opcode if crate::staged::reads_named_field(opcode) => region.dynamic_field_reads.contains_key(&(offset as u32)),
            opcode if crate::staged::reads_element(opcode) => region.dynamic_arrays.contains(&register)
                || !region.arrays.iter().any(|array| array.register == register),
            _ if instruction.field_store_register().is_some() => region.dynamic_field_stores.contains_key(&(offset as u32)),
            _ => region.dynamic_arrays.contains(&register),
        };
        if dynamic { reads.push((register, Type::ANY)); }
    }
    if admitted_store(instruction) {
        let store = region.field_stores.get(&(offset as u32)).map(|index| &region.field_store_sites[*index]);
        let expected = store.map_or(Type::ANY, |store| region.field_store_type(store));
        reads.push((instruction.register_a(), expected));
    }
    reads
}

/// Whether `instruction` is a register move, by its staged row (`Operation::Move`).
fn is_move_row(instruction: WideInstruction) -> bool {
    matches!(Operation::<(), ()>::decode(instruction, DecodeContext::EMPTY), Some(Operation::Move { .. }))
}

/// Whether `instruction` jumps on its register's truthiness, by its staged row (`Operation::JumpIf`).
fn is_truthiness_row(instruction: WideInstruction) -> bool {
    matches!(Operation::<(), ()>::decode(instruction, DecodeContext::EMPTY), Some(Operation::JumpIf { .. }))
}

/// Whether `instruction` stores to a capture, by its staged row (`Operation::StoreCapture`).
fn is_store_capture_row(instruction: WideInstruction) -> bool {
    matches!(Operation::<(), ()>::decode(instruction, DecodeContext::EMPTY), Some(Operation::StoreCapture { .. }))
}

/// Whether `instruction` reads a capture, by its staged row (`Operation::LoadCapture`, which the
/// row decodes only where the instruction has no result flags).
fn is_load_capture_row(instruction: WideInstruction) -> bool {
    matches!(Operation::<(), ()>::decode(instruction, DecodeContext::EMPTY), Some(Operation::LoadCapture { .. }))
}

/// The value register an instruction defines, numeric or tagged.
fn number_definition(instruction: WideInstruction) -> Option<Register> {
    instruction.op().defines_loop_result().then(|| instruction.result_register())
}

/// The instructions this tier lowers to native code (`admit`): those of the lane specializer's
/// (`lane_region::Lowering::instruction`) that a loop's entry facts support.
/// Every other one leaves at its exact PC before its effects and the interpreter runs its own
/// definition, so the tier covers every opcode; analysis and emission keep that boundary.
fn lowered_natively(instruction: WideInstruction) -> bool {
    let op = instruction.op();
    admitted_store(instruction) || if comparison_jump(instruction) {
        // A conditional jump only on a relation or a bitwise and, so one on `in` or
        // `instanceof` is an exit.
        super::super::operations::NumberRelation::from_operator(instruction.binary_operator_field())
            .is_some() || bitwise_condition(instruction).is_some()
    } else {
        op.lowers_in_loop()
    }
}

/// The registers an allocation of `instructions` defines, which also hold the Numbers a
/// register reuse writes to them.
fn boxed(instructions: &[WideInstruction]) -> std::collections::BTreeSet<Register> {
    instructions.iter().filter(|instruction| instruction.op().allocates()).map(|instruction| instruction.result_register()).collect()
}

/// A `typeof` of its operand: the type-name index or a fused test (`bytecode::typeof_test`).
fn reads_type(instruction: WideInstruction) -> bool {
    crate::staged::computes_unary(instruction.op()) && (instruction.unary_operator() == crate::bytecode::UNARY_TYPEOF_INDEX
        || crate::bytecode::typeof_test(instruction.unary_operator()).is_some())
}

fn exits_before_effects(instruction: WideInstruction) -> bool { !lowered_natively(instruction) }

/// The register a native run defines at an instruction: an exit's definition is the VM's.
fn native_definition(at: WideInstruction) -> Option<Register> { at.defined_register().filter(|_| lowered_natively(at)) }

/// The region PCs control continues at from the instruction at `pc`, and the PCs outside it
/// where the lane resumes; as lowered.
fn successors(region: &LoopRegion, pc: u32) -> Vec<u32> {
    let instruction = region.instructions[(pc - region.header) as usize];
    if exits_before_effects(instruction) {
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
    let bits = entries + count;
    let words = bits.div_ceil(crate::bitset::WORD_BITS);
    let inside = |pc: u32| (region.header..=region.backedge).contains(&pc);
    let mut written: BTreeMap<Register, Vec<u64>> = BTreeMap::new();
    let mut seed = vec![0; words];
    for (index, register) in region.entry_registers().enumerate() {
        crate::webs::set(written.entry(register).or_insert_with(|| vec![0; words]), index);
        crate::webs::set(&mut seed, index);
    }
    for (offset, instruction) in region.instructions.iter().enumerate() {
        if let Some(register) = number_definition(*instruction) {
            crate::webs::set(written.entry(register).or_insert_with(|| vec![0; words]), entries + offset);
        }
    }
    // An instruction's definition replaces every other of its register.
    let transfer = |offset: usize, out: &mut [u64]| if let Some(register) = number_definition(region.instructions[offset]) {
        for (word, kill) in out.iter_mut().zip(&written[&register]) { *word &= !kill; }
        crate::webs::set(out, entries + offset);
    };
    let next = |offset: usize, deliver: &mut dyn FnMut(crate::webs::Next)| for pc in successors(region, region.header + offset as u32) {
        deliver(if inside(pc) { crate::webs::Next::Row((pc - region.header) as usize) } else { crate::webs::Next::Leave(pc) });
    };
    let flow = crate::webs::solve(count, bits, &seed, &transfer, &next, false);
    let mut webs = Webs { flow, written, sets: crate::webs::DefSets::new(bits), leaves: BTreeMap::new(), entry_read: vec![false; entries] };
    let mut reads: Vec<Definition> = Vec::new();
    let unify = |webs: &mut Webs, definitions: Vec<Definition>, reads: &mut Vec<Definition>| -> Option<Definition> {
        let first = webs.sets.join_all(definitions)?;
        reads.push(first);
        Some(first)
    };
    for offset in 0..count {
        if !webs.reachable(offset) {
            continue;
        }
        let pc = region.header + offset as u32;
        let instruction = region.instructions[offset];
        for (register, _) in value_reads(region, offset) {
            let definitions = webs.reaching(offset, register);
            unify(&mut webs, definitions, &mut reads);
        }
        // Native code may leave at any instruction of the region, before its effects, and
        // resumes the lane where a jump leaves the region; each writes back what is live.
        let leave = |webs: &mut Webs, at: u32, after: bool, reads: &mut Vec<Definition>| {
            for register in region.entry_registers().filter(|register| region.is_live(at, *register)) {
                let mut definitions: BTreeSet<Definition> = if after && number_definition(instruction) == Some(register) {
                    BTreeSet::from([entries + offset])
                } else {
                    webs.reaching(offset, register).into_iter().collect()
                };
                if let Some(previous) = webs.leaves.get(&(at, register)) {
                    definitions.insert(*previous);
                }
                if let Some(first) = unify(webs, definitions.into_iter().collect(), reads) {
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

