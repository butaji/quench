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
use super::value::{Class, Key, Origin};
use super::AotFacts;
use crate::typed_ir::Type;
use crate::ValueType;
use std::collections::BTreeMap;

/// The most passes the solve takes. Each pass is one whole-program flow over the types the one before found,
/// and the types only climb, each through a lattice of bounded height, so the solve settles; a chain of fields
/// that feed one another (richards' task queue: `blocks` → `currentTcb` → `link` → `queue`) climbs one link a pass.
pub(super) const TYPE_PASSES: usize = 64;

/// The types one pass found, which the next pass reads.
#[derive(PartialEq, Eq)]
pub(crate) struct Types {
    /// Per field name, the join of the values every store of it writes.
    pub(crate) fields: BTreeMap<u32, Type>,
    /// Per function, the join of the values its returns return.
    returns: BTreeMap<u32, Type>,
    /// Per call `(function, pc)`, the join of the returns of every function it may call.
    calls: BTreeMap<(u32, u32), Type>,
    /// Per function, the join of the arguments each parameter takes (`parameters`).
    params: BTreeMap<u32, Vec<Type>>,
    /// Per function, the join of the `this` its calls pass (`Call::receiver`).
    receivers: BTreeMap<u32, Type>,
    /// Per global or slot binding, the join of the values its writes store. A write the analysis does
    /// not see (sloppy-mode global code, a builtin writing through the global object) may store anything:
    /// the binding types only feed the field types, a guess held under the fields' slot dependencies.
    bindings: BTreeMap<Key, Type>,
    /// The type of a field, binding or return no write of has been seen yet: none (`Type::NEVER`) in the
    /// solve's first pass (`bottom`), any value once the passes stop.
    unseen: Type,
    /// Per field, return, element or binding, the class of the objects its writes store (`Class`), joined
    /// beside its type: what a read of it holds, as a guess (`Class::guessed`).
    classes: BTreeMap<Key, Class>,
    /// Per call `(function, pc)`, the join of the classes of every function it may call's returns.
    call_classes: BTreeMap<(u32, u32), Class>,
    /// The class of a key no write of has been seen yet, as `unseen` is its type.
    unseen_class: Class,
    /// Per function, the join of the classes of the `this` its calls pass, beside `receivers`.
    receiver_classes: BTreeMap<u32, Class>,
    /// Per function, the join of the classes of each parameter's arguments, beside `params`.
    param_classes: BTreeMap<u32, Vec<Class>>,
}

impl Types {
    /// The types `flows` store and return, with each call typed by the callees `facts` prove or
    /// guess for it, the parameters of each function, and the bindings.
    pub(super) fn of(flows: &[FunctionFlow], facts: &AotFacts, census: &Census) -> Self {
        let mut types = Self { fields: BTreeMap::new(), returns: BTreeMap::new(), calls: BTreeMap::new(), params: BTreeMap::new(), receivers: BTreeMap::new(),
            bindings: BTreeMap::new(), unseen: Type::ANY, classes: BTreeMap::new(), call_classes: BTreeMap::new(), unseen_class: Class::Many,
            receiver_classes: BTreeMap::new(), param_classes: BTreeMap::new() };
        static PASS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let pass = PASS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        for (fid, flow) in flows.iter().enumerate() {
            for &(key, fact) in &flow.writes {
                if matches!(key, Key::Member(_) | Key::MemberOn { .. } | Key::Return(_) | Key::Global(_) | Key::Slot { .. } | Key::Element(_)) {
                    let class = types.classes.entry(key).or_insert(Class::Never);
                    if std::env::var_os("QUENCH_CLASS_DEBUG").is_some() && *class != Class::Many && class.join(fact.class) == Class::Many { eprintln!("CLASSDBG pass {pass} f{fid} {key:?} {:?} <- {fact:?}", *class); }
                    *class = class.join(fact.class);
                }
                let joined = match key {
                    Key::Member(atom) => types.fields.entry(atom).or_insert(Type::NEVER),
                    Key::Return(function) => types.returns.entry(function).or_insert(Type::NEVER),
                    Key::Global(_) | Key::Slot { .. } | Key::Element(_) => types.bindings.entry(key).or_insert(Type::NEVER),
                    _ => continue,
                };
                *joined = joined.join(fact.ty);
            }
        }
        for (id, flow) in flows.iter().enumerate() {
            let id = id as u32;
            for call in flow.calls.iter().filter(|call| !call.construct) {
                let callees = targets(id, call, facts, census);
                if callees.is_empty() { continue; }
                // A callee no return of reaches never returns: it adds nothing to the join.
                let ty = callees.iter().fold(Type::NEVER, |ty, callee| ty.join(types.returns.get(callee).copied().unwrap_or(Type::NEVER)));
                types.calls.insert((id, call.pc), ty);
                let class = callees.iter().fold(Class::Never, |class, callee| class.join(types.classes.get(&Key::Return(*callee)).copied().unwrap_or(Class::Never)));
                types.call_classes.insert((id, call.pc), class);
            }
        }
        (types.params, types.param_classes) = parameters(flows, facts, census);
        for (id, flow) in flows.iter().enumerate() {
            for call in &flow.calls {
                let receiver = receiver_type(call);
                for target in targets(id as u32, call, facts, census) {
                    let joined = types.receivers.entry(target).or_insert(Type::NEVER);
                    *joined = joined.join(receiver);
                    let class = types.receiver_classes.entry(target).or_insert(Class::Never);
                    *class = class.join(call.receiver.class);
                }
            }
        }
        types
    }

