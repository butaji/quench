//! Admission on value identities (JL10, `admit_lifetime_roles`): the fallback that splits each
//! register into its webs where the register analysis rejects a loop for a reused register.
#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationExt};
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use std::collections::{BTreeMap, BTreeSet};

/// The webs and analysis identities of a loop, and the instructions it is admitted on.
struct Renaming {
    flow: LoopRegion,
    values: Webs,
    pinned: BTreeSet<Register>,
    identities: BTreeMap<Definition, Register>,
    /// The VM register of each analysis register.
    original: Vec<Register>,
}

/// Admission on value identities (JL10): each register splits into its webs, so a register the
/// bytecode reuses for unrelated values (an element, then a number) gives each its own role, with
/// the same checks and emitter. A register named where no renaming reaches (a call window, a
/// field site's base, a write or optional register, an inliner's object) is pinned: its webs join
/// and keep its number. Analysis only: generated code and recovery still name VM slots. `None`
/// where an instruction names registers in an immediate this does not decode.
#[allow(clippy::too_many_arguments)]
pub(crate) fn admit_lifetime_roles(
    instructions: Vec<WideInstruction>,
    registers: usize,
    constants: &[Value],
    sites: &[FieldSite],
    live: Option<std::rc::Rc<quench_runtime::compile::liveness::RegisterMasks>>,
    header: u32,
    objects: &BTreeSet<Register>,
    method_calls: MethodCalls,
) -> Option<Result<LoopRegion, RegionAnalysisFailure>> {
    if live.is_none() || instructions.is_empty() || registers > MAX_REGION_REGISTERS {
        return declined(header, "no liveness or too many registers");
    }
    let Some(pinned) = pinned_registers(&instructions, sites, objects) else { return declined(header, "an immediate names registers") };
    let flow = flow_region(instructions, registers, live, header);
    let mut values = webs(&flow);
    join_pinned(&mut values, &flow, &pinned);
    let Some(canonical) = canonical_arrays(&flow, &values) else { return declined(header, "an array read has no single entry root") };
    let roots = analysis_roots(&values, &flow);
    let Some((identities, original)) = identities(&values, &flow, &roots, &pinned) else { return declined(header, "too many value identities") };
    let mut renaming = Renaming { flow, values, pinned, identities, original };
    let analysis = renaming.instructions(header, &canonical)?;
    let mut region = match admit_register_roles(analysis, renaming.original.len(), constants, sites, None, None, header, objects, method_calls) {
        Ok(region) => region,
        Err(failure) => return rejected(header, failure),
    };
    restore_registers(&mut region, &renaming.original);
    region.instructions = canonical;
    region.live = renaming.flow.live.take();
    Some(Ok(region))
}

/// The loop's instructions as a region every register of which enters as a number: what the
/// webs are computed over.
fn flow_region(instructions: Vec<WideInstruction>, registers: usize,
    live: Option<std::rc::Rc<quench_runtime::compile::liveness::RegisterMasks>>, header: u32) -> LoopRegion {
    LoopRegion {
        header, backedge: header + instructions.len() as u32 - 1,
        numbers: (0..registers).map(|register| register as Register).collect(),
        tagged: Vec::new(), arrays: Vec::new(), captures: Vec::new(), stored_captures: Vec::new(),
        fields: Vec::new(), field_reads: BTreeMap::new(), dynamic_field_reads: BTreeMap::new(),
        dynamic_arrays: BTreeSet::new(),
        field_stores: BTreeMap::new(), dynamic_field_stores: BTreeMap::new(), field_store_sites: Vec::new(), held: BTreeMap::new(),
        identity_branches: BTreeMap::new(), instructions, constants: BTreeMap::new(),
        live, inlining: None, stencil_only: false,
    }
}

/// The entry register an array access at `pc` reads through `register`: a temporary receiver
/// may be copied from an entry array, but its backing view must never be captured from the
/// unrelated value left in that slot at entry.
fn array_root(flow: &LoopRegion, values: &Webs, pc: u32, register: Register, visiting: &mut BTreeSet<(u32, Register)>) -> Option<Register> {
    if !visiting.insert((pc, register)) {
        return None;
    }
    let definitions = values.reaching((pc - flow.header) as usize, register);
    if definitions.is_empty() {
        return None;
    }
    let mut root = None;
    for definition in definitions {
        let candidate = definition_root(flow, values, definition, visiting)?;
        if root.is_some_and(|root| root != candidate) {
            return None;
        }
        root = Some(candidate);
    }
    visiting.remove(&(pc, register));
    root
}

