//! The value types of the program: what every store of a field name writes (`Key::Member`), what
//! every return of a function returns (`Key::Return`), what every write of a global or slot binding
//! stores, and what the arguments of each function's calls pass its parameters (`parameters`), joined
//! in the one fact lattice (`typed_ir::Type`).
//!
//! The types are the least solution of one system: a store's type is the type of the value it writes,
//! which may be a read of a field or binding, a parameter, or the result of a call, typed in turn by the
//! types of their writes and of the callee's returns. The solve starts from the bottom (`Types::bottom`):
//! each pass reads the types the one before found, and the first pass that changes nothing is the least
//! solution, which a self-dependent field (`this.n = this.n | 1`) reaches where a solve from the top
//! stays at any value. A solve that does not settle keeps no type.
//!
//! The types are a guess, not a proof: a write the analysis does not see (a callback a builtin makes,
//! a store through a computed key, sloppy global code) may store anything. A tier holds a field type
//! only as the slot's type under the slot's own dependency (`SlotDependency`), so a store of anything
//! else, by any tier, widens the slot and retires the code: the fact is proven where the slot is
//! written, never at the read.

use super::accessors::Shadows;
use super::census::{Census, ElementClasses};
use super::escapes::{escapes, resolved};
use super::flow::FunctionFlow;
use super::gate::World;
use super::quiet::{Proofs, StringBuiltins};
use super::scope::Scope;
use super::value::{Key, Origin, Word};
use crate::staged::typing;
use crate::typed_ir::{Cell, Classes, ElementClass, Elements, FactKey, Identity, Sites, View, WIDENING_ROUNDS};
use super::AotFacts;
use crate::typed_ir::Type;
use crate::ValueType;
use std::collections::{BTreeMap, BTreeSet};

/// The most passes the solve takes. Each pass is one whole-program flow over the types the one before found,
/// and the types only climb, each through a lattice of bounded height, so the solve settles; a chain of fields
/// that feed one another (richards' task queue: `blocks` → `currentTcb` → `link` → `queue`) climbs one link a pass.
pub(super) const TYPE_PASSES: usize = 64;

/// The cells one pass found, which the next pass reads: every field, return, element and binding, call
/// result, parameter and receiver of the program as one cell of the fact lattice (`Cell`: a type and a
/// class, joined together).
#[derive(PartialEq, Eq)]
pub(crate) struct Types {
    /// Per key (a field name, a class's own field, a return, an element, a global or slot binding), the
    /// join of the values every write of it stores. A write the analysis does not see (sloppy-mode global
    /// code, a builtin writing through the global object) may store anything: the cells only feed the field
    /// types, a guess held under the fields' slot dependencies.
    cells: BTreeMap<Key, Cell>,
    /// Per call `(function, pc)`, the join of the returns of every function it may call.
    calls: BTreeMap<(u32, u32), Cell>,
    /// Per call `(function, pc)` that may call a known function, the first of its callees: the array the call returns is
    /// the array that callee's `Return` holds (`call`), the callees' return classes being one (`Types::of`).
    call_returns: BTreeMap<(u32, u32), u32>,
    /// Per function, the join of the arguments each parameter takes (`parameters`).
    params: BTreeMap<u32, Vec<Cell>>,
    /// Per function, the join of the `this` its calls pass (`Call::receiver`).
    receivers: BTreeMap<u32, Cell>,
    /// The cell of a key no write of has been seen yet: none (`Cell::NEVER`) in the solve's first pass
    /// (`bottom`), any value once the passes stop.
    unseen: Cell,
    /// The element classes the flows' links, the escapes and the calls make (`ElementClasses`): which classes name the
    /// same arrays.
    elements: ElementClasses,
    /// Per key, the integer bounds its widening moves a grown range to (`typing::widen`): `typing::thresholds` of the
    /// constants stored to the key (its singleton integer writes) and of the bounds its functions compare it with or
    /// mask it to (`FunctionFlow::bounds`), and int32's span. Each of these follows from the bytecode and the literals,
    /// not from the types the passes find, so the thresholds the passes widen to do not move under the solve.
    thresholds: BTreeMap<Key, Vec<i64>>,
    /// Whether a flow writes elements of arrays it cannot name (`effects::Events::wild_elements`): every class's elements
    /// are then any.
    wild_elements: bool,
    /// The stores that may shadow a named property of an instance (`accessors::Shadows`), which bound `holder`.
    shadows: Shadows,
    /// Per class, the parent its prototype chain links it to (`Accessors::parents`): the `extends` chain `holder` walks.
    parents: BTreeMap<u32, u32>,
    /// The world the program is solved in: a key new code can move is no fold (`World::live`).
    world: World,
    /// The String builtins the program leaves as the host installed them: the calls of them are typed by the string rules.
    strings: StringBuiltins,
}

