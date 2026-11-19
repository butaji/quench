//! The phases of `inline_region` after it has chosen its calls: the callee's admission, the
//! region PCs every kept instruction takes, the instructions each takes there, and the
//! guards the frames end with.

use super::{
    admitted_store, CalleeSource, field_base, inlined_returning, inlined_set_this, intern_field, is_move_row, object_reads, renamed,
    FieldBase, FieldLookup, FieldPath, FieldReceiver, FieldSite, FieldUse, ImmediateLayout, ImmediateRole, InlineCallee,
    InlineFrame, InlineGuard, LoopRegion, Origin, Register, RegisterSet, WideInstruction, CALLEE_CAPTURE,
    MAX_INLINE_PARAMS, MAX_REGION_FIELDS,
};
use super::super::lane_unit::{inlinable, InlineScope};
mod pc_map;
pub(super) use pc_map::PcMap;
use crate::bitset::BitSet;
use crate::compile::liveness::{register_in, RegisterMasks};

/// One inlined call: its frame, its callee and where leaving in its setup resumes.
pub(super) struct Site<'b> {
    pub(super) frame: InlineFrame,
    pub(super) callee: InlineCallee<'b>,
    /// The path the callee's read reads it through, from a root the region does not
    /// write (a receiver given its own register reads through its path).
    pub(super) callee_path: Option<FieldPath>,
    /// Where leaving in the call's setup resumes: the receiver's own read, else the
    /// callee's.
    pub(super) resume: u32,
    pub(super) window: Register,
    pub(super) result: Register,
    pub(super) reached: BitSet,
}

/// The names the loop stores: a receiver read through one changes in the loop.
pub(super) fn stored_names(caller: &[WideInstruction], sites: &[FieldSite]) -> BitSet {
    let mut names = BitSet::new(0);
    caller
        .iter()
        .filter_map(|instruction| match instruction.op() {
            _ if admitted_store(*instruction) => Some(instruction.atom_index()),
            // A fused read the bytecode stores into a `this` field (`inlined_set_this`).
            op if crate::staged::reads_named_field(op) && instruction.writes_current_this() => match instruction.field_lookup()? {
                FieldLookup::Site(index) => sites.get(index)?.sink.map(|(atom, _)| atom),
                FieldLookup::Atom { .. } => None,
            },
            _ => None,
        })
        .for_each(|atom| {
            names.insert_growing(atom as usize);
        });
    names
}

/// The callee's instructions its entry reaches.
fn body<'s>(callee: &'s InlineCallee<'_>, reached: &'s BitSet) -> impl Iterator<Item = WideInstruction> + 's {
    callee.instructions.iter().copied().enumerate().filter(move |(pc, _)| reached.contains(*pc)).map(|(_, instruction)| instruction)
}

/// The callee as the read finds it now, with the instructions its entry reaches, when the
/// region can run it in place.
pub(super) fn admitted_callee<'a>(
    resolve: &mut dyn FnMut(&[FieldUse], CalleeSource) -> Option<InlineCallee<'a>>,
    path: Option<&FieldPath>,
    source: CalleeSource,
    argc: u16,
    sites: &[FieldSite],
) -> Option<(InlineCallee<'a>, BitSet)> {
    let mut chain = Vec::new();
    if let Some(path) = path {
        intern_field(&mut chain, path, true);
    }
    let callee = resolve(&chain, source)?;
    (callee.params == argc && usize::from(argc) <= MAX_INLINE_PARAMS).then_some(())?;
    let reached = crate::bytecode::reachable_from(&callee.instructions, [0]);
    body(&callee, &reached)
        .all(|instruction| {
            // The ops an inlined callee's instructions may be: those the loop admits, its `Return`,
            // and none that names its function's own state (captures, frame, calls).
            inlinable(instruction, InlineScope::Loop)
                && (instruction.result_flags() == 0 || inlined_returning(instruction) || inlined_set_this(instruction, sites).is_some())
        })
        .then_some(())?;
    // A captured or register-held callee's call may bind undefined to the global receiver. Keep
    // this slice receiver-independent; recovery still performs the ordinary binding below.
    if !matches!(source, CalleeSource::Path) {
        body(&callee, &reached)
            .all(|instruction| !crate::staged::reads_named_field(instruction.op()) || field_base(instruction, sites) != Some(FieldBase::THIS))
            .then_some(())?;
    }
    Some((callee, reached))
}

