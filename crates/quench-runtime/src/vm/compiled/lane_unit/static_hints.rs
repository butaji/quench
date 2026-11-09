//! Static method-name, by-name, local-closure and captured-slot hints, resolved into flat
//! per-PC call targets (`static_call_targets`). A hint is never a proof: replacement and
//! aliasing are handled by call-edge admission, which tries each target a site names behind
//! its own body guard.
use crate::bytecode::{Atom, Constant, ControlFlowLayout, FieldLookup, Function, InstructionField, Op, PropertyDefinitionMode, StoreReceiver, WideInstruction};

/// The bodies a call site's callee may name statically: a superset of the callees it meets,
/// which the unit's inlining budgets choose among, each behind its own body guard. A name,
/// slot or register defined with more names none.
pub(crate) type StaticTargets = smallvec::SmallVec<[u32; super::super::tier_policy::POLYMORPHISM]>;

#[derive(Clone, Copy)]
enum Definition { Closure(u32), Name(Atom) }

/// The distinct bodies stored to one name or slot, every one: a call site narrows them to the
/// targets it guards (`Hints::narrow`).
type Candidates = Vec<u32>;

/// What a program's definitions name: per method atom, per global name and per local slot of
/// every function (flat, by `offsets`), the bodies stored there.
struct Hints {
    methods: Vec<Candidates>,
    named: Vec<Candidates>,
    captured: Vec<Candidates>,
    offsets: Vec<usize>,
    parents: Vec<Option<u32>>,
    /// Each function's declared parameter count.
    params: Vec<u16>,
    /// Each constant's atom when it is a string naming one, projected once.
    constant_names: Vec<Option<Atom>>,
}

/// Static call targets of every function of `program`, per PC.
pub(crate) fn static_call_targets(program: &crate::ResidualProgram) -> Vec<Vec<StaticTargets>> {
    let mut hints = Hints::new(program);
    for id in 0..program.function_count() {
        program.with_record(id, |function| hints.record(id, function));
    }
    hints.offsets.push(hints.captured.len());
    (0..program.function_count()).map(|id| program.with_record(id, |function| hints.targets(id, function))).collect()
}

/// Every register output retires the previous hypothesis before its new value is recorded.
fn clear_written_hints<T>(hints: &mut [Option<T>], instruction: WideInstruction) {
    for register in instruction.written_registers() { hints[usize::from(register)] = None; }
}

fn observe(slot: &mut Candidates, target: u32) {
    if !slot.contains(&target) { slot.push(target) }
}

/// The one body `targets` names, as a definition.
fn single(targets: Option<&Candidates>) -> Option<Definition> {
    match targets.map(|targets| targets.as_slice()) { Some(&[target]) => Some(Definition::Closure(target)), _ => None }
}

impl Hints {
    fn new(program: &crate::ResidualProgram) -> Self {
        let mut names: Vec<_> = program.atoms.iter().enumerate().map(|(atom, name)|
            (name, Atom::try_from(atom).expect("validated atom index"))).collect();
        names.sort_unstable_by_key(|&(name, _)| name);
        let constant_names = program.constants.iter().map(|constant| match constant {
            Constant::String(name) => names.binary_search_by(|(candidate, _)| candidate.cmp(&name.as_ref())).ok().map(|index| names[index].1),
            _ => None,
        }).collect();
        let atoms = vec![Candidates::new(); program.atoms.len()];
        Self { methods: atoms.clone(), named: atoms, captured: Vec::new(), offsets: Vec::with_capacity(program.function_count() + 1),
            parents: Vec::with_capacity(program.function_count()), params: Vec::with_capacity(program.function_count()), constant_names }
    }

    /// The bodies the slot a capture of function `id` reads holds. The lexical owner and
    /// source slot come from the same layout as capture execution: this identifies a possible
    /// body, never the closure; admission checks its body, and captures read its environment.
    fn captured(&self, id: usize, instruction: WideInstruction) -> Option<&Candidates> {
        let owner = (0..=instruction.capture_depth()).try_fold(id, |owner, _|
            self.parents.get(owner).copied().flatten().map(|parent| parent as usize))?;
        let end = self.offsets.get(owner + 1).copied().unwrap_or(self.captured.len());
        self.captured[self.offsets[owner]..end].get(usize::from(instruction.capture_slot()))
    }

    /// Observes the definitions of function `id`, in order.
    fn record(&mut self, id: usize, function: &Function) {
        let start = self.captured.len();
        self.offsets.push(start);
        self.parents.push(function.parent);
        self.params.push(function.params);
        self.captured.resize(start + usize::from(function.locals), Candidates::new());
        let mut definitions = vec![None; usize::from(function.registers)];
        for instruction in function.instructions() {
            self.note_store(start, function, instruction, &definitions);
            let value = self.definition(id, instruction, &definitions);
            clear_written_hints(&mut definitions, instruction);
            if let Some(register) = instruction.defined_register() { definitions[usize::from(register)] = value; }
            if instruction.op().control_flow_layout() != ControlFlowLayout::Fallthrough { definitions.fill(None); }
        }
    }