impl Types {
    /// The cells `flows` store and return, with each call typed by the callees `facts` prove or
    /// guess for it, the parameters of each function, and the bindings.
    pub(super) fn of(flows: &[FunctionFlow], facts: &AotFacts, census: &Census, scope: &Scope) -> Self {
        // The arrays an escaped function returns or an unresolved call holds may reach code the solve does not follow, so
        // their classes join the unnamed one (`escapes`); the flows' links join the rest.
        let open = escapes(flows, facts, census, scope);
        let escaped = open.functions;
        let plain = open.plain;
        let passed_this = open.passed_this;
        let mut links: Vec<(ElementClass, ElementClass)> = flows.iter().flat_map(|flow| &flow.links).copied().collect();
        links.extend(open.classes.into_iter().map(|class| (class, ElementClass::Unnamed)));
        entry_links(flows, facts, census, &escaped, &mut links);
        let mut types = Self {
            cells: BTreeMap::new(),
            calls: BTreeMap::new(),
            call_returns: BTreeMap::new(),
            params: BTreeMap::new(),
            receivers: BTreeMap::new(),
            unseen: Cell::ANY,
            elements: ElementClasses::default(),
            wild_elements: flows.iter().any(|flow| flow.wild_elements),
            thresholds: BTreeMap::new(),
            shadows: census.accessors.shadows.clone(),
            parents: census.accessors.parents().clone(),
            world: facts.world,
            strings: scope.string_builtins(&Proofs::of(flows)),
        };
        let mut constants: BTreeMap<Key, Vec<i64>> = BTreeMap::new();
        for &(_, key, fact) in flows.iter().flat_map(|flow| &flow.writes) {
            let joined = types.cells.entry(key).or_insert(Cell::NEVER);
            *joined = joined.join(fact.cell);
            if let Some(stored) = fact.cell.ty.int_range().filter(|range| range.min == range.max) {
                constants.entry(key).or_default().push(stored.min);
            }
        }
        for &(key, bound) in flows.iter().flat_map(|flow| &flow.bounds) {
            constants.entry(key).or_default().push(bound);
        }
        types.thresholds = constants.into_iter().map(|(key, stored)| (key, typing::thresholds(stored))).collect();
        (types.calls, types.call_returns) = call_results(flows, facts, census, &types.cells, &mut links);
        types.elements = ElementClasses::of(links.iter());
        types.params = parameters(flows, facts, census);
        // An escaped function (`escapes`) may be called by code the solve cannot follow, with any `this` and arguments:
        // its parameters are any value, as are its `this` (below), whatever its recorded calls pass.
        for function in &escaped {
            if let Some(slots) = types.params.get_mut(function) { slots.iter_mut().for_each(|slot| *slot = Cell::ANY); }
        }
        for (id, flow) in flows.iter().enumerate() {
            for call in &flow.calls {
                for target in targets(id as u32, call, facts, census) {
                    // A plain call binds a sloppy callee's `this` to the global object; a strict callee's `this` is `undefined`.
                    let plain_call = call.plain && sloppy(flows, target);
                    let receiver = Cell { ty: receiver_type(call), global: call.receiver.cell.global || plain_call, ..call.receiver.cell };
                    let joined = types.receivers.entry(target).or_insert(Cell::NEVER);
                    *joined = joined.join(receiver);
                }
            }
        }
        // A method one class holds (`Key::MemberOn`: its prototype's, or its instances' own) is called on an
        // object of that class: its `this` is an instance of the class, a guess, whatever the calls' receivers'
        // join is (`this.task.run()` reaches four classes' `run`, each with its own class's `this`).
        // An escaped function is called with any `this` (`escapes`): the class its holder guesses does not narrow it.
        for (function, class) in holders(&types.cells) {
            if escaped.contains(&function) { continue; }
            types.receivers.entry(function).or_insert(Cell::ANY).classes = Classes::one(class, false);
        }
        for function in &escaped {
            let joined = types.receivers.entry(*function).or_insert(Cell::NEVER);
            *joined = joined.join(Cell::ANY);
        }
        // A `call`, `apply` or `bind` runs its function with its first argument as the `this` (`escapes::Escapes::passed_this`):
        // the argument joins the function's `this`, and the global object where it may be `undefined` or `null` and the function
        // is sloppy (a sloppy function binds those to the global object).
        for (function, this) in passed_this {
            let nullish = [ValueType::Undefined, ValueType::Null].into_iter().any(|ty| this.ty.may_be_languages(Type::language(ty)));
            let global = this.global || (nullish && sloppy(flows, function));
            let joined = types.receivers.entry(function).or_insert(Cell::NEVER);
            *joined = joined.join(Cell { global, ..this });
        }
        // Unknown code may call a function it was handed with no receiver (`escapes::Escapes::plain`): a sloppy one's `this`
        // is then the global object.
        for function in plain.iter().filter(|&&function| sloppy(flows, function)) {
            if let Some(receiver) = types.receivers.get_mut(function) { receiver.global = true; }
        }
        types
    }