/// A parameter read as an object is bound to the caller register its argument was
/// moved from in the setup `run` (starting at PC `first`), which the region never writes;
/// any other is a Number it copies. Each such move drops out.
pub(super) fn bound_parameters(
    run: &[WideInstruction],
    first: u32,
    window: Register,
    callee: &InlineCallee<'_>,
    reached: &BitSet,
    defines: &dyn Fn(Register) -> bool,
    dropped: &mut PcMap<()>,
) -> Option<[Option<Register>; MAX_INLINE_PARAMS]> {
    let mut bound = [None; MAX_INLINE_PARAMS];
    for parameter in 0..callee.params {
        if !body(callee, reached).any(|instruction| object_reads(instruction) == Some(parameter)) {
            continue;
        }
        body(callee, reached).all(|instruction| instruction.defined_register() != Some(parameter)).then_some(())?;
        let argument = window + parameter;
        let moved = run.iter().rposition(|instruction| instruction.defined_register() == Some(argument))?;
        (is_move_row(run[moved]) && !defines(run[moved].register_b())).then_some(())?;
        bound[usize::from(parameter)] = Some(run[moved].register_b());
        dropped.insert(first + moved as u32, ());
    }
    Some(bound)
}

/// Region PCs: each kept caller instruction takes one, a call its numeric parameters'
/// moves and its callee's reached instructions, each `Return` two (the result's move and
/// the jump past the call). A dropped instruction's PC is the next instruction's.
pub(super) struct Layout {
    pub(super) caller: Vec<u32>,
    pub(super) callee: PcMap<Vec<u32>>,
    pub(super) length: u32,
}

fn set_this_length(instruction: WideInstruction, sites: &[FieldSite]) -> u32 {
    inlined_set_this(instruction, sites).map_or(1, |parts| parts.len() as u32)
}

/// The region instructions that complete a call (`Emission::complete`): the move of the result and the jump past the call.
const COMPLETION_LENGTH: u32 = 2;

/// How many region instructions a reached callee instruction becomes.
fn callee_length(instruction: WideInstruction, sites: &[FieldSite]) -> u32 {
    if crate::staged::returns(instruction.op()) {
        COMPLETION_LENGTH
    } else if inlined_returning(instruction) {
        // The computation, then the completion of the call.
        1 + COMPLETION_LENGTH
    } else {
        set_this_length(instruction, sites)
    }
}

impl Layout {
    /// The region PC of each of the callee's instructions, advancing past its parameters'
    /// moves and its reached instructions.
    fn flatten(&mut self, site: &Site<'_>, header: u32, sites: &[FieldSite]) -> Vec<u32> {
        self.length += site.frame.bound[..usize::from(site.callee.params)].iter().filter(|bound| bound.is_none()).count() as u32;
        let mut flat = Vec::with_capacity(site.callee.instructions.len());
        for (index, instruction) in site.callee.instructions.iter().enumerate() {
            flat.push(header + self.length);
            if site.reached.contains(index) {
                self.length += callee_length(*instruction, sites);
            }
        }
        flat
    }
}

pub(super) fn layout(
    caller: &[WideInstruction],
    header: u32,
    dropped: &PcMap<()>,
    calls: &PcMap<Site<'_>>,
    sites: &[FieldSite],
) -> Layout {
    let mut layout = Layout { caller: Vec::with_capacity(caller.len()), callee: PcMap::new(header, caller.len()), length: 0 };
    for (offset, instruction) in caller.iter().enumerate() {
        let pc = header + offset as u32;
        layout.caller.push(header + layout.length);
        if dropped.contains(pc) {
            continue;
        }
        match calls.get(pc) {
            // A fused field read the caller stores into its `this` field runs as its parts,
            // each leaving at the caller's PC, which reruns the pure read.
            None => layout.length += set_this_length(*instruction, sites),
            Some(site) => {
                let flat = layout.flatten(site, header, sites);
                layout.callee.insert(pc, flat);
            }
        }
    }
    layout
}

