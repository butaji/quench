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

use super::census::Census;
use super::flow::FunctionFlow;
use super::value::{Key, Origin, UNNAMED_ARRAYS};
use crate::typed_ir::{Cell, Classes, Elements, WIDENING_ROUNDS, language_bit};
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
    /// Per function, the join of the arguments each parameter takes (`parameters`).
    params: BTreeMap<u32, Vec<Cell>>,
    /// Per function, the join of the `this` its calls pass (`Call::receiver`).
    receivers: BTreeMap<u32, Cell>,
    /// The cell of a key no write of has been seen yet: none (`Cell::NEVER`) in the solve's first pass
    /// (`bottom`), any value once the passes stop.
    unseen: Cell,
}

impl Types {
    /// The cells `flows` store and return, with each call typed by the callees `facts` prove or
    /// guess for it, the parameters of each function, and the bindings.
    pub(super) fn of(flows: &[FunctionFlow], facts: &AotFacts, census: &Census) -> Self {
        let mut types = Self { cells: BTreeMap::new(), calls: BTreeMap::new(), params: BTreeMap::new(), receivers: BTreeMap::new(), unseen: Cell::ANY };
        for &(key, fact) in flows.iter().flat_map(|flow| &flow.writes) {
            let joined = types.cells.entry(key).or_insert(Cell::NEVER);
            *joined = joined.join(fact.cell);
        }
        for (id, flow) in flows.iter().enumerate() {
            let id = id as u32;
            for call in flow.calls.iter().filter(|call| !call.construct) {
                let callees = targets(id, call, facts, census);
                if callees.is_empty() { continue; }
                // A callee no return of reaches never returns: it adds nothing to the join.
                let cell = callees.iter().fold(Cell::NEVER, |cell, callee| cell.join(types.cells.get(&Key::Return(*callee)).copied().unwrap_or(Cell::NEVER)));
                types.calls.insert((id, call.pc), cell);
            }
        }
        types.params = parameters(flows, facts, census);
        // An escaped function (`escapes`) may be called by code the solve cannot follow, with any `this` and arguments:
        // its parameters are any value, as are its `this` (below), whatever its recorded calls pass.
        let escaped = escapes(flows, facts, census);
        for function in &escaped {
            if let Some(slots) = types.params.get_mut(function) { slots.iter_mut().for_each(|slot| *slot = Cell::ANY); }
        }
        for (id, flow) in flows.iter().enumerate() {
            for call in &flow.calls {
                let receiver = Cell { ty: receiver_type(call), ..call.receiver.cell };
                for target in targets(id as u32, call, facts, census) {
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
            types.receivers.insert(*function, Cell::ANY);
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

    /// `cell` with the elements the stores into field `atom`'s arrays made (`Key::Element(atom)`): their type and
    /// classes, and the length, which an edge that may change it made `Length::Any`. A field's cell so names what
    /// its arrays hold, whichever store put the array there.
    fn with_elements(&self, atom: u32, cell: Cell) -> Cell {
        Cell { elements: cell.elements.join(self.stored_elements(Key::Element(atom))), ..cell }
    }

    /// The elements the writes of `key` (`Key::Element`) name: the type and classes of the values stored, and the
    /// length they leave (`Elements::length`), none where no such write is seen.
    fn stored_elements(&self, key: Key) -> Elements {
        let stored = self.cells.get(&key).copied().unwrap_or(Cell::NEVER);
        Elements { ty: stored.ty, classes: stored.classes, length: stored.elements.length }
    }

    /// The elements every array may hold that no field names (`UNNAMED_ARRAYS`): an element read joins them, since
    /// an alias may store through an array the field read does not name.
    pub(super) fn unnamed_elements(&self) -> Elements {
        self.stored_elements(Key::Element(UNNAMED_ARRAYS))
    }

    /// Widens the integer ranges `self` found against the pass before (`previous`), so the solve settles
    /// (`TYPE_PASSES`). A key whose range still grows after `WIDENING_ROUNDS` passes takes the widened range
    /// (`Type::widen`: the int32 span, then `IntRange::WORD`); a range the widening does not cover drops to
    /// `Type::NUMBER`, which holds every Number. `grown` counts, per key, the passes its range grew in.
    /// A pass that changes nothing widens nothing, so the least fixpoint is unchanged.
    pub(super) fn widen(mut self, previous: &Types, grown: &mut BTreeMap<Key, u8>) -> Self {
        for (key, cell) in self.cells.iter_mut() {
            let Some(before) = previous.cells.get(key).copied() else { continue };
            let (Some(old), Some(new)) = (before.ty.int_range(), cell.ty.int_range()) else { continue };
            if old.contains(new) { continue; }
            let rounds = grown.entry(*key).or_insert(0);
            *rounds = rounds.saturating_add(1);
            if *rounds < WIDENING_ROUNDS { continue; }
            let widened = before.ty.widen(cell.ty);
            cell.ty = match widened.int_range() {
                Some(range) if range.contains(new) => widened,
                _ => Type::NUMBER,
            };
        }
        self
    }

    /// Every settled cell by its key in the one lattice's key space (`typed_ir::FactKey`): a class's own field
    /// joined with the stores into objects of no one class (`cell_on`), every other key as solved, each
    /// parameter and receiver. The name-wide `Key::Member` join is no key of its own: `fields` gives it.
    pub(super) fn fact_cells(&self) -> impl Iterator<Item = (crate::typed_ir::FactKey, Cell)> + '_ {
        use crate::typed_ir::FactKey;
        let keyed = self.cells.keys().filter_map(|key| Some(match *key {
            Key::MemberOn { class, instance, atom } => (FactKey::Field { class, instance, atom }, self.cell_on(class, instance, atom)?),
            Key::MemberAny(atom) => (FactKey::AnyField(atom), self.cell(*key)),
            Key::Return(function) => (FactKey::Return(function), self.cell(*key)),
            Key::Global(atom) => (FactKey::Global(atom), self.cell(*key)),
            Key::Slot { owner, slot } => (FactKey::Slot { owner, slot }, self.cell(*key)),
            Key::Element(UNNAMED_ARRAYS) => return None,
            Key::Element(atom) => {
                let elements = self.with_elements(atom, Cell::NEVER).elements;
                (FactKey::Elements(atom), Cell { ty: elements.ty, classes: elements.classes, elements })
            }
            Key::Member(_) => return None,
        }));
        let params = self.params.iter().flat_map(|(function, cells)| cells.iter().enumerate()
            .map(move |(index, cell)| (FactKey::Parameter { function: *function, index: index as u16 }, cell.guessed())));
        let receivers = self.receivers.iter().map(|(function, cell)| (FactKey::Receiver(*function), cell.guessed()));
        keyed.chain(params).chain(receivers)
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

    /// The cell the call at `pc` of `function` returns: none where its callees are not known.
    pub(super) fn call(&self, function: u32, pc: u32) -> Option<Cell> {
        self.calls.get(&(function, pc)).map(|cell| cell.guessed())
    }

    /// The cell of `this` in `function`: the join of the receivers its calls pass, any value where no
    /// recorded call reaches it.
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
fn targets(function: u32, call: &super::flow::Call, facts: &AotFacts, census: &Census) -> Vec<u32> {
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
    }
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

/// The functions whose closure may reach a position the solve cannot follow, so that code it does not see may call
/// them with any `this` and arguments (`Types::of` gives an escaped function `Cell::ANY` for its `this` and every
/// parameter). A closure escapes when a sink may hold it:
/// - the callee, `this` or an argument of a recorded call the solve does not resolve (`resolved`): the call may run a
///   native or an unseen function, which the recorded arguments never reach;
/// - a value thrown, or a register live at a `yield` or `await` (`FunctionFlow::handed`): a consumer or a reaction runs it;
/// - a store into an element (`Key::Element`), or into a global, which the host may read by name;
/// - a return of an escaped function, of the root, or of an async or generator function (`FunctionFlow::returns_out`);
/// - a store under a named property (`Key::Member`, `MemberOn`, `MemberAny`) that new code can move (`Census::live`), or
///   any store while an object reaches a sink (`object_reader`): unresolved code may read any named property of it.
/// A sink holds a closure by its cell (`mark`): the function its type names, else, where its identity is lost, every
/// materialized closure. A value passed on to a resolved callee reaches the callee's own cells, which its sinks read.
/// The rule is a fixpoint: an escaped function's return is a sink, which may make an object reader, which escapes
/// the named stores. Known gap: a method read's call is resolved by the census's closed set, so a builtin receiver whose
/// method the program also defines (the record's guard falls back to the VM) gets arguments the solve never saw.
fn escapes(flows: &[FunctionFlow], facts: &AotFacts, census: &Census) -> BTreeSet<u32> {
    use super::flow::Target;
    let materialized: BTreeSet<u32> = flows.iter().flat_map(|flow| flow.closures.iter().copied()).collect();
    let roots: BTreeSet<u32> = flows.iter().enumerate().filter(|(_, flow)| flow.returns_out).map(|(id, _)| id as u32).collect();
    let mut sinks: Vec<Cell> = Vec::new();
    for (id, flow) in flows.iter().enumerate() {
        for call in &flow.calls {
            if resolved(call, &targets(id as u32, call, facts, census), census) { continue; }
            if call.target == Target::Register { sinks.push(call.callee.cell); }
            sinks.push(call.receiver.cell);
            sinks.extend(call.arguments.iter().map(|argument| argument.cell));
        }
        sinks.extend(flow.handed.iter().map(|word| word.cell));
    }
    let writes: Vec<(Key, Cell)> = flows.iter().flat_map(|flow| &flow.writes).map(|&(key, word)| (key, word.cell)).collect();
    let mut escaped = BTreeSet::new();
    loop {
        let mut held = sinks.clone();
        let mut object_reader = sinks.iter().any(|cell| may_be_object(*cell));
        let mut named = Vec::new();
        for &(key, cell) in &writes {
            match key {
                Key::Element(_) => held.push(cell),
                Key::Global(_) => { object_reader |= may_be_object(cell); held.push(cell); }
                Key::Return(function) if escaped.contains(&function) || roots.contains(&function) => { object_reader |= may_be_object(cell); held.push(cell); }
                Key::Member(_) | Key::MemberOn { .. } | Key::MemberAny(_) => named.push((key, cell)),
                Key::Return(_) | Key::Slot { .. } => {}
            }
        }
        held.extend(named.into_iter()
            .filter(|(key, _)| object_reader || census.live(super::census::fact_key(*key)))
            .map(|(_, cell)| cell));
        let mut next = escaped.clone();
        for cell in &held { mark(cell, &materialized, &mut next); }
        if next == escaped { return escaped; }
        escaped = next;
    }
}

/// Whether a recorded call resolves: its targets are known functions (`targets` is not empty) and the callee is not a guess
/// (`Census::guessed`: a read from a key new code can move, or a method read the census only guesses). Only a resolved call's
/// arguments reach the callees' parameters (`parameters`), so an unresolved call's arguments escape instead (`escapes`).
fn resolved(call: &super::flow::Call, callees: &[u32], census: &Census) -> bool {
    !callees.is_empty() && !census.guessed(call.callee)
}

/// Adds to `escaped` the functions a cell may hold a closure of: its type's identity and its elements' type's identity. A
/// type with a function alternative but no body (identity lost: a join of two closures, or `Type::ANY`) may hold any
/// materialized closure, so each of them escapes.
fn mark(cell: &Cell, materialized: &BTreeSet<u32>, escaped: &mut BTreeSet<u32>) {
    for ty in [cell.ty, cell.elements.ty] {
        if let Some(function) = ty.body_index() {
            escaped.insert(function);
        } else if ty.may_be_languages(Type::language(ValueType::Function)) {
            escaped.extend(materialized.iter().copied());
        }
    }
}

/// Whether a cell may hold an object: an object, a function or an instance of a class. Code the solve does not see may read
/// any named property of such a value, so a store under a named property escapes wherever one reaches a sink (`escapes`).
fn may_be_object(cell: Cell) -> bool {
    let objects = Type::languages(language_bit(ValueType::Object) | language_bit(ValueType::Function));
    cell.ty.may_be_languages(objects) || cell.classes != Classes::Never
}