    /// The solve's first guess (`aot_facts::analyze_as`): every cell these types name, none of it yet.
    /// The passes from it climb to the least solution, which a self-dependent field (`this.n = this.n | 1`)
    /// reaches where passes from the top stay at any value; a solve that stops short of a fixpoint keeps
    /// no type.
    pub(super) fn bottom(mut self) -> Self {
        let never = |cell: &mut Cell| *cell = Cell::NEVER;
        self.cells.values_mut().for_each(never);
        self.calls.values_mut().for_each(never);
        self.receivers.values_mut().for_each(never);
        self.params.values_mut().flatten().for_each(never);
        self.unseen = Cell::NEVER;
        self
    }

    /// The cell a read of `key` holds: the join of its writes', its class a guess; `unseen` where none is seen.
    /// A field's cell also names the elements its arrays keep (`with_elements`).
    pub(super) fn cell(&self, key: Key) -> Cell {
        let cell = self.cells.get(&key).copied().unwrap_or(self.unseen).guessed();
        match key {
            Key::Member(atom) | Key::MemberAny(atom) => self.with_elements(atom, cell),
            _ => cell,
        }
    }

    /// The cell a read of field `atom` off class `class`'s instances (`instance`) or static object holds: the
    /// join of that class's own stores of it (`Key::MemberOn`) and the stores into objects of no one class
    /// (`Key::MemberAny`), either of which may land in such an object. None where the class stores no `atom`.
    pub(super) fn cell_on(&self, class: u32, instance: bool, atom: u32) -> Option<Cell> {
        let own = self.cells.get(&Key::MemberOn { class, instance, atom })?;
        let cell = own.join(self.cells.get(&Key::MemberAny(atom)).copied().unwrap_or(Cell::NEVER)).guessed();
        Some(self.with_elements(atom, cell))
    }