    /// The solve's first guess (`aot_facts::analyze_as`): every type these types name, none of it yet.
    /// The passes from it climb to the least solution, which a self-dependent field (`this.n = this.n | 1`)
    /// reaches where passes from the top stay at any value; a solve that stops short of a fixpoint keeps
    /// no type.
    pub(super) fn bottom(mut self) -> Self {
        let never = |ty: &mut Type| *ty = Type::NEVER;
        self.fields.values_mut().for_each(never);
        self.returns.values_mut().for_each(never);
        self.calls.values_mut().for_each(never);
        self.bindings.values_mut().for_each(never);
        self.receivers.values_mut().for_each(never);
        self.params.values_mut().flatten().for_each(|ty| *ty = Type::NEVER);
        self.unseen = Type::NEVER;
        self.classes.values_mut().for_each(|class| *class = Class::Never);
        self.call_classes.values_mut().for_each(|class| *class = Class::Never);
        self.unseen_class = Class::Never;
        self.receiver_classes.values_mut().for_each(|class| *class = Class::Never);
        self.param_classes.values_mut().flatten().for_each(|class| *class = Class::Never);
        self
    }

    /// The class of `this` in `function`, a guess: the join of its receivers' (`this`), any where no
    /// recorded call reaches it.
    pub(super) fn this_class(&self, function: u32) -> Class {
        self.receiver_classes.get(&function).copied().unwrap_or(Class::Many).guessed()
    }

    /// The class parameter `index` of `function` takes at entry, a guess (`param`).
    pub(super) fn param_class(&self, function: u32, index: u16) -> Class {
        self.param_classes.get(&function).and_then(|classes| classes.get(usize::from(index))).copied().unwrap_or(Class::Many).guessed()
    }

    /// The class a read of `key` holds (a field, return, element or binding): the join of its writes', a
    /// guess; `unseen_class` where none is seen.
    pub(super) fn class(&self, key: Key) -> Class {
        self.classes.get(&key).copied().unwrap_or(self.unseen_class).guessed()
    }

    /// The classes the solve found, for the trace.
    pub(super) fn class_trace(&self) -> Vec<(Key, Class)> {
        self.classes.iter().filter(|(_, class)| **class != Class::Many).map(|(key, class)| (*key, *class))
            .chain(self.receiver_classes.iter().map(|(function, class)| (Key::Slot { owner: *function, slot: u16::MAX }, *class))).collect()
    }

    /// The class a read of field `atom` off class `class`'s instances (`instance`) or static object holds, a
    /// guess: the join of that class's stores of it (`Key::MemberOn`), none where none is seen.
    pub(super) fn class_on(&self, class: u32, instance: bool, atom: u32) -> Option<Class> {
        self.classes.get(&Key::MemberOn { class, instance, atom }).map(|class| class.guessed())
    }