/// What emitting the region's instructions reads: the loop, its calls and their layout.
#[derive(Clone, Copy)]
pub(super) struct Plan<'c, 'a> {
    pub(super) header: u32,
    pub(super) backedge: u32,
    pub(super) caller: &'c [WideInstruction],
    pub(super) sites: &'c [FieldSite],
    pub(super) live: &'c RegisterMasks,
    pub(super) calls: &'c PcMap<Site<'a>>,
    /// The caller PCs whose instructions drop out.
    pub(super) dropped: &'c PcMap<()>,
    /// The setup PCs by the PC they resume at.
    pub(super) setup: &'c PcMap<u32>,
    /// The caller instructions the region runs rewritten.
    pub(super) rewritten: &'c PcMap<WideInstruction>,
    pub(super) layout: &'c Layout,
}

impl Plan<'_, '_> {
    fn mask(&self, pc: u32) -> Option<RegisterSet> {
        self.live.get(pc as usize)
    }
}

/// The region's instructions with each one's origin and live registers.
pub(super) struct Emission<'c, 'a> {
    plan: Plan<'c, 'a>,
    pub(super) instructions: Vec<WideInstruction>,
    pub(super) origins: Vec<Origin>,
    pub(super) masks: Vec<RegisterSet>,
    leaves: Vec<u32>,
    pub(super) callee_captures: Vec<(usize, u16)>,
}

/// One call's callee in the region: its registers renamed past the caller's.
struct CallView<'s, 'a> {
    site: &'s Site<'a>,
    frame: InlineFrame,
    index: u16,
    flat: &'s [u32],
    /// The caller's registers live past the call, less its result.
    after: RegisterSet,
    continuation: u32,
}

impl CallView<'_, '_> {
    fn register(&self, register: Register) -> Register {
        match self.frame.bound.get(usize::from(register)).copied().flatten() {
            Some(source) => source,
            None => self.frame.base + register,
        }
    }

    /// A callee's live registers there, in the region, with the caller's live past the call.
    fn frame_mask(&self, callee_pc: usize) -> Option<RegisterSet> {
        let own = self.site.callee.live.get(callee_pc)?;
        let mut set = self.after;
        for callee_register in 0..self.site.callee.registers {
            if register_in(own, usize::from(callee_register)) {
                set |= RegisterSet::bit(usize::from(self.register(callee_register)));
            }
        }
        Some(set)
    }

    fn target(&self, target: u32) -> Option<u32> {
        self.flat.get(target as usize).copied().filter(|_| self.site.reached.contains(target as usize))
    }
}

impl<'c, 'a> Emission<'c, 'a> {
    pub(super) fn new(plan: Plan<'c, 'a>) -> Self {
        let length = plan.layout.length as usize;
        Self {
            plan,
            instructions: Vec::with_capacity(length),
            origins: Vec::with_capacity(length),
            masks: Vec::with_capacity(length),
            leaves: Vec::new(),
            callee_captures: Vec::new(),
        }
    }

    fn push(&mut self, instruction: WideInstruction, origin: Origin, set: RegisterSet) {
        self.instructions.push(instruction);
        self.origins.push(origin);
        self.masks.push(set);
    }

    /// Emits every kept caller instruction and each call's callee, then the jumps leaving
    /// the region.
    pub(super) fn run(&mut self) -> Option<()> {
        let plan = self.plan;
        for (offset, instruction) in plan.caller.iter().copied().enumerate() {
            let pc = plan.header + offset as u32;
            if plan.dropped.contains(pc) {
                continue;
            }
            // A setup instruction leaves as the first of its run does.
            let resumes = plan.setup.get(pc).copied().unwrap_or(pc);
            let instruction = plan.rewritten.get(pc).copied().unwrap_or(instruction);
            match plan.calls.get(pc) {
                Some(site) => self.call(offset, pc, site)?,
                None => self.plain(instruction, resumes)?,
            }
        }
        debug_assert_eq!(self.instructions.len(), plan.layout.length as usize);
        for target in &self.leaves {
            self.origins.push(Origin::Leave(*target));
            self.masks.push(plan.mask(*target)?);
        }
        Some(())
    }