    /// `cell` with the elements of field `atom`'s class (`stored_elements`): a field's cell so names what its arrays
    /// hold, whichever store put the array there, and whichever key a store through an alias wrote.
    fn with_elements(&self, atom: u32, cell: Cell) -> Cell {
        Cell { elements: cell.elements.join(self.stored_elements(ElementClass::Field(atom))), ..cell }
    }

    /// The elements the stores into the arrays of element class `class` write (`Key::Element` of every class of it,
    /// `ElementClasses::members`): the type and classes of the values stored, and the length they leave
    /// (`Elements::length`). A store into any array of the class is a store into each of its classes, so the join is
    /// over the class; `Elements::NEVER` where no store of the class is seen, and any value where a store the analysis
    /// cannot name may write an array of any class (`wild_elements`).
    pub(super) fn stored_elements(&self, class: ElementClass) -> Elements {
        if self.wild_elements { return Elements::ANY; }
        self.elements.members(class)
            .filter_map(|member| self.cells.get(&Key::Element(member)))
            .fold(Elements::NEVER, |elements, stored| elements.join(Elements { ty: stored.ty, classes: stored.classes, length: stored.elements.length, view: View::None, holes: stored.elements.holes }))
    }

    /// The elements of the arrays of the unnamed class: an array no key names, and every array that may reach code the
    /// solve does not follow. An element read of an array with no origin joins them.
    pub(super) fn unnamed_elements(&self) -> Elements {
        self.stored_elements(ElementClass::Unnamed)
    }

