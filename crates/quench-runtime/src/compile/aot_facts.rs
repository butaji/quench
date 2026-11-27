//! Facts for AOT: what holds of the program, solved the same way in every world (`gate::World`).
//! A fact is a guess held under an edge where new code can move its key: a `Function`, `eval` or
//! dynamic import may add code that changes it (`AotFacts::live`). Each fact is a dependency in the
//! one dependency system (`code_dependencies`), never a per-use check, and is read by every tier
//! that lowers the program (lane bodies, BaseJIT bodies), so no layer re-derives it. A key that is
//! not live is settled for good: its readers keep no subscription.
//!
//! - Call targets: a call whose callee register holds a closure, or a binding written exactly
//!   once with one (`census`), names that function (direct call, inlinable). A live binding's or
//!   member's target is a guess the call's record guards.
//! - Number representations: registers that hold only Numbers at every read and write, as
//!   `Int32` or `Float64` (`flow`), so lane bodies keep them unboxed.
//!
//! The analyses run over the bytecode (`flow`, `effects`) and reuse its dataflow driver
//! (`dataflow.rs`), its successor table and its register uses. Owner: AOT facts.

mod blocks;
mod census;
mod effects;
mod flow;
mod gate;
mod scope;
mod types;
mod value;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use crate::bytecode::ResidualProgram;
use crate::typed_ir::Representation;
use census::Census;
use flow::FunctionFlow;
use scope::Scope;
pub use gate::World;
pub use flow::RegisterCells;

/// The facts of one program, solved in `world`.
pub struct AotFacts {
    /// The world the facts were solved in: which keys new code can move (`live`).
    world: World,
    /// `(function, pc)` of a plain call → the function it always calls, or, for a method read
    /// (`guessed_calls`), the one closure every store of its name writes.
    pub(crate) call_targets: BTreeMap<(u32, u32), u32>,
    /// The `call_targets` that are guesses, not proofs: a method read (the receiver may hold another
    /// value of that name: a builtin's, a closure made again, a store through a computed key), or a
    /// target read from a live key (`World::live`: new code may store another closure there). Only a
    /// row that guards the callee it reaches takes one (`FunctionFacts::call_target`); one that drops
    /// the guard takes the proven ones only (`FunctionFacts::proven_call_target`).
    pub(crate) guessed_calls: std::collections::BTreeSet<(u32, u32)>,
    /// `(function, pc)` of a method call whose name the program stores two or more closures under → that closed set
    /// (`Census::candidates`), each a guess the call's records guard.
    pub(crate) call_candidates: BTreeMap<(u32, u32), Box<[u32]>>,
    /// `(function, pc)` of a construction (`new`) → the function it always constructs.
    pub(crate) construct_targets: BTreeMap<(u32, u32), u32>,
    /// Function id → register → the Number representation it always holds.
    pub(crate) numbers: BTreeMap<u32, Box<[Option<Representation>]>>,
    /// Per function, the names its `prototype` is given, in code order (`prototype_fields`).
    pub(crate) prototype_fields: BTreeMap<u32, Vec<u32>>,
    /// Per field name, the type of the values every store of it writes, into an object of any class: the
    /// join of the name's `FactKey::Field` and `FactKey::AnyField` cells, which a tier keeps only under the
    /// slot's dependency.
    pub(crate) field_types: BTreeMap<u32, crate::typed_ir::Type>,
    /// The settled cell of every key of the program (`typed_ir::FactKey`): each class's own fields (joined
    /// with the stores into objects of no one class), returns, bindings, array fields' elements, parameters
    /// and receivers. The one store of the solve's facts: code that trusts a cell subscribes to its key.
    pub(crate) cells: BTreeMap<crate::typed_ir::FactKey, crate::typed_ir::Cell>,
    /// `(function, pc)` of a named field access → the classes of the objects it reads or stores, and of a call →
    /// the classes of its receiver, where the settled cells list them (`typed_ir::Classes::Set`): a guess.
    pub(crate) access_classes: BTreeMap<(u32, u32), crate::typed_ir::Classes>,
    /// Per function, the cells its registers hold at every point of the settled solve (`flow::RegisterCells`):
    /// what a tier lowering the function reads instead of re-deriving a register's type, range or classes.
    pub(crate) registers: BTreeMap<u32, flow::RegisterCells>,
}