/// The entry register `definition` copies: an entry value the loop never writes, or a plain
/// move of one.
fn definition_root(flow: &LoopRegion, values: &Webs, definition: Definition, visiting: &mut BTreeSet<(u32, Register)>) -> Option<Register> {
    if let Some(entry) = flow.entry_register(definition) {
        let written = flow.instructions.iter().any(|instruction| instruction.defined_register() == Some(entry));
        return (!written).then_some(entry);
    }
    let offset = definition.checked_sub(flow.entry_count())?;
    let instruction = *flow.instructions.get(offset)?;
    if !matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. })) || instruction.result_flags() != 0 {
        return None;
    }
    array_root(flow, values, flow.header + offset as u32, instruction.register_b(), visiting)
}

/// The array access `instruction` is, by its staged row: `Some(true)` for an element read, `Some(false)`
/// for an element write or the object-coercible check before one, `None` for any other.
fn element_access(instruction: WideInstruction) -> Option<bool> {
    use quench_runtime::staged::Operation;
    match Operation::<(), ()>::decode(instruction, quench_runtime::staged::DecodeContext::EMPTY)? {
        Operation::ElementRead { .. } => Some(true),
        Operation::ElementWrite { .. } | Operation::RequireObjectCoercible { .. } => Some(false),
        _ => None,
    }
}

/// Whether every definition of `receiver` reaching `offset` is an element read: a nested array
/// read, which reads through no entry root.
fn reads_element_value(flow: &LoopRegion, values: &Webs, offset: usize, receiver: Register) -> bool {
    Some(values.reaching(offset, receiver)).is_some_and(|definitions| {
        !definitions.is_empty() && definitions.iter().all(|definition| {
            definition.checked_sub(flow.entry_count())
                .and_then(|index| flow.instructions.get(index))
                .is_some_and(|definition| element_access(*definition) == Some(true))
        })
    })
}

/// The loop's instructions with each array access reading through its entry array's root (field
/// receivers rename as values do); `None` where an array read has no single entry root.
fn canonical_arrays(flow: &LoopRegion, values: &Webs) -> Option<Vec<WideInstruction>> {
    let mut canonical = flow.instructions.clone();
    for (offset, instruction) in canonical.iter_mut().enumerate() {
        let Some(reads) = element_access(*instruction) else { continue };
        let Some(receiver) = object_reads(*instruction) else { continue };
        let root = match array_root(flow, values, flow.header + offset as u32, receiver, &mut BTreeSet::new()) {
            Some(root) => root,
            None if reads && reads_element_value(flow, values, offset, receiver) => continue,
            None => return None,
        };
        let base = if reads { Operand::register(root).0 } else { root };
        *instruction = WideInstruction::new(instruction.op(), instruction.a(), base, instruction.c(), instruction.imm());
    }
    Some(canonical)
}

/// The web roots that need an analysis identity: entry values, and the instruction definitions
/// that define a register (an unused one has none).
fn analysis_roots(values: &Webs, flow: &LoopRegion) -> Vec<Definition> {
    let roots: BTreeSet<Definition> = (0..values.definitions()).map(|definition| values.find(definition)).collect();
    roots.into_iter().filter(|definition| {
        *definition < flow.entry_count()
            || number_definition(flow.instructions[*definition - flow.entry_count()]).is_some()
    }).collect()
}

impl Renaming {
    /// The loop's `canonical` instructions naming analysis registers; `None` (declined) where an
    /// operand does not rename.
    fn instructions(&self, header: u32, canonical: &[WideInstruction]) -> Option<Vec<WideInstruction>> {
        canonical.iter().copied().enumerate().map(|(offset, instruction)| self.instruction(header + offset as u32, instruction).or_else(|| {
            declined(header, &format!("an operand of {:?} at offset {offset} does not rename", instruction.op())).and(None)
        })).collect()
    }