    /// The element classes of `class`'s class (`ElementClasses::members`): a read of the class's elements depends on each.
    pub(super) fn element_members(&self, class: ElementClass) -> impl Iterator<Item = ElementClass> + '_ {
        self.elements.members(class)
    }

    /// Per element class the solve names (each class a store writes, each class a link joins, and the unnamed class), the
    /// keys of every class of its class: a store through any array of the class moves all of them (`AotFacts::element_class_keys`).
    pub(super) fn element_classes(&self) -> BTreeMap<FactKey, Box<[FactKey]>> {
        // Every linked class is listed, cell or not: two classes a link joins are one class, and `element_classes_distinct`
        // reads the list to tell that two classes are apart.
        let named = self.cells.keys().filter_map(|key| match key { Key::Element(class) => Some(*class), _ => None });
        named.chain(self.elements.linked()).chain(std::iter::once(ElementClass::Unnamed))
            .map(|class| (FactKey::Elements(class), self.elements.members(class).map(FactKey::Elements).collect::<Box<[FactKey]>>()))
            .collect()
    }

    /// Widens the integer ranges of the fields, elements, returns and bindings `self` found against the pass before
    /// (`previous`), so the solve settles (`TYPE_PASSES`). A key whose range still grows after `WIDENING_ROUNDS` passes
    /// widens to the nearest threshold beyond its new bound (`typing::widen`, over the key's `thresholds`), so a field
    /// that counts up to a constant it is stored or compared against stays at that constant. Where no threshold lies
    /// beyond, the widening settles at int32's span, then at `IntRange::WORD`. A range `WORD` does not cover is a join
    /// of integers, a NaN-free Number. `grown` counts, per key, the passes its range grew in.
    /// A pass that changes nothing widens nothing, so the least fixpoint is unchanged.
    pub(super) fn widen(mut self, previous: &Types, grown: &mut BTreeMap<Key, u8>) -> Self {
        for (key, cell) in self.cells.iter_mut() {
            let Some(before) = previous.cells.get(key).copied() else { continue };
            let (Some(old), Some(new)) = (before.ty.int_range(), cell.ty.int_range()) else { continue };
            if old.contains(new) { continue; }
            let rounds = grown.entry(*key).or_insert(0);
            *rounds = rounds.saturating_add(1);
            if *rounds < WIDENING_ROUNDS { continue; }
            // An element class's stores are read-modify-writes (`+=`, `++`, `|=`) that grow its range each pass, so it also
            // widens to the power-of-two bounds of its new range (`typing::power_thresholds`) before the word.
            let mut bounds = self.thresholds.get(key).cloned().unwrap_or_default();
            if matches!(key, Key::Element(_)) {
                bounds.extend(typing::power_thresholds(new));
                bounds.sort_unstable();
                bounds.dedup();
            }
            let widened = typing::widen(&bounds, before.ty, cell.ty);
            cell.ty = match widened.int_range() {
                Some(range) if range.contains(new) => widened,
                _ => Type::NOT_NAN_NUMBER,
            };
        }
        self
    }

    /// Every settled cell by its key in the one lattice's key space (`typed_ir::FactKey`): a class's own field
    /// joined with the stores into objects of no one class (`cell_on`), every other key as solved, each
    /// parameter and receiver. The name-wide `Key::Member` join is no key of its own: `fields` gives it.
    pub(super) fn fact_cells(&self) -> impl Iterator<Item = (FactKey, Cell)> + '_ {
        let keyed = self.cells.keys().filter_map(|key| Some(match *key {
            Key::MemberOn { class, instance, atom } => (FactKey::Field { class, instance, atom }, self.cell_on(class, instance, atom)?),
            Key::MemberAny(atom) => (FactKey::AnyField(atom), self.cell(*key)),
            Key::Return(function) => (FactKey::Return(function), self.cell(*key)),
            Key::Global(atom) => (FactKey::Global(atom), self.cell(*key)),
            Key::Slot { owner, slot } => (FactKey::Slot { owner, slot }, self.cell(*key)),
            // The unnamed class is emitted below, whether or not a store wrote it.
            Key::Element(ElementClass::Unnamed) => return None,
            Key::Element(class) => (FactKey::Elements(class), elements_cell(self.stored_elements(class))),
            Key::Member(_) => return None,
        }));
        // The unnamed class is a key of its own whether or not a store wrote it, so a subscriber always has a cell.
        let unnamed = std::iter::once((FactKey::Elements(ElementClass::Unnamed), elements_cell(self.unnamed_elements())));
        let params = self.params.iter().flat_map(|(function, cells)| cells.iter().enumerate()
            .map(move |(index, cell)| (FactKey::Parameter { function: *function, index: index as u16 }, cell.guessed())));
        let receivers = self.receivers.iter().map(|(function, cell)| (FactKey::Receiver(*function), cell.guessed()));
        keyed.chain(unnamed).chain(params).chain(receivers)
    }

    /// The type of every field name the program stores (`Key::Member`), for the lowering.
    pub(super) fn fields(&self) -> impl Iterator<Item = (u32, Type)> + '_ {
        self.cells.iter().filter_map(|(key, cell)| match *key { Key::Member(atom) => Some((atom, cell.ty)), _ => None })
    }

    /// The classes the solve found, for the trace.
    pub(super) fn class_trace(&self) -> Vec<(Key, Classes)> {
        self.cells.iter().filter(|(_, cell)| cell.classes != Classes::Many).map(|(key, cell)| (*key, cell.classes))
            .chain(self.receivers.iter().map(|(function, cell)| (Key::Slot { owner: *function, slot: u16::MAX }, cell.classes))).collect()
    }

    /// The value the call at `pc` of `function` returns: the cell of the returns it may reach, read as the first
    /// callee's `Return` (`call_returns`), so an array it returns is in that callee's element class. None where its
    /// callees are not known.
    pub(super) fn call(&self, function: u32, pc: u32) -> Option<Word> {
        let cell = self.calls.get(&(function, pc))?.guessed();
        let origin = self.call_returns.get(&(function, pc)).map(|&callee| Origin::Binding(Key::Return(callee)));
        Some(Word { cell, origin })
    }

    /// The class whose own instance field `atom` an instance of `class` reads. That is `class` where it holds the field, or
    /// where no holder is proven; else the first class up its `extends` chain (`parents`) that holds it (its own `MemberOn`
    /// cell), when no class on the way may shadow it. A key the world can move (`World::live`) keeps the read on `class`: a
    /// field that class gains later would shadow the holder. A name some store writes on an object of no one class, or that
    /// a store the analysis cannot name may write, is never folded.
    pub(super) fn holder(&self, class: u32, atom: u32) -> u32 {
        if self.shadows.writes_any(atom) { return class; }
        let mut current = class;
        for _ in 0..=self.parents.len() {
            if self.world.live(FactKey::Field { class: current, instance: true, atom }) { return class; }
            if self.cells.contains_key(&Key::MemberOn { class: current, instance: true, atom }) { return current; }
            if self.shadows.writes_class(current, atom) { return class; }
            match self.parents.get(&current) {
                Some(&parent) => current = parent,
                None => return class,
            }
        }
        class
    }

    /// The String builtins a call's result is typed by (`strings`).
    pub(super) fn strings(&self) -> &StringBuiltins {
        &self.strings
    }

    /// The cell of `this` in `function`: the join of the receivers its calls pass, any value where no
    /// recorded call reaches it.
    /// Whether a constructor's returns may be the global object (`Key::Return`): a `new` of it then yields the global
    /// object, not only the instance. A constructor no write of returns holds nothing to return.
    pub(super) fn returns_global(&self, function: u32) -> bool {
        self.cells.get(&Key::Return(function)).is_some_and(|cell| cell.global)
    }

    /// The cell of `this` in `function`: the join of the receivers its calls pass, the global object among them where a plain
    /// call or unknown code may bind a sloppy function's `this` to it (`escapes::Escapes::plain`). Any value where no recorded
    /// call reaches it: a method a runtime conversion calls is called with its object, not the global object.
    pub(super) fn this(&self, function: u32) -> Cell {
        self.receivers.get(&function).copied().unwrap_or(Cell::ANY).guessed()
    }

    /// The parameter types of every function they type, for the solve's trace.
    /// How many parameters of `function` the solve types (`parameters`): none for one whose calls do not bind them.
    pub(super) fn param_count(&self, function: u32) -> usize {
        self.params.get(&function).map_or(0, Vec::len)
    }

    pub(super) fn parameter_types(&self) -> BTreeMap<u32, Vec<Type>> {
        self.params.iter().map(|(function, cells)| (*function, cells.iter().map(|cell| cell.ty).collect())).collect()
    }

    /// The cell parameter `index` of `function` takes at entry: the join of the arguments its calls
    /// pass (`parameters`); any value where the function's parameters are not typed.
    pub(super) fn param(&self, function: u32, index: u16) -> Cell {
        self.params.get(&function).and_then(|cells| cells.get(usize::from(index))).copied().unwrap_or(Cell::ANY).guessed()
    }
}