    /// The class the call at `pc` of `function` returns, a guess: any where its callees are not known.
    pub(super) fn call_class(&self, function: u32, pc: u32) -> Class {
        self.call_classes.get(&(function, pc)).copied().unwrap_or(Class::Many).guessed()
    }

    /// The type a read of field `atom` takes: the join of its stores, `unseen` where none is seen.
    pub(super) fn field(&self, atom: u32) -> Type {
        self.fields.get(&atom).copied().unwrap_or(self.unseen)
    }

    /// The type the call at `pc` of `function` returns: any value where its callees are not known.
    pub(super) fn call(&self, function: u32, pc: u32) -> Option<Type> {
        self.calls.get(&(function, pc)).copied()
    }

    /// The type a load of binding `key` takes: the join of its writes, `unseen` where none is seen.
    pub(super) fn binding(&self, key: Key) -> Type {
        self.bindings.get(&key).copied().unwrap_or(self.unseen)
    }

    /// The type of `this` in `function`: the join of the receivers its calls pass, any value where no
    /// recorded call reaches it.
    pub(super) fn this(&self, function: u32) -> Type {
        self.receivers.get(&function).copied().unwrap_or(Type::ANY)
    }

    /// The parameter types of every function they type, for the solve's trace.
    pub(super) fn parameter_types(&self) -> &BTreeMap<u32, Vec<Type>> {
        &self.params
    }

    /// The type parameter `index` of `function` takes at entry: the join of the arguments its calls
    /// pass (`parameters`); any value where the function's parameters are not typed.
    pub(super) fn param(&self, function: u32, index: u16) -> Type {
        self.params.get(&function).and_then(|types| types.get(usize::from(index))).copied().unwrap_or(Type::ANY)
    }
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
    if !method || call.receiver.ty == Type::NEVER { return call.receiver.ty; }
    let ty = call.receiver.ty;
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
/// no recorded call reaches takes any value throughout. A call the analysis does not see (a callback a
/// builtin makes, a call through a computed callee) may pass anything: the types are a guess, as the
/// field types they feed are, which a tier holds only under the fields' slot dependencies, so such a call
/// that stores what it was passed widens the slot and retires the code.
fn parameters(flows: &[FunctionFlow], facts: &AotFacts, census: &Census) -> (BTreeMap<u32, Vec<Type>>, BTreeMap<u32, Vec<Class>>) {
    let undefined = Type::language(ValueType::Undefined);
    let mut params: BTreeMap<u32, Vec<Type>> = flows.iter().enumerate()
        .filter_map(|(id, flow)| Some((id as u32, vec![Type::NEVER; usize::from(flow.arity?)])))
        .collect();
    let mut classes: BTreeMap<u32, Vec<Class>> = params.iter().map(|(id, slots)| (*id, vec![Class::Never; slots.len()])).collect();
    let mut reached = std::collections::BTreeSet::new();
    for (id, flow) in flows.iter().enumerate() {
        for call in &flow.calls {
            for target in targets(id as u32, call, facts, census) {
                let Some(slots) = params.get_mut(&target) else { continue };
                reached.insert(target);
                for (index, slot) in slots.iter_mut().enumerate() {
                    *slot = slot.join(call.arguments.get(index).map_or(undefined, |argument| argument.ty));
                }
                if let Some(slots) = classes.get_mut(&target) {
                    for (index, slot) in slots.iter_mut().enumerate() {
                        *slot = slot.join(call.arguments.get(index).map_or(Class::Never, |argument| argument.class));
                    }
                }
            }
        }
    }
    // The parameters of a function no recorded call reaches are ones the closed world does not see bound: any
    // value. One a call reaches keeps `NEVER` while no value reaches its arguments yet (the solve's bottom).
    for (function, slots) in &mut params {
        if !reached.contains(function) { slots.iter_mut().for_each(|slot| *slot = Type::ANY); }
    }
    for (function, slots) in &mut classes {
        if !reached.contains(function) { slots.iter_mut().for_each(|slot| *slot = Class::Many); }
    }
    (params, classes)
}