impl AotFacts {
    /// No fact yet, solved in `world`: the solve fills the maps.
    fn unsolved(world: World) -> Self {
        Self {
            world,
            call_targets: BTreeMap::new(),
            guessed_calls: std::collections::BTreeSet::new(),
            call_candidates: BTreeMap::new(),
            construct_targets: BTreeMap::new(),
            numbers: BTreeMap::new(),
            prototype_fields: BTreeMap::new(),
            field_types: BTreeMap::new(),
            cells: BTreeMap::new(),
            access_classes: BTreeMap::new(),
        }
    }

    /// Whether new code (`World`) can move `key`. A tier that trusts a live key's fact subscribes to
    /// the key (its `code_dependencies` cell readers), and an edge retires those readers. A key that is
    /// not live needs no subscription and no recovery: a closed unit has no exits.
    pub fn live(&self, key: crate::typed_ir::FactKey) -> bool {
        self.world.live(key)
    }

    /// The names stored into `function`'s `prototype`, each once, in the order the program's code
    /// first stores them (`AotFacts::prototype_fields`): a guess of its prototype's layout after
    /// `constructor`, which a loader checks against the object it finds (`aot_image::symbols`).
    pub fn prototype_fields(&self, function: u32) -> &[u32] {
        self.prototype_fields.get(&function).map_or(&[], |fields| fields.as_slice())
    }

    /// The type of the values every store of field `atom` writes (`types`): a guess of what its
    /// slots hold, which a tier holds as the slot's type under the slot's dependency, so a store of
    /// anything else retires the code. Any value where a store may write the name unseen.
    pub fn field_type(&self, atom: u32) -> crate::typed_ir::Type {
        self.field_types.get(&atom).copied().unwrap_or(crate::typed_ir::Type::ANY)
    }

    /// The type of field `atom` of class `class`'s instances (`class_field_types`), else the name's (`field_type`):
    /// a guess held as the slot's type under the slot's dependency, as `field_type` is.
    pub fn field_type_on(&self, class: u32, atom: u32) -> crate::typed_ir::Type {
        self.cell(crate::typed_ir::FactKey::Field { class, instance: true, atom }).map_or_else(|| self.field_type(atom), |cell| cell.ty)
    }

    /// The settled cell of `key` (`cells`), none where the solve keeps none.
    pub fn cell(&self, key: crate::typed_ir::FactKey) -> Option<crate::typed_ir::Cell> {
        self.cells.get(&key).copied()
    }

    /// The Any ratchet: how many field names the program stores whose type is `any`, and how many
    /// field names it stores at all (`field_types`). A proof that sharpens a field's type lowers the
    /// first count, so it can only fall as the solve and the lowerings sharpen the types.
    pub fn any_fields(&self) -> (usize, usize) {
        let any = self.field_types.values().filter(|ty| **ty == crate::typed_ir::Type::ANY).count();
        (any, self.field_types.len())
    }

    /// The functions some construction (`new`) always constructs.
    pub fn constructed_functions(&self) -> std::collections::BTreeSet<u32> {
        self.construct_targets.values().copied().collect()
    }

    /// The facts of function `function`, for its lowering.
    pub fn function_facts(&self, function: u32) -> FunctionFacts<'_> {
        FunctionFacts { facts: self, function }
    }
}

/// The facts of one function.
pub struct FunctionFacts<'a> {
    facts: &'a AotFacts,
    function: u32,
}

impl<'a> FunctionFacts<'a> {
    /// The function the plain call at `pc` calls: proven, or a guess (`AotFacts::guessed_calls`: a method
    /// read's, or one read from a live key). A row taking it guards the callee it reaches.
    pub fn call_target(&self, pc: u32) -> Option<u32> {
        self.facts.call_targets.get(&(self.function, pc)).copied()
    }

    /// The functions the call at `pc` may call: its one target, or the closed set of a method read (`call_candidates`).
    pub fn call_targets(&self, pc: u32) -> Vec<u32> {
        match self.facts.call_candidates.get(&(self.function, pc)) {
            Some(set) => set.to_vec(),
            None => self.call_target(pc).into_iter().collect(),
        }
    }

    /// The function the plain call at `pc` always calls, proven (not a guess under `guessed_calls`): a row may
    /// drop its callee guard for it.
    pub fn proven_call_target(&self, pc: u32) -> Option<u32> {
        self.call_target(pc).filter(|_| !self.facts.guessed_calls.contains(&(self.function, pc)))
    }

    /// The class (constructed function) of every object the named field read or store at `pc` reads or stores,
    /// where the settled types name one (`AotFacts::field_owners`): a guess, held under the layout's guard.
    pub fn field_owner(&self, pc: u32) -> Option<u32> {
        self.access_classes(pc).single().map(|(class, _)| class)
    }