/// Per call that may reach known callees: the join of the returns they may make (`calls`), and the first callee (`call_returns`).
/// The callees' returns are one result, so their element classes join in `links`.
fn call_results(
    flows: &[FunctionFlow],
    facts: &AotFacts,
    census: &Census,
    returns: &BTreeMap<Key, Cell>,
    links: &mut Vec<(ElementClass, ElementClass)>,
) -> (BTreeMap<(u32, u32), Cell>, BTreeMap<(u32, u32), u32>) {
    let (mut cells, mut firsts) = (BTreeMap::new(), BTreeMap::new());
    for (id, flow) in flows.iter().enumerate() {
        let id = id as u32;
        for call in flow.calls.iter().filter(|call| !call.construct) {
            let callees = targets(id, call, facts, census);
            let Some((&first, rest)) = callees.split_first() else { continue };
            links.extend(rest.iter().map(|&callee| (ElementClass::Return(callee), ElementClass::Return(first))));
            // A callee no return of reaches never returns: it adds nothing to the join.
            let cell = callees.iter().fold(Cell::NEVER, |cell, callee| cell.join(returns.get(&Key::Return(*callee)).copied().unwrap_or(Cell::NEVER)));
            cells.insert((id, call.pc), cell);
            firsts.insert((id, call.pc), first);
        }
    }
    (cells, firsts)
}