    /// `instruction` at `pc` naming analysis registers; `None` where an operand does not rename.
    fn instruction(&self, pc: u32, instruction: WideInstruction) -> Option<WideInstruction> {
        let a = self.field(pc, instruction, InstructionField::A, instruction.a())?;
        let b = self.field(pc, instruction, InstructionField::B, instruction.b())?;
        let c = self.field(pc, instruction, InstructionField::C, instruction.c())?;
        Some(WideInstruction::new(instruction.op(), a, b, c, instruction.imm()))
    }

    fn read(&self, pc: u32, register: Register) -> Option<Register> {
        self.identities.get(&self.values.read(&self.flow, pc, register)).copied()
    }

    /// The field `field` of `instruction`, whose raw value is `raw`, naming analysis registers.
    fn field(&self, pc: u32, instruction: WideInstruction, field: InstructionField, raw: u16) -> Option<u16> {
        Some(match instruction.op().field_layout(field) {
            FieldLayout::ResultRegister => self.result(pc, instruction, raw)?,
            FieldLayout::Register => self.read(pc, raw)?,
            FieldLayout::Operand | FieldLayout::NumericIndexOperand => {
                let operand = Operand(raw);
                match operand.kind()? {
                    OperandKind::Register => Operand::register(self.read(pc, operand.payload())?).0,
                    OperandKind::Constant => raw,
                    _ => return None,
                }
            }
            // `this` and a nested lookup's site name no register here.
            FieldLayout::FieldBase => match FieldBase(raw).register_index() {
                Some(register) => FieldBase::register(self.read(pc, register)?).0,
                None => raw,
            },
            _ => raw,
        })
    }

    /// The analysis register of `instruction`'s result: its web's, or for a result no web tracks
    /// (a call's) its pinned register.
    fn result(&self, pc: u32, instruction: WideInstruction, raw: u16) -> Option<u16> {
        if instruction.result_flags() != 0 { return None; }
        if number_definition(instruction).is_none() { return self.pinned.contains(&raw).then_some(raw); }
        self.identities.get(&self.values.defined(&self.flow, pc)).copied()
    }
}

/// `region`, admitted on analysis registers, naming VM registers again (`original`): an entry
/// receiver's identity is its slot at entry, and a redefined one's reads go through the
/// canonical instructions' registers.
fn restore_registers(region: &mut LoopRegion, original: &[Register]) {
    let vm = |register: Register| original[usize::from(register)];
    let tagged: BTreeSet<Register> = region.tagged.iter().map(|register| vm(*register)).collect();
    region.numbers = region.numbers.iter().map(|register| vm(*register))
        .filter(|register| !tagged.contains(register)).collect::<BTreeSet<_>>().into_iter().collect();
    region.tagged = tagged.into_iter().collect();
    for array in &mut region.arrays { array.register = vm(array.register); }
    region.dynamic_arrays = region.dynamic_arrays.iter().map(|register| vm(*register)).collect();
    for branch in region.identity_branches.values_mut() { branch.register = vm(branch.register); }
    let receivers = region.fields.iter_mut().map(|field| &mut field.receiver)
        .chain(region.field_store_sites.iter_mut().map(|store| &mut store.receiver));
    for receiver in receivers {
        if let FieldReceiver::Register(register) = receiver { *register = vm(*register); }
    }
    region.held = std::mem::take(&mut region.held).into_iter().map(|(register, (mut path, index))| {
        if let FieldReceiver::Register(root) = &mut path.root { *root = vm(*root); }
        (vm(register), (path, index))
    }).collect();
}

/// The register analysis's `failure` on the renamed loop at `header` (`QUENCH_TRACE=osr-reject`).
fn rejected(header: u32, failure: RegionAnalysisFailure) -> Option<Result<LoopRegion, RegionAnalysisFailure>> {
    if quench_runtime::trace::Channel::OsrReject.on() {
        eprintln!("QUENCH_JS_JIT_OSR_LIFETIME_REJECT header={header} failure={failure:?}");
    }
    Some(Err(failure))
}