    /// The cell register `register` holds just before the instruction at `pc` in the settled solve
    /// (`RegisterCells::at`): its type with its range, the classes of its objects and what its arrays keep.
    /// None where the solve did not settle or no path reaches `pc`. A tier lowers the instruction from it:
    /// a guard the cell decides is never built.
    pub fn cell_at(&self, pc: u32, register: u16) -> Option<crate::typed_ir::Cell> {
        self.facts.registers.get(&self.function)?.at(pc, register)
    }

    /// The cells of every register at the entry of the block starting at `pc` (`RegisterCells::entry`).
    pub fn block_entry(&self, pc: u32) -> Option<&'a [crate::typed_ir::Cell]> {
        self.facts.registers.get(&self.function)?.entry(pc)
    }

    /// The classes of the objects the named field access at `pc` reads or stores, or of the receiver the call at
    /// `pc` passes (`AotFacts::access_classes`): a closed union a class switch dispatches on, a guess.
    pub fn access_classes(&self, pc: u32) -> crate::typed_ir::Classes {
        self.facts.access_classes.get(&(self.function, pc)).copied().unwrap_or(crate::typed_ir::Classes::Many)
    }

    /// The class of every `this` the function's recorded calls pass (`AotFacts::this_classes`): a guess.
    pub fn this_class(&self) -> Option<u32> {
        self.facts.cell(crate::typed_ir::FactKey::Receiver(self.function)).and_then(|cell| cell.classes.single()).map(|(class, _)| class)
    }

    /// The function the construction (`new`) at `pc` always constructs.
    pub fn construct_target(&self, pc: u32) -> Option<u32> {
        self.facts.construct_targets.get(&(self.function, pc)).copied()
    }

    /// Per register, the Number representation it always holds.
    pub fn numbers(&self) -> Option<&'a [Option<Representation>]> {
        self.facts.numbers.get(&self.function).map(|numbers| &**numbers)
    }
}

/// The facts of `program`, solved in the world its source proves (`world`).
pub fn analyze(program: &ResidualProgram) -> AotFacts {
    analyze_as(program, world(program))
}

/// How closed `program` is, as its source says (`gate`): an image keeps the answer its compile
/// found (`AotImage::world`), so a binary without the source frontend derives the same facts
/// again (`analyze_as`).
pub fn world(program: &ResidualProgram) -> World {
    gate::world(program)
}