    /// Notes the body a store of a closure names: a method's (`note_method`), a local's, or a
    /// global's by name (a script root's function is both its global's and its same-named
    /// slot's, which its functions capture).
    fn note_store(&mut self, start: usize, function: &Function, instruction: WideInstruction, definitions: &[Option<Definition>]) {
        if !instruction.op().field_layout(InstructionField::A).is_register_field() { return; }
        let Some(Definition::Closure(target)) = definitions.get(usize::from(instruction.register_a())).copied().flatten() else { return };
        self.note_method(instruction, target, definitions);
        if let Some(slot) = function.stored_slot(instruction).and_then(|slot| self.captured[start..].get_mut(slot)) { observe(slot, target); }
        if instruction.op().stores_name() { observe(&mut self.named[instruction.atom_index() as usize], target); }
    }

    /// Notes `target` stored as a data property, or as a method record's of a literal name:
    /// class definitions use an explicit key register, and a computed key or accessor
    /// result remains dynamic.
    fn note_method(&mut self, instruction: WideInstruction, target: u32, definitions: &[Option<Definition>]) {
        let op = instruction.op();
        let atom = if op.field_store().map_or_else(|| op == Op::DefineField, |store| store.receiver == StoreReceiver::Register) {
            Some(instruction.atom_index())
        } else if op == Op::DefinePropertyRecord
            && matches!(instruction.property_definition_mode(), Some(PropertyDefinitionMode::Method | PropertyDefinitionMode::ReadonlyMethod))
            && let Some(Definition::Name(atom)) = definitions[usize::from(instruction.register_c())] {
            Some(atom)
        } else { None };
        if let Some(atom) = atom { observe(&mut self.methods[atom as usize], target); }
    }

    /// What `instruction` of function `id` defines its result as.
    fn definition(&self, id: usize, instruction: WideInstruction, definitions: &[Option<Definition>]) -> Option<Definition> {
        match instruction.op() {
            Op::MakeClosure => Some(Definition::Closure(instruction.closure_function_index())),
            Op::LoadConst => self.constant_names.get(instruction.constant_index()).copied().flatten().map(Definition::Name),
            Op::Move => definitions.get(usize::from(instruction.register_b())).copied().flatten(),
            op if op.loads_name() => single(self.named.get(instruction.atom_index() as usize)),
            op if op.addresses_capture() => single(self.captured(id, instruction)),
            _ => None,
        }
    }

    /// The targets a call of `argc` arguments guards among `candidates`: all of them within a
    /// switch's width (`tier_policy::POLYMORPHISM`), else those declaring as many parameters as
    /// it passes, the bodies written for such a call, where those fit; else none. Any choice
    /// is sound: each target sits behind its body guard and every other callee calls generically.
    fn narrow(&self, candidates: &[u32], argc: u16) -> StaticTargets {
        let width = super::super::tier_policy::POLYMORPHISM;
        if candidates.len() <= width { return candidates.iter().copied().collect(); }
        let matching: StaticTargets = candidates.iter().copied()
            .filter(|&target| self.params.get(target as usize) == Some(&argc)).take(width + 1).collect();
        if matching.len() <= width { matching } else { StaticTargets::new() }
    }

    /// The targets of function `id`'s calls, per PC.
    fn targets(&self, id: usize, function: &Function) -> Vec<StaticTargets> {
        let mut methods_by_register = vec![None::<Candidates>; usize::from(function.registers)];
        function.instructions().map(|instruction| {
            let target = (instruction.op().is_call()).then(|| methods_by_register[usize::from(instruction.register_b())].as_deref()
                .map(|candidates| self.narrow(candidates, instruction.call_window().count))).flatten().unwrap_or_default();
            let method = match instruction.op() {
                Op::GetField => match instruction.field_lookup() {
                    Some(FieldLookup::Atom { atom, .. }) => self.methods.get(atom as usize).cloned(),
                    _ => None,
                },
                Op::MakeClosure => Some(vec![instruction.closure_function_index()]),
                Op::LoadCapture => self.captured(id, instruction).cloned(),
                Op::Move => methods_by_register[usize::from(instruction.register_b())].clone(),
                _ => None,
            };
            clear_written_hints(&mut methods_by_register, instruction);
            if let Some(register) = instruction.defined_register() { methods_by_register[usize::from(register)] = method; }
            // Hints survive control-flow edges, including loops around a local closure. They
            // are hypotheses only: inline admission checks the actual closure/program/body
            // before any callee effect.
            target
        }).collect()
    }
}