    /// The region PC of a caller target: inside the loop its flat PC, else a jump out of it.
    fn caller_target(&mut self, target: u32) -> u32 {
        let plan = self.plan;
        if (plan.header..=plan.backedge).contains(&target) {
            return plan.layout.caller[(target - plan.header) as usize];
        }
        let index = self.leaves.iter().position(|leave| *leave == target).unwrap_or_else(|| {
            self.leaves.push(target);
            self.leaves.len() - 1
        });
        plan.header + plan.layout.length + index as u32
    }

    /// A caller instruction keeps its registers; a jump takes the region PC of its target.
    fn plain(&mut self, instruction: WideInstruction, resumes: u32) -> Option<()> {
        let renamed = if instruction.op().immediate_role() == ImmediateRole::JumpTarget {
            let target = self.caller_target(instruction.imm());
            WideInstruction::new(instruction.op(), instruction.a(), instruction.b(), instruction.c(), target)
        } else {
            instruction
        };
        let parts = inlined_set_this(renamed, self.plan.sites).unwrap_or_else(|| vec![renamed]);
        let resume_mask = self.plan.mask(resumes)?;
        for part in parts {
            self.push(part, Origin::Caller(resumes), resume_mask);
        }
        Some(())
    }

    /// A call's numeric parameters' moves, then its callee's reached instructions.
    fn call(&mut self, offset: usize, pc: u32, site: &Site<'a>) -> Option<()> {
        let plan = self.plan;
        self.bind_parameters(site)?;
        let continuation = plan.layout.caller.get(offset + 1).copied().unwrap_or_else(|| self.caller_target(pc + 1));
        let view = CallView {
            site,
            frame: site.frame,
            index: plan.calls.iter().position(|(key, _)| key == pc)? as u16,
            flat: plan.layout.callee.get(pc)?,
            after: plan.mask(pc + 1)? & !RegisterSet::bit(usize::from(site.result)),
            continuation,
        };
        for (callee_pc, instruction) in site.callee.instructions.iter().copied().enumerate() {
            if site.reached.contains(callee_pc) {
                self.callee_instruction(&view, callee_pc, instruction)?;
            }
        }
        Some(())
    }

    fn bind_parameters(&mut self, site: &Site<'_>) -> Option<()> {
        for parameter in 0..site.callee.params {
            if site.frame.bound[usize::from(parameter)].is_some() {
                continue;
            }
            let copy = crate::vm::compiled::meaning_compiled::move_record(site.frame.base + parameter, site.window + parameter);
            self.push(copy, Origin::Caller(site.resume), self.plan.mask(site.resume)?);
        }
        Some(())
    }

    /// Completes the call as the callee's `Return` does: the result's move, then the jump
    /// past the call.
    fn complete(&mut self, view: &CallView<'_, '_>, returned: Register, origin: Origin, set: RegisterSet) {
        self.push(crate::vm::compiled::meaning_compiled::move_record(view.site.result, returned), origin, set);
        self.push(crate::vm::compiled::meaning_compiled::jump_record(view.continuation), origin, set);
    }

    fn callee_instruction(&mut self, view: &CallView<'_, 'a>, callee_pc: usize, instruction: WideInstruction) -> Option<()> {
        let origin = Origin::Inlined { frame: view.index, pc: callee_pc as u32 };
        let set = view.frame_mask(callee_pc)?;
        if crate::staged::returns(instruction.op()) {
            self.complete(view, view.register(instruction.register_a()), origin, set);
            return Some(());
        }
        // A returning instruction (`result = op; return result`) computes into its own
        // register, then completes the call as the callee's `Return` does.
        if inlined_returning(instruction) {
            let plain = WideInstruction::new(instruction.op(), instruction.result_register(), instruction.b(), instruction.c(), instruction.imm());
            self.push(view.rename(plain)?, origin, set);
            self.complete(view, view.register(instruction.result_register()), origin, set);
            return Some(());
        }
        // A fused read stored into a `this` field runs as its read, then the store.
        let parts = inlined_set_this(instruction, self.plan.sites).unwrap_or_else(|| vec![instruction]);
        for part in parts {
            let part = self.callee_part(view, part)?;
            self.push(part, origin, set);
        }
        Some(())
    }

