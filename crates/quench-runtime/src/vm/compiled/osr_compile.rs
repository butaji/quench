//! Loop-region code compiled beside the program: the loop runs in the lane while the code
//! generator's thread compiles, and the next look publishes the code.

use super::{
    compile_native_function, dump_native_code, names_length, native_dump_directory, lane_unit, osr_loop,
    publish_entry, tier_policy, CompiledOsrRegion, FieldReceiver, LaneHelpers, LoopRegion,
    NativeCompileRejection, NativeFieldLoad, Observation, VmFacts,
};
use crate::bytecode::{FieldLookup, Function, Op, ResidualProgram, WideInstruction};
use super::super::specializer::Specializer;
use std::collections::BTreeMap;
use std::rc::Rc;

/// What a region's code relies on beyond its entry guards: the exits that widen a value,
/// and the facts its own-field descriptors embed, which retire it when they change.
pub(in crate::vm) struct OsrFacts {
    /// Shape IDs the code embeds, the receiver's included. Shape compaction retires this code
    /// through its lane view before execution can resume with remapped IDs.
    pub(in crate::vm) shape_dependencies: Vec<u32>,
    /// Holder shapes: their layout is read without the receiver-shape guard, so their
    /// departure discards the compile.
    departure_shapes: Vec<u32>,
    slot_dependencies: Vec<crate::code_dependencies::SlotDependency>,
}

impl OsrFacts {

    /// Subscribes `code` to the facts; whether all hold. A receiver shape is guarded at each
    /// field use, so only a holder shape's departure discards it; a receiver shape ID still
    /// retires it at compaction (`shape_dependencies`).
    pub(in crate::vm) fn subscribe(&self, code: &Rc<dyn crate::code_dependencies::DependentCode>, heap: &mut crate::heap::Heap) -> bool {
        self.departure_shapes.iter().all(|shape| heap.subscribe_shape(*shape, code))
            && self.slot_dependencies.iter().all(|dependency| heap.subscribe_slot_observation(*dependency, code))
    }
}

/// A loop region whose code compiles on the code generator's thread, as V8 compiles OSR code
/// concurrently: compiling on the loop's own thread stalled it 2 ms a region (Box2D --jit:
/// 34 ms of a 733 ms run). Its facts are the observation's at the start, to which `watch`
/// subscribes: a shape compaction or a slot widening meanwhile discards the code.
pub(in crate::vm) struct PendingOsrRegion {
    code: super::super::jit_backend::Pending<Result<Vec<u8>, NativeCompileRejection>>,
    watch: Rc<crate::code_dependencies::PublishedCode<()>>,
    header: u32,
    clif: Option<String>,
    facts: OsrFacts,
}

/// What a background loop-region compile came to.
pub(in crate::vm) enum OsrCompile {
    Done(CompiledOsrRegion),
    /// A fact it took changed meanwhile: the next entry may compile from fresh facts.
    Stale,
    Failed,
}

/// The function's full decoded code as a plain lane unit: the loop coverage selects
/// `header..=backedge`, so every other PC is an exact-PC exit and the loop's PCs stay the
/// function's.
fn loop_unit(function: &Function) -> lane_unit::Unit {
    let instructions: Vec<_> = function.instructions().collect();
    let count = instructions.len();
    lane_unit::Unit {
        instructions,
        origins: (0..count).map(|pc| lane_unit::Origin::Caller(pc as u32)).collect(),
        frames: Vec::new(), markers: Vec::new(),
        caller_registers: function.registers, registers: function.registers,
        end: count as u32,
    }
}

/// The lane unit's frames for an inlining region's inlined callees.
fn unit_frames(region: &LoopRegion, inlining: &osr_loop::Inlining) -> Vec<lane_unit::Frame> {
    inlining.frames.iter().map(|frame| lane_unit::Frame {
        recovery: lane_unit::CallRecovery::Materialize,
        parent: None,
        call: frame.call,
        target: lane_unit::CallTarget {
            callee: lane_unit::CalleeBinding::Identity(frame.callee),
            function: frame.function,
        },
        callee_register: frame.callee_register,
        callee_entry_word: match frame.guard {
            osr_loop::InlineGuard::Field(index) => u32::try_from(osr_loop::field_word(region, index)).ok(),
            _ => None,
        },
        receiver: frame.receiver,
        window: frame.window,
        arguments: frame.arguments,
        result: Some(frame.result),
        base: frame.base,
        registers: frame.registers,
        alternative: None,
        function_call: None,
    }).collect()
}