/// The cell of an element class's elements: their type and classes, and the length they leave (`Elements`).
fn elements_cell(elements: Elements) -> Cell {
    Cell { ty: elements.ty, classes: elements.classes, elements, sites: Sites::of_type(elements.ty), global: false, prototype: true, identity: Identity::of_type(elements.ty) }
}

/// Whether `function` is sloppy code: a plain call may bind its `this` to the global object (`Function::strict` is unset). A
/// function with no flow is taken as sloppy.
fn sloppy(flows: &[FunctionFlow], function: u32) -> bool {
    flows.get(function as usize).is_none_or(|flow| !flow.strict)
}

/// The functions one class alone holds as a method or an own field (`Key::MemberOn`), with that class.
fn holders(cells: &BTreeMap<Key, Cell>) -> BTreeMap<u32, u32> {
    let mut holders: BTreeMap<u32, Option<u32>> = BTreeMap::new();
    for (key, cell) in cells {
        let (Key::MemberOn { class, instance: true, .. }, Some(function)) = (*key, cell.ty.body_index()) else { continue };
        let holder = holders.entry(function).or_insert(Some(class));
        if *holder != Some(class) { *holder = None; }
    }
    holders.into_iter().filter_map(|(function, class)| Some((function, class?))).collect()
}

/// The `this` call `call` passes. A method call (`o.m()`, its callee read off its receiver) passes an object:
/// the read of `m` off `null` or `undefined` throws before the call, and a method the closed world stores on a
/// class's prototype is found on an object's chain. A primitive receiver reaches such a method only where a
/// builtin prototype's chain was rewritten, a write the analysis does not see: the guess is held under the
/// slot dependencies, as every type here is.
fn receiver_type(call: &super::flow::Call) -> Type {
    use super::flow::Target;
    let method = matches!(call.target, Target::Method(_))
        || matches!(call.callee.origin, Some(Origin::Member(_) | Origin::MemberOn { .. })) && !call.construct;
    if !method || call.receiver.cell.ty == Type::NEVER { return call.receiver.cell.ty; }
    let ty = call.receiver.cell.ty;
    if ty.is_subtype(Type::DIRECT_OBJECT_FAMILY) { ty } else { Type::DIRECT_OBJECT_FAMILY }
}

/// The functions call `call` of `function` may reach: a register callee's one target or closed set
/// (`AotFacts::call_targets`) or construction target, a known function, or the closures stored under a
/// method call's name (`Census::method_closures`). None where the closed world names none.
pub(super) fn targets(function: u32, call: &super::flow::Call, facts: &AotFacts, census: &Census) -> Vec<u32> {
    use super::flow::Target;
    let target_facts = facts.function_facts(function);
    match call.target {
        Target::Register if call.construct => target_facts.construct_target(call.pc).into_iter().collect(),
        Target::Register => {
            let proven = target_facts.call_targets(call.pc);
            if !proven.is_empty() { return proven; }
            // A method read the census resolves no closure for (`o.m` off an instance, whose class
            // stores its methods by plain field stores): the closures stored under its name.
            match call.callee.origin {
                Some(Origin::Member(atom) | Origin::MemberOn { atom, .. }) => census.method_closures(atom).map(|set| set.iter().copied().collect()).unwrap_or_default(),
                _ => Vec::new(),
            }
        }
        Target::Known(callee) => vec![callee],
        Target::Method(atom) => census.method_closures(atom).map(|set| set.iter().copied().collect()).unwrap_or_default(),
        Target::Accessor { atom, setter } => super::accessors::accessor_arms(call, atom, setter, census).into_iter().map(|arm| arm.function).collect(),
    }
}