    fn callee_part(&mut self, view: &CallView<'_, 'a>, instruction: WideInstruction) -> Option<WideInstruction> {
        let instruction = if crate::staged::reads_capture(instruction.op()) { self.callee_capture(view.index, instruction)? } else { instruction };
        // The callee's `this` is the call's receiver, a caller register: its store of a
        // `this` field is a store to that register's object.
        if !crate::staged::writes_this_field(instruction.op()) {
            return view.rename(instruction);
        }
        let store = view.rename(crate::vm::compiled::meaning_compiled::field_store_record(instruction.a(), 0, instruction.c(), instruction.imm()))?;
        Some(crate::vm::compiled::meaning_compiled::field_store_record(store.a(), view.frame.receiver, store.c(), store.imm()))
    }

    /// A callee's own capture reads its environment, which its identity guard fixes
    /// and no instruction of the region stores: the entry snapshots it.
    fn callee_capture(&mut self, frame: u16, instruction: WideInstruction) -> Option<WideInstruction> {
        (instruction.capture_depth() == 0 && instruction.capture_slot() < CALLEE_CAPTURE).then_some(())?;
        let capture = (usize::from(frame), instruction.capture_slot());
        let position = self.callee_captures.iter().position(|existing| *existing == capture).unwrap_or_else(|| {
            self.callee_captures.push(capture);
            self.callee_captures.len() - 1
        });
        let slot = CALLEE_CAPTURE | u16::try_from(position).ok().filter(|position| *position < CALLEE_CAPTURE)?;
        Some(crate::vm::compiled::meaning_compiled::capture_load_record(instruction.a(), instruction.b(), instruction.c(), ImmediateLayout::capture_immediate(0, slot)))
    }
}

impl CallView<'_, '_> {
    /// `instruction` with the callee's registers, `this` and jump targets in the region.
    fn rename(&self, instruction: WideInstruction) -> Option<WideInstruction> {
        renamed(instruction, &|register| self.register(register), self.frame.receiver, &|target| self.target(target))
    }
}

/// The guard of a callee read through `path`: its field, interned in `fields`, read through
/// the region's held registers.
fn field_guard<'h>(
    path: FieldPath,
    held: &'h dyn Fn(Register) -> Option<&'h FieldPath>,
    defines: &dyn Fn(Register) -> bool,
    fields: &mut Vec<FieldUse>,
) -> Option<usize> {
    let FieldPath { root, steps } = path;
    let path = match root {
        FieldReceiver::Register(register) => match held(register) {
            Some(path) => FieldPath { root: path.root, steps: path.steps.iter().copied().chain(steps).collect() },
            None => FieldPath { root, steps },
        },
        _ => FieldPath { root, steps },
    };
    // The root is `this`, or a register the region does not write (`field_path`).
    if let FieldReceiver::Register(register) = path.root {
        (!defines(register)).then_some(())?;
    }
    Some(intern_field(fields, &path, true))
}

/// Each call's frame with its guard: the callee's field, which the entry reads and guards
/// still holds it, read through the region's held registers.
pub(super) fn guarded_frames<'h>(
    calls: PcMap<Site<'_>>,
    checked_reads: &PcMap<u32>,
    held: &'h dyn Fn(Register) -> Option<&'h FieldPath>,
    caller_registers: u16,
    defines: &dyn Fn(Register) -> bool,
    region: &mut LoopRegion,
) -> Option<Vec<InlineFrame>> {
    let mut frames = Vec::new();
    for (call_pc, site) in calls.into_entries() {
        let mut frame = site.frame;
        if checked_reads.contains(call_pc) {
            frame.guard = InlineGuard::AtRead;
        } else {
            let written = frame.receiver >= caller_registers || defines(frame.receiver);
            (!written || held(frame.receiver).is_some()).then_some(())?;
            if let Some(path) = site.callee_path {
                frame.guard = InlineGuard::Field(field_guard(path, held, defines, &mut region.fields)?);
            }
        }
        frames.push(frame);
    }
    (region.fields.len() <= MAX_REGION_FIELDS).then_some(())?;
    Some(frames)
}