/// Where each inlined frame's call marker goes: its first instruction's position and the PC
/// to resume at, where a failed identity guard must rerun the callee's read too.
fn marker_sites(origins: &[lane_unit::Origin], frames: &[lane_unit::Frame]) -> Vec<(usize, u32)> {
    (0..frames.len()).filter_map(|frame| {
        let at = origins.iter().position(|origin|
            matches!(origin, lane_unit::Origin::Inlined { frame: at, .. } if *at == frame))?;
        // A failed identity guard must rerun the callee's read too: the region resolved it
        // at compile time and dropped it, so resume where its setup begins.
        let call = match at.checked_sub(1).and_then(|before| origins.get(before)).copied() {
            Some(lane_unit::Origin::Caller(resume)) => resume,
            _ => frames[frame].call,
        };
        Some((at, call))
    }).collect()
}

/// The region's instructions with each jump target a region-local PC (one past the end where it leaves).
fn localized_jumps(region: &LoopRegion) -> Vec<WideInstruction> {
    let count = region.instructions().len();
    let header = region.header as usize;
    let mut instructions = region.instructions().to_vec();
    for instruction in &mut instructions {
        if instruction.op().immediate_role() == crate::bytecode::ImmediateRole::JumpTarget {
            let target = instruction.imm() as usize;
            let local = target.checked_sub(header).filter(|local| *local < count).unwrap_or(count);
            instruction.set_jump_target(local as u32);
        }
    }
    instructions
}

/// The region's static field reads, by the unit PC each instruction took, answered from
/// the entry record: an instruction at region offset `o` moved past every marker at or
/// before it (`inserted`).
fn inlined_entry_fields(region: &LoopRegion, program: &ResidualProgram, inserted: &[(usize, u32)]) -> BTreeMap<usize, u32> {
    let unit_pc = |offset: usize| offset + inserted.iter().filter(|(at, _)| *at <= offset).count();
    region.field_reads().iter()
        .filter(|(_, index)| entry_read(region, **index, program))
        .filter_map(|(offset, index)| Some((unit_pc(*offset as usize),
            u32::try_from(osr_loop::field_word(region, *index)).ok()?)))
        .collect()
}

/// Inserts each frame's call marker: a `Nop` where its body begins, at which the lane binds the
/// callee and passes the call's arguments, as `lane_unit::unit` emits it. The region's own
/// parameter moves precede the marker and write the same registers. Returns the insertions
/// (position, resume PC) and the markers' unit PCs.
fn insert_markers(instructions: &mut Vec<WideInstruction>, origins: &mut Vec<lane_unit::Origin>,
    frames: &[lane_unit::Frame]) -> (Vec<(usize, u32)>, Vec<usize>) {
    let mut inserted = marker_sites(origins, frames);
    inserted.sort_unstable_by_key(|(at, _)| *at);
    inserted.dedup_by_key(|(at, _)| *at);
    let mut markers: Vec<usize> = inserted.iter().map(|(at, _)| *at).collect();
    for &(at, call) in inserted.iter().rev() {
        instructions.insert(at, WideInstruction::new(Op::Nop, 0, 0, 0, 0));
        origins.insert(at, lane_unit::Origin::Caller(call));
    }
    // A jump target lands on the marker at its own position, as the call's PC did: an
    // insertion at the target itself is that marker, so only earlier ones move it.
    let shift = |target: usize| target + inserted.iter().filter(|(at, _)| *at < target).count();
    for instruction in instructions.iter_mut() {
        if instruction.op().immediate_role() == crate::bytecode::ImmediateRole::JumpTarget {
            instruction.set_jump_target(shift(instruction.imm() as usize) as u32);
        }
    }
    for marker in &mut markers { *marker = shift(*marker); }
    (inserted, markers)
}