/// The facts of `program`, solved the same way in every world: `world` only marks the keys new code
/// can move (`AotFacts::live`), whose facts the census keeps as guesses (`Census::guessed`).
pub fn analyze_as(program: &ResidualProgram, world: World) -> AotFacts {
    let scope = Scope::of(program);
    let analyze_typed = |resolved: &std::collections::BTreeMap<super::aot_facts::value::Key, u32>, types: Option<&types::Types>| -> Vec<FunctionFlow> {
        (0..program.function_count())
            .map(|id| program.with_record(id, |function| flow::analyze_function(id as u32, function,
                (&scope, crate::staged::DecodeContext::of(program), &program.method_arguments), resolved, types)))
            .collect()
    };
    let analyze = |resolved: &std::collections::BTreeMap<super::aot_facts::value::Key, u32>| analyze_typed(resolved, None);
    // The first pass finds the bindings that hold one closure; the second reads their loads as
    // that closure, so a class object or prototype reached through a binding keeps its identity
    // and its methods prove per class, not per property name.
    let first = analyze(&std::collections::BTreeMap::new());
    let resolved = Census::of(&first, world).singles();
    let flows = analyze(&resolved);
    let census = Census::of(&flows, world);
    let mut facts = AotFacts::unsolved(world);
    for (id, flow) in flows.iter().enumerate() {
        let id = id as u32;
        for call in flow.calls.iter().filter(|call| call.target == flow::Target::Register) {
            if !call.construct && let Some(set) = census.candidates(call.callee) {
                facts.call_candidates.insert((id, call.pc), set.iter().copied().collect());
            }
            let Some(target) = census.target(call.callee) else { continue };
            let guessed = census.guessed(call.callee);
            if call.construct {
                if !guessed { facts.construct_targets.insert((id, call.pc), target); }
            } else {
                facts.call_targets.insert((id, call.pc), target);
                if guessed { facts.guessed_calls.insert((id, call.pc)); }
            }
        }
        for &(base, atom) in &flow.prototype_writes {
            let Some(owner) = census.prototype_owner(base) else { continue };
            let fields = facts.prototype_fields.entry(owner).or_default();
            if !fields.contains(&atom) { fields.push(atom); }
        }
        if flow.numbers.iter().any(Option::is_some) {
            facts.numbers.insert(id, flow.numbers.clone());
        }
    }
    // The field, return, parameter and binding types, solved from the bottom (`types`): each pass reads
    // the types the one before found, and the call targets the passes above proved; the least solution is
    // the first pass that changes nothing. A solve that does not settle in `TYPE_PASSES` keeps no type.
    let mut found = types::Types::of(&flows, &facts, &census).bottom();
    let mut settled = false;
    for _ in 0..types::TYPE_PASSES {
        let next = types::Types::of(&analyze_typed(&resolved, Some(&found)), &facts, &census);
        settled = next == found;
        found = next;
        if settled { break; }
    }
    if crate::trace::Channel::AotLanes.on() {
        let name = |id: u32| program.with_record(id as usize, |function| function.name).and_then(|atom| program.atoms.get(atom as usize)).map_or("?", |name| name.as_ref());
        eprintln!("aot-lanes: types settled {settled}; parameters {:?}", found.parameter_types().iter().filter(|(_, types)| !types.is_empty()).map(|(id, types)| (name(*id), types)).collect::<Vec<_>>());
    }
    if crate::trace::Channel::AotLanes.on() {
        let named = |atom: u32| program.atoms.get(atom as usize).map_or("?", |name| name.as_ref()).to_string();
        eprintln!("aot-lanes: classes {:?}", found.class_trace().into_iter().map(|(key, class)| (match key { value::Key::Member(atom) => named(atom), other => format!("{other:?}") }, class)).collect::<Vec<_>>());
    }
    // A method read off a value whose class the solve found (`crate::typed_ir::Classes`, a field holding instances of one
    // class) resolves per class (`Origin::MemberOn`, a guess): the calls the passes above found no target for
    // take it from a flow over the settled types.
    if settled {
        for (id, mut flow) in analyze_typed(&resolved, Some(&found)).into_iter().enumerate() {
            let id = id as u32;
            facts.registers.insert(id, std::mem::take(&mut flow.registers));
            for (&pc, &classes) in &flow.field_owners {
                if matches!(classes, crate::typed_ir::Classes::Set { .. }) { facts.access_classes.insert((id, pc), classes); }
            }
            for call in &flow.calls {
                if matches!(call.receiver.cell.classes, crate::typed_ir::Classes::Set { .. }) { facts.access_classes.insert((id, call.pc), call.receiver.cell.classes); }
            }
            for call in flow.calls.iter().filter(|call| call.target == flow::Target::Register && !call.construct) {
                if facts.call_targets.contains_key(&(id, call.pc)) { continue; }
                let Some(target) = census.target(call.callee) else { continue };
                facts.call_targets.insert((id, call.pc), target);
                if census.guessed(call.callee) { facts.guessed_calls.insert((id, call.pc)); }
                facts.call_candidates.remove(&(id, call.pc));
            }
        }
    }
    // A field no write of reaches (`Type::NEVER` at the fixpoint) is one the analysis sees no value of: any value.
    let seen = |ty: crate::typed_ir::Type| if ty == crate::typed_ir::Type::NEVER { crate::typed_ir::Type::ANY } else { ty };
    if settled {
        facts.field_types = found.fields().map(|(atom, ty)| (atom, seen(ty))).collect();
        facts.cells = found.fact_cells().map(|(key, cell)| (key, crate::typed_ir::Cell { ty: seen(cell.ty), ..cell })).collect();
    }
    if crate::trace::Channel::AotLanes.on() {
        let named = |atom: &u32| program.atoms.get(*atom as usize).map_or("?", |name| name.as_ref());
        eprintln!("aot-lanes: field types {:?}", facts.field_types.iter().map(|(atom, ty)| (named(atom), *ty)).collect::<Vec<_>>());
        eprintln!("aot-lanes: class field types {:?}", facts.cells.iter().filter_map(|(key, cell)| match *key {
            crate::typed_ir::FactKey::Field { class, instance: true, atom } if facts.field_types.get(&atom) != Some(&cell.ty) => Some((class, named(&atom), cell.ty)),
            _ => None,
        }).collect::<Vec<_>>());
        let (any, total) = facts.any_fields();
        eprintln!("aot-lanes: any fields {any} of {total}");
        eprintln!("aot-lanes: closed world {world:?}: {} call targets ({} guessed), {} closed sets, {} constructions, {} access classes, {} cells", facts.call_targets.len(), facts.guessed_calls.len(), facts.call_candidates.len(), facts.construct_targets.len(), facts.access_classes.len(), facts.cells.len());
    }
    facts
}