/// The links a callee's entry makes (`ElementClass::Parameter`, `Receiver`): a resolved call joins each argument's class to
/// the parameter it binds, and its receiver's class to the callee's `this`, so the callee's stores into them reach the
/// caller's arrays. A callee an unresolved call may reach, an escaped one, and one no recorded call reaches are entered with
/// values the solve does not see: their entry classes join the unnamed class, as the arguments of an escape do (`escapes`).
fn entry_links(flows: &[FunctionFlow], facts: &AotFacts, census: &Census, escaped: &BTreeSet<u32>, links: &mut Vec<(ElementClass, ElementClass)>) {
    let mut reached = BTreeSet::new();
    let mut open = escaped.clone();
    for (id, flow) in flows.iter().enumerate() {
        for call in &flow.calls {
            let callees = targets(id as u32, call, facts, census);
            reached.extend(callees.iter().copied());
            if !resolved(call, &callees, census) {
                open.extend(callees);
                continue;
            }
            for &callee in &callees {
                links.extend(call.receiver.link(ElementClass::Receiver(callee)));
                for (index, argument) in call.arguments.iter().enumerate() {
                    links.extend(argument.link(entry_class(flows, callee, index)));
                }
            }
        }
    }
    for (id, flow) in flows.iter().enumerate() {
        let id = id as u32;
        if open.contains(&id) || !reached.contains(&id) {
            links.push((ElementClass::Receiver(id), ElementClass::Unnamed));
            links.extend((0..flow.in_register.len()).map(|index| (ElementClass::Parameter { function: id, index: index as u16 }, ElementClass::Unnamed)));
        }
    }
}

/// The class argument `index` of a call to `callee` joins: the parameter's own where the callee's entry state holds the
/// argument in a register (`FunctionFlow::in_register`), else the unnamed class, which the callee reads the argument back through.
fn entry_class(flows: &[FunctionFlow], callee: u32, index: usize) -> ElementClass {
    let in_register = flows.get(callee as usize).and_then(|flow| flow.in_register.get(index)).copied().unwrap_or(false);
    if in_register { ElementClass::Parameter { function: callee, index: index as u16 } } else { ElementClass::Unnamed }
}

/// The parameter types of every function whose parameters bind its arguments one to one
/// (`FunctionFlow::arity`). Parameter `i` joins argument `i` of every recorded call the closed world
/// proves or guesses may reach the function: the call's one target, its closed method set, or the
/// construction's target. A call that passes no `i`th argument passes `undefined` there, and a function
/// no recorded call reaches takes any value throughout. `Types::of` takes any value for an escaped function
/// (`escapes`), since a call the recorded calls do not show (a native's callback, an unresolved callee) may
/// pass it anything. A call the analysis does not see may pass anything: the types are a guess, as the
/// field types they feed are, which a tier holds only under the fields' slot dependencies.
fn parameters(flows: &[FunctionFlow], facts: &AotFacts, census: &Census) -> BTreeMap<u32, Vec<Cell>> {
    let undefined = Cell::of(Type::language(ValueType::Undefined));
    let mut params: BTreeMap<u32, Vec<Cell>> = flows.iter().enumerate()
        .filter_map(|(id, flow)| Some((id as u32, vec![Cell::NEVER; usize::from(flow.arity?)])))
        .collect();
    let mut reached = std::collections::BTreeSet::new();
    for (id, flow) in flows.iter().enumerate() {
        for call in &flow.calls {
            for target in targets(id as u32, call, facts, census) {
                let Some(slots) = params.get_mut(&target) else { continue };
                reached.insert(target);
                for (index, slot) in slots.iter_mut().enumerate() {
                    *slot = slot.join(call.arguments.get(index).map_or(undefined, |argument| argument.cell));
                }
            }
        }
    }
    // The parameters of a function no recorded call reaches are ones the closed world does not see bound: any
    // value. One a call reaches keeps `NEVER` while no value reaches its arguments yet (the solve's bottom).
    for (function, slots) in &mut params {
        if !reached.contains(function) { slots.iter_mut().for_each(|slot| *slot = Cell::ANY); }
    }
    params
}