/// Map an inlining loop region onto the lane unit: its instructions' jump targets become
/// unit PCs, its origins name the caller/inlined frame of each instruction, and its frames
/// carry the callee identity, function, call window and result.
fn inlining_unit(region: &LoopRegion, program: &ResidualProgram) -> Option<(lane_unit::Unit, BTreeMap<usize, u32>)> {
    let inlining = region.inlining.as_deref()?;
    let count = region.instructions().len();
    let mut instructions = localized_jumps(region);
    let mut origins: Vec<lane_unit::Origin> = inlining.origins.iter().take(count).map(|origin| match origin {
        osr_loop::Origin::Caller(pc) => lane_unit::Origin::Caller(*pc),
        osr_loop::Origin::Inlined { frame, pc } => lane_unit::Origin::Inlined { frame: usize::from(*frame), pc: *pc },
        osr_loop::Origin::Leave(pc) => lane_unit::Origin::Caller(*pc),
    }).collect();
    let frames = unit_frames(region, inlining);
    let (inserted, markers) = insert_markers(&mut instructions, &mut origins, &frames);
    let registers = frames.iter().map(|frame| frame.base + frame.registers)
        .max().unwrap_or(0).max(inlining.caller_registers);
    let entry_fields = inlined_entry_fields(region, program, &inserted);
    Some((lane_unit::Unit {
        instructions, origins, frames, markers,
        caller_registers: inlining.caller_registers, registers,
        // The caller PC past the loop, where an edge out of it resumes.
        end: inlining.latch + 1,
    }, entry_fields))
}

/// Whether the read of `region`'s field `index` answers from the entry record: a static read
/// of a field, not a register's `length`, which native code reads from the array's length
/// word at the read (`Lowering::array_length`) as the loop's stores may change it.
fn entry_read(region: &LoopRegion, index: usize, program: &ResidualProgram) -> bool {
    let field = &region.fields[index];
    !region.is_dynamic_field(index)
        && !(matches!(field.receiver, FieldReceiver::Register(_)) && names_length(program, field.atom))
}

/// A plain loop unit's static field reads, by the unit PC each instruction keeps, answered
/// from the entry record.
fn plain_entry_fields(region: &LoopRegion, program: &ResidualProgram) -> BTreeMap<usize, u32> {
    let header = region.header as usize;
    region.field_reads().iter()
        .filter(|(_, index)| entry_read(region, **index, program))
        .filter_map(|(offset, index)| Some((header + *offset as usize,
            u32::try_from(osr_loop::field_word(region, *index)).ok()?)))
        .collect()
}

