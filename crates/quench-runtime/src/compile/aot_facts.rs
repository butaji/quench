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
mod class_layout;
mod effects;
mod flow;
mod gate;
mod record;
mod registers;
mod scope;
mod shape;
mod types;
mod value;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use crate::bytecode::ResidualProgram;
use crate::staged::effects::{CallForm, Effects, Signature};
use crate::typed_ir::Representation;
use super::signatures::{self, Signatures};
use census::Census;
use class_layout::ClassLayouts;
use flow::FunctionFlow;
use scope::Scope;
pub use gate::World;
pub use registers::{RegisterCells, Source};
pub use scope::constant_type;

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
    /// with the stores into objects of no one class), returns, bindings, the elements of each element class
    /// (`Elements`, `UnnamedElements`), parameters and receivers. The one store of the solve's facts: code that
    /// trusts a cell subscribes to its key.
    pub(crate) cells: BTreeMap<crate::typed_ir::FactKey, crate::typed_ir::Cell>,
    /// `(function, pc)` of a named field access → the classes of the objects it reads or stores, and of a call →
    /// the classes of its receiver, where the settled cells list them (`typed_ir::Classes::Set`): a guess.
    pub(crate) access_classes: BTreeMap<(u32, u32), crate::typed_ir::Classes>,
    /// Per function, the cells and sources its registers hold at every point of the settled solve (`registers::RegisterCells`):
    /// what a tier lowering the function reads instead of re-deriving a register's type, range or classes.
    pub(crate) registers: BTreeMap<u32, RegisterCells>,
    /// Per function, the keys its settled cells were read from (`FunctionFacts::dependencies`).
    pub(crate) dependencies: BTreeMap<u32, Box<[crate::typed_ir::FactKey]>>,
    /// `(function, pc)` of a call whose callee was read from a key → the method name it reads (a member read) and
    /// that key: what a call plan's miss subscribes to (`call_plan::Miss::Edge`).
    pub(crate) call_keys: BTreeMap<(u32, u32), (Option<u32>, crate::typed_ir::FactKey)>,
    /// The composed effect signatures of the program's functions and call forms, solved over the settled facts
    /// above (`signatures::solve`); none where the type solve did not settle, so no signature is taken from it.
    pub(crate) signatures: Option<Signatures>,
    /// The closed-world layouts of the program's classes (`class_layout`): each instance layout and prototype layout
    /// the solve proved, read by `AotFacts::class_layout` and `AotFacts::prototype_layout`.
    pub(crate) layouts: ClassLayouts,
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
            registers: BTreeMap::new(),
            dependencies: BTreeMap::new(),
            call_keys: BTreeMap::new(),
            signatures: None,
            layouts: ClassLayouts::default(),
        }
    }

    /// The composed effect signatures the solve settled (`signatures`), none where the type solve did not settle.
    pub fn signatures(&self) -> Option<&Signatures> {
        self.signatures.as_ref()
    }

    /// Whether new code (`World`) can move `key`. A tier that trusts a live key's fact subscribes to
    /// the key (its `code_dependencies` cell readers), and an edge retires those readers. A key that is
    /// not live needs no subscription and no recovery: a closed unit has no exits.
    pub fn live(&self, key: crate::typed_ir::FactKey) -> bool {
        self.world.live(key)
    }

    /// Whether no code can be added to the program (`World::Closed`): no key is live, so a unit that
    /// trusts these facts reads no invalidation word and keeps no exit for them.
    pub fn closed(&self) -> bool {
        self.world == World::Closed
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

    /// The ordered named fields every instance of class `class` (a constructor's function id) holds, in slot order:
    /// proven where every construction of the class makes exactly this shape and no other code reshapes its instances
    /// (`class_layout`); None otherwise.
    pub fn class_layout(&self, class: u32) -> Option<&[u32]> {
        self.layouts.instances.get(&class).map(|fields| &**fields)
    }

    /// The names class `class`'s prototype takes, in the order the program's code first installs them, proven where no
    /// object of unknown class receives another name (`class_layout`); None otherwise.
    pub fn prototype_layout(&self, class: u32) -> Option<&[u32]> {
        self.layouts.prototypes.get(&class).map(|fields| &**fields)
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

/// Per function, the keys its settled cells were read from (`FunctionFacts::dependencies`): every key its steps
/// read (`flow::FunctionFlow::reads`), a name-wide field read standing for every class's field of that name and
/// the stores into objects of no one class, the returns of every function its calls may reach, its parameters
/// and its receiver. A sound over-approximation of each cell's provenance, per function.
fn dependencies(facts: &AotFacts, reads: BTreeMap<u32, std::collections::BTreeSet<value::Key>>, found: &types::Types)
    -> BTreeMap<u32, Box<[crate::typed_ir::FactKey]>> {
    use crate::typed_ir::FactKey;
    let mut fields: BTreeMap<u32, Vec<FactKey>> = BTreeMap::new();
    for key in facts.cells.keys() {
        if let FactKey::Field { atom, .. } = *key { fields.entry(atom).or_default().push(*key); }
    }
    reads.into_iter().map(|(function, keys)| {
        let mut set: std::collections::BTreeSet<FactKey> = std::collections::BTreeSet::new();
        for key in keys {
            set.insert(census::fact_key(key));
            match key {
                value::Key::Member(atom) => set.extend(fields.get(&atom).into_iter().flatten().copied()),
                // An element read depends on every key of its class: a store through any of them moves the class's elements.
                value::Key::Element(atom) => set.extend(found.element_members(atom).map(|member| census::fact_key(value::Key::Element(member)))),
                _ => {}
            }
        }
        for (&(caller, pc), _) in facts.call_targets.range((function, 0)..=(function, u32::MAX)) {
            set.extend(facts.function_facts(caller).call_targets(pc).into_iter().map(FactKey::Return));
        }
        for (_, set_of) in facts.call_candidates.range((function, 0)..=(function, u32::MAX)) {
            set.extend(set_of.iter().copied().map(FactKey::Return));
        }
        let params = found.param_count(function);
        set.extend((0..params).map(|index| FactKey::Parameter { function, index: index as u16 }));
        set.insert(FactKey::Receiver(function));
        (function, set.into_iter().collect())
    }).collect()
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

    /// The source of the value register `register` holds just before the instruction at `pc` (`RegisterCells::source_at`),
    /// where the solve settled: a binding it was loaded from or a property it was read off, else `Computed`. Computed also
    /// where no path reaches `pc` or the solve did not settle, the answer that assumes nothing about the value's origin.
    pub fn source_at(&self, pc: u32, register: u16) -> Source {
        self.facts.registers.get(&self.function).and_then(|cells| cells.source_at(pc, register)).unwrap_or(Source::Computed)
    }

    /// The cells of every register at the entry of the block starting at `pc` (`RegisterCells::entry`).
    pub fn block_entry(&self, pc: u32) -> Option<&'a [crate::typed_ir::Cell]> {
        self.facts.registers.get(&self.function)?.entry(pc)
    }

    /// The cell the function's returns join to (`FactKey::Return`): a call of the function defines it, since a call's
    /// result is the join of the returns of the callees it may reach (`types::Types::call`). Any value where the solve
    /// did not settle, so a caller that reads it assumes no more than that.
    pub fn return_cell(&self) -> crate::typed_ir::Cell {
        self.facts.cell(crate::typed_ir::FactKey::Return(self.function)).unwrap_or(crate::typed_ir::Cell::ANY)
    }

    /// The keys every cell of this function's settled solve was read from (`AotFacts::dependencies`): a unit that
    /// trusts `cell_at`/`block_entry` of the function subscribes exactly the live ones (`AotFacts::live`); in a
    /// closed world none is live, so it subscribes nothing and keeps no exit.
    pub fn dependencies(&self) -> &'a [crate::typed_ir::FactKey] {
        self.facts.dependencies.get(&self.function).map_or(&[], |keys| keys)
    }

    /// The method name and key the callee of the call at `pc` was read from (`AotFacts::call_keys`).
    pub fn call_key(&self, pc: u32) -> Option<(Option<u32>, crate::typed_ir::FactKey)> {
        self.facts.call_keys.get(&(self.function, pc)).copied()
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

    /// The slot of named field `atom` in instances of `class` (its index in `AotFacts::class_layout`), where the layout is
    /// proven: a fixed offset a lowering reads in place of the field's name.
    pub fn field_slot(&self, class: u32, atom: u32) -> Option<u16> {
        let slot = self.facts.class_layout(class)?.iter().position(|&field| field == atom)?;
        u16::try_from(slot).ok()
    }

    /// The function the construction (`new`) at `pc` always constructs.
    pub fn construct_target(&self, pc: u32) -> Option<u32> {
        self.facts.construct_targets.get(&(self.function, pc)).copied()
    }

    /// Per register, the Number representation it always holds.
    pub fn numbers(&self) -> Option<&'a [Option<Representation>]> {
        self.facts.numbers.get(&self.function).map(|numbers| &**numbers)
    }

    /// The composed effect signature of the function (`signatures::solve`): the leaves its steps can cause and the
    /// effects they have. The same settled facts that answer this file's queries decide it, and the same edges
    /// retire it. Top where the type solve did not settle.
    pub fn signature(&self) -> Signature {
        self.facts.signatures.as_ref().map_or(Signature::ANY, |signatures| signatures.of(self.function))
    }

    /// The graded effects of the step at `pc` (`Signatures::site_effects`): the operation's kind row met with what the
    /// settled facts prove of that site, so a plain read of a closed receiver is not graded as a call. The same facts
    /// and edges as `signature`. Top where the type solve did not settle.
    pub fn site_effects(&self, pc: u32) -> Effects {
        self.facts.signatures.as_ref().map_or(Effects::ANY, |signatures| signatures.site_effects(self.function, pc))
    }

    /// The call form of the call at `pc` (`staged::effects::call_form`): inlined where `inlinable`, else frameless
    /// where the callees the facts name cannot leave, collect or run guest code, else framed. The same settled facts
    /// decide it, and the same edges retire it. Framed where the type solve did not settle.
    pub fn call_form(&self, pc: u32, inlinable: bool) -> CallForm {
        match self.facts.signatures.as_ref() {
            Some(signatures) => signatures.call_form(self.function, pc, inlinable),
            None => crate::staged::effects::call_form(Signature::ANY, inlinable),
        }
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
    // A pass whose integer range still grows after `WIDENING_ROUNDS` widens it (`Types::widen`), so a counter that
    // climbs settles at int32's span rather than one step a pass.
    let mut found = types::Types::of(&flows, &facts, &census).bottom();
    let mut settled = false;
    let mut grown = BTreeMap::new();
    for _ in 0..types::TYPE_PASSES {
        let next = types::Types::of(&analyze_typed(&resolved, Some(&found)), &facts, &census).widen(&found, &mut grown);
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
    let mut reads: BTreeMap<u32, std::collections::BTreeSet<value::Key>> = BTreeMap::new();
    if settled {
        let mut settled_flows = analyze_typed(&resolved, Some(&found));
        for (id, flow) in settled_flows.iter_mut().enumerate() {
            let id = id as u32;
            facts.registers.insert(id, std::mem::take(&mut flow.registers));
            reads.insert(id, std::mem::take(&mut flow.reads));
            for call in &flow.calls {
                let keyed = match call.callee.origin {
                    Some(value::Origin::Binding(key)) => Some((None, census::fact_key(key))),
                    Some(value::Origin::Member(atom)) => Some((Some(atom), crate::typed_ir::FactKey::AnyField(atom))),
                    Some(value::Origin::MemberOn { class, instance, atom, .. }) => Some((Some(atom), crate::typed_ir::FactKey::Field { class, instance, atom })),
                    _ => None,
                };
                if let Some(keyed) = keyed { facts.call_keys.insert((id, call.pc), keyed); }
            }
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
        // The class layouts read the settled shapes: each construction's classes are the settled ones.
        facts.layouts = class_layout::of_program(program, &settled_flows, &census, &facts.constructed_functions());
    }
    // A field no write of reaches (`Type::NEVER` at the fixpoint) is one the analysis sees no value of: any value.
    let seen = |ty: crate::typed_ir::Type| if ty == crate::typed_ir::Type::NEVER { crate::typed_ir::Type::ANY } else { ty };
    if settled {
        facts.field_types = found.fields().map(|(atom, ty)| (atom, seen(ty))).collect();
        facts.cells = found.fact_cells().map(|(key, cell)| (key, crate::typed_ir::Cell { ty: seen(cell.ty), ..cell })).collect();
        facts.dependencies = dependencies(&facts, reads, &found);
        // The effect signatures are solved over the settled facts above, so a step leaves only where they leave it open
        // (`signatures::solve`).
        facts.signatures = Some(signatures::solve(program, &facts));
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