/// No analysis on value identities for the loop at `header`, for `reason` (`QUENCH_TRACE=osr-reject`).
fn declined(header: u32, reason: &str) -> Option<Result<LoopRegion, RegionAnalysisFailure>> {
    if quench_runtime::trace::Channel::OsrReject.on() {
        eprintln!("QUENCH_JS_JIT_OSR_LIFETIME_DECLINE header={header} reason={reason}");
    }
    None
}

/// The registers renaming must not split: those named by a call window, a register window, a write
/// or optional register field, a field site's base (the site table keeps VM slots), the result of
/// an instruction whose definition no web tracks (a call's), and `objects`;
/// `None` where an immediate names registers otherwise.
fn pinned_registers(instructions: &[WideInstruction], sites: &[FieldSite], objects: &BTreeSet<Register>)
    -> Option<BTreeSet<Register>> {
    use quench_runtime::bytecode::ImmediateLayout;
    let mut pinned = objects.clone();
    let window = |pinned: &mut BTreeSet<Register>, base: Register, count: u16| pinned.extend((0..count).map(|index| base + index));
    for instruction in instructions {
        match instruction.op().immediate_layout() {
            ImmediateLayout::CallWindow | ImmediateLayout::CallWindowWithEvalFlags | ImmediateLayout::SingleArgumentCallWindowWithEvalFlags => {
                let call = instruction.call_window();
                window(&mut pinned, call.base, call.count);
            }
            ImmediateLayout::RegisterPair | ImmediateLayout::ConstructCountAndFlags => return None,
            ImmediateLayout::Scalar | ImmediateLayout::CaptureDepthAndSlot => {}
        }
        for (field, raw) in [(InstructionField::A, instruction.a()), (InstructionField::B, instruction.b()), (InstructionField::C, instruction.c())] {
            match instruction.op().field_layout(field) {
                FieldLayout::WriteRegister | FieldLayout::ReadWriteRegister
                | FieldLayout::OptionalRegister | FieldLayout::OptionalReadRegister => { pinned.insert(raw); }
                FieldLayout::RegisterWindowBase => {
                    let registers = instruction.register_window();
                    window(&mut pinned, registers.base, registers.count);
                }
                _ => {}
            }
        }
        if number_definition(*instruction).is_none() {
            pinned.extend(instruction.defined_register());
        }
        if instruction.op().immediate_role() == quench_runtime::bytecode::ImmediateRole::FieldLookup
            && let Some(FieldLookup::Site(index)) = instruction.field_lookup() {
            pinned.extend(sites.get(index)?.base.register_index());
        }
    }
    Some(pinned)
}

/// Joins every web of each pinned register into one, its value at entry included.
fn join_pinned(values: &mut Webs, flow: &LoopRegion, pinned: &BTreeSet<Register>) {
    for &register in pinned.iter().filter(|register| usize::from(**register) < flow.entry_count()) {
        let definitions = flow.instructions.iter().enumerate()
            .filter(|(_, instruction)| number_definition(**instruction) == Some(register))
            .map(|(offset, _)| flow.entry_count() + offset);
        for definition in definitions.collect::<Vec<_>>() {
            values.union(usize::from(register), definition);
        }
    }
}

/// The analysis register of each web, and the VM register of each analysis register: a pinned
/// register's web, and the first web of any other register, keep the register's number; a further
/// web of an unpinned register takes a number past the function's registers.
fn identities(values: &Webs, flow: &LoopRegion, roots: &[Definition], pinned: &BTreeSet<Register>)
    -> Option<(BTreeMap<Definition, Register>, Vec<Register>)> {
    let registers = flow.entry_count();
    let mut original: Vec<Register> = (0..registers).map(|register| register as Register).collect();
    let mut taken = quench_runtime::bitset::BitSet::new(registers);
    let mut identities = BTreeMap::new();
    for &root in roots {
        let register = values.register(flow, root);
        let identity = if taken.checked_insert(usize::from(register))? || pinned.contains(&register) {
            register
        } else {
            original.push(register);
            Register::try_from(original.len() - 1).ok()?
        };
        identities.insert(root, identity);
    }
    (original.len() <= MAX_REGION_REGISTERS).then_some((identities, original))
}