/// The loop's cached field reads, keyed by their field-cache site: the lane specializer's
/// field-site map, from the observation's descriptors.
fn loop_fields(region: &LoopRegion, observation: &Observation<'_>)
    -> BTreeMap<u16, Vec<NativeFieldLoad>> {
    let mut fields = BTreeMap::new();
    for (&offset, &index) in region.field_reads() {
        let Some(FieldLookup::Atom { cache_site, .. }) =
            region.instructions()[offset as usize].field_lookup() else { continue };
        if let Some(loads) = observation.native_field_loads.get(index) {
            fields.insert(cache_site, loads.clone());
        }
    }
    fields
}

impl OsrFacts {
    /// The facts the observation's field descriptors embed in code compiled from it.
    fn observed(observation: &Observation<'_>) -> Self {
        let loads = || observation.native_field_loads.iter().flatten();
        let mut shape_dependencies: Vec<_> = loads().flat_map(|field| std::iter::once(field.shape).chain(field.holder)).collect();
        shape_dependencies.sort_unstable();
        shape_dependencies.dedup();
        let mut departure_shapes: Vec<_> = loads().filter_map(|field| field.holder).collect();
        departure_shapes.sort_unstable();
        departure_shapes.dedup();
        let mut slot_dependencies: Vec<_> = loads().filter_map(|field| field.slot_dependency()).collect();
        slot_dependencies.sort_unstable_by_key(|dependency| (dependency.shape, dependency.slot));
        slot_dependencies.dedup();
        Self { shape_dependencies, departure_shapes, slot_dependencies }
    }
}

/// The element reads through an array observed to hold int32s or Numbers (`number_arrays`), where
/// no earlier body left (`generic`) and none was `abandoned`: typed, each leaving at an element
/// that is not one. A lane body's observation is its entry activation's array parameters.
pub(super) fn observed_reads(unit: &lane_unit::Unit, number_arrays: &[(crate::bytecode::Register, crate::typed_ir::Type)],
    abandoned: &[usize], generic: &[usize]) -> Vec<super::facts::ObservedRead> {
    unit.instructions.iter().enumerate()
        .filter(|(pc, instruction)| instruction.op().is_get_index() && !abandoned.contains(pc))
        .filter_map(|(pc, instruction)| {
            let register = instruction.operand_b().register_index()?;
            let ty = number_arrays.iter().find(|(array, _)| *array == register)?.1;
            // An inlined callee's reads are Numbers whatever the entry showed: its registers
            // hold nothing at entry that the sample could have seen. So is a read that left
            // once: an int32 observation it outgrew widens to Numbers before it is dropped.
            let ty = if unit.frames.is_empty() && !generic.contains(&pc) { ty } else { crate::typed_ir::Type::NUMBER };
            Some(super::facts::ObservedRead { pc, ty })
        }).collect()
}

/// What lowers a loop's unit: the region specializer, the same plan a lane body and a static
/// typed region use. A region it declines stays in the interpreter; there is no second
/// lowering with its own facts, widenings and entry record.
struct LoopPlanner<'a> {
    observation: &'a Observation<'a>,
    fields: &'a BTreeMap<u16, Vec<NativeFieldLoad>>,
    isa: &'a dyn cranelift_codegen::isa::TargetIsa,
    program: &'a ResidualProgram,
    function_id: u32,
    helpers: LaneHelpers,
    views: super::ElementViews,
}

impl LoopPlanner<'_> {
    /// The function's own unit run over the loop is the general plan: a call inside it leaves
    /// at the call's exact PC, with no callee inlined. The inlined unit is an optimization over
    /// that plan, used only where the general plan declines; an inlined callee the region
    /// cannot prove exactly then costs the region nothing but the inlining.
    fn plan(&self, global_reads: &mut dyn FnMut(&lane_unit::Unit) -> crate::vm::global_facts::GlobalReads,
        unit: &lane_unit::Unit, entry_fields: &BTreeMap<usize, u32>, (header, backedge): (usize, usize),
        generic: &[usize]) -> Option<super::lane_region::LoweredRegion> {
        let number_reads = observed_reads(unit, self.observation.number_arrays, self.observation.abandoned, generic);
        super::lane_region::RegionSpecializer {
            number_reads: &number_reads, int_entries: self.observation.int_entries, typed_captures: self.observation.typed_captures, int_captures: self.observation.int_captures,
            function_id: self.function_id, unit, helpers: self.helpers, fields: self.fields,
            global_reads: &global_reads(unit), facts: VmFacts::STATIC, generic,
            direct: &BTreeMap::new(), frame_captures: &[], isa: self.isa,
            entry_fields,
            plan: super::lane_region::RegionPlan {
                coverage: super::lane_region::Coverage::Loop { header, backedge, poll: false },
                completion: super::lane_region::Completion::StatusAndPc, views: self.views,
            },
        }.specialize(self.program).ok().filter(|lowered| self.views == super::ElementViews::Dense || lowered.hoists_arrays)
    }
}

/// A lane region carries each inlined callee's full body and its guards, so a loop
/// that inlines two calls may legitimately exceed one function's code budget. The code
/// is published once per hot loop and retires with its site, so the larger per-site
/// bound stays bounded by the lane plan's own admission.
const REGION_CODE_BUDGET: usize = 2 * osr_loop::MAX_CODE_BYTES;

/// Starts compiling `region`'s code over its entry's element `views` from `observation`, or
/// `None` where it lowers no code (a typed plan hoisting no view would leave at an access).
pub(in crate::vm) fn start_osr_region(
    region: &LoopRegion, views: super::ElementViews,
    observation: &Observation<'_>,
    policy: crate::MemoryPolicy,
    program: &ResidualProgram,
    function_id: u32,
    helpers: LaneHelpers,
    global_reads: &mut dyn FnMut(&lane_unit::Unit) -> crate::vm::global_facts::GlobalReads,
    widened: &[usize],
) -> Option<PendingOsrRegion> {
    let isa = super::super::jit_backend::target_isa().filter(|_| tier_policy::by_policy(policy).loop_tier)?;
    let planner = LoopPlanner { observation, fields: &loop_fields(region, observation), isa: &*isa, program, function_id, helpers, views };
    let mut plan = |unit: &lane_unit::Unit, entry_fields: &BTreeMap<usize, u32>, header: usize, backedge: usize, generic: &[usize]|
        planner.plan(&mut *global_reads, unit, entry_fields, (header, backedge), generic);
    let inlined = region.inlining.is_some().then(|| inlining_unit(region, program)).flatten();
    let lane = inlined
        .as_ref()
        .and_then(|(unit, fields)| plan(unit, fields, 0, unit.instructions.len().saturating_sub(1), &[]))
        .or_else(|| {
            let plain = loop_unit(program.function(function_id as usize));
            // The plain unit's PCs are the function's, which the site's leaves name.
            plan(&plain, &plain_entry_fields(region, program), region.header as usize, region.backedge as usize, widened)
        });
    // One plan lowers a loop: the region specializer, the same plan a lane body and a static
    // typed region use. A region it declines stays in the interpreter; there is no second
    // lowering with its own facts, widenings and entry record.
    let function = lane?.function;
    let clif = native_dump_directory().map(|_| function.display().to_string());
    let facts = OsrFacts::observed(observation);
    let code = super::super::jit_backend::run_in_background(move || {
        compile_native_function(function, &*isa, REGION_CODE_BUDGET, |code| Some(code.to_vec()))
    });
    let watch = Rc::new(crate::code_dependencies::PublishedCode::new(()));
    Some(PendingOsrRegion { code, watch, header: region.header, clif, facts })
}

impl PendingOsrRegion {

    /// Subscribes its watch to the facts its code names; whether they hold.
    pub(in crate::vm) fn watch(&self, heap: &mut crate::heap::Heap) -> bool {
        let watch: Rc<dyn crate::code_dependencies::DependentCode> = self.watch.clone();
        self.facts.subscribe(&watch, heap)
    }

    /// Makes it stale where its code names shape ids, which a compaction renumbers.
    pub(in crate::vm) fn retire_named_shapes(&self) {
        if !self.facts.shape_dependencies.is_empty() { self.watch.retire(); }
    }

    /// What its compile came to, or itself while its code compiles.
    pub(in crate::vm) fn poll(self: Box<Self>) -> Result<OsrCompile, Box<Self>> {
        let code = match self.code.poll() {
            std::task::Poll::Pending => return Err(self),
            std::task::Poll::Ready(_) if !self.watch.is_published() => return Ok(OsrCompile::Stale),
            std::task::Poll::Ready(Some(Ok(code))) => code,
            std::task::Poll::Ready(_) => return Ok(OsrCompile::Failed),
        };
        let Self { header, clif, facts, .. } = *self;
        let Some((memory, entry)) = publish_entry(&code) else { return Ok(OsrCompile::Failed) };
        if let (Some(directory), Some(clif)) = (native_dump_directory(), clif) {
            dump_native_code(directory, header, &memory, &clif);
        }
        Ok(OsrCompile::Done(CompiledOsrRegion { _code: memory, entry, facts }))
    }
}


#[cfg(test)]
#[path = "osr_compile/tests.rs"]
mod tests;
