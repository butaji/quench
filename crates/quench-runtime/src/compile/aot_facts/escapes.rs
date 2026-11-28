//! Which closures and arrays escape the closed world: the functions code the solve does not follow may call, and the
//! element classes whose arrays it may read or write. `Types::of` takes both from `escapes`.

use super::census::{Census, fact_key};
use super::flow::{Call, FunctionFlow, Target};
use super::scope::Scope;
use super::types::targets;
use super::value::{Key, Origin, Word};
use super::AotFacts;
use crate::typed_ir::{Cell, Classes, ElementClass, Elements, Sites, language_bit};
use super::copies::literal_fields;
use crate::typed_ir::Type;
use crate::ValueType;
use std::collections::{BTreeMap, BTreeSet};

/// What the closed world leaves open: the functions code the solve does not follow may call, the element classes of the
/// arrays that code may read or write, and the cells that code may hold. `types::Types::of` takes the functions and the
/// classes; `readers::Readers::finish` takes the cells, as the values an unseen read may reach.
pub(super) struct Escapes {
    /// The functions whose closure escapes (`escapes`).
    pub(super) functions: BTreeSet<u32>,
    /// The escaped functions unknown code may call with no receiver (`plain`): a sloppy one's `this` is then the global
    /// object. A closure handed to code the solve does not follow as a value (a builtin's argument or callee, a thrown or
    /// yielded value, an element, a live global, an escaped return) may be called so. A method an object holds is called
    /// with that object as its `this` (a builtin reads it off the object), so a named store is no plain hand.
    pub(super) plain: BTreeSet<u32>,
    /// The functions a `call`, `apply` or `bind` runs with its first argument as their `this` (`Handed::this_passes`), each
    /// with that argument's cell: the `this` the function takes from that call, whether or not it is plain.
    pub(super) passed_this: Vec<(u32, Cell)>,
    /// The element classes of the arrays the escaped positions hold: each joins the unnamed class.
    pub(super) classes: BTreeSet<ElementClass>,
    /// The cell of every value an escaped position holds (the sinks and the escaped positions' writes).
    pub(super) sinks: Vec<Cell>,
}

impl Escapes {
    /// An assumption of the functions that escape (`pipeline::solve_assumed`), with nothing else open.
    pub(super) fn assumed(functions: BTreeSet<u32>) -> Self {
        Self { functions, plain: BTreeSet::new(), passed_this: Vec::new(), classes: BTreeSet::new(), sinks: Vec::new() }
    }
}

/// The functions whose closure may reach a position the solve cannot follow, so that code it does not see may call
/// them with any `this` and arguments (`Types::of` gives an escaped function `Cell::ANY` for its `this` and every
/// parameter). A closure escapes when a sink may hold it:
/// - the callee, `this` or an argument of a recorded call the solve does not resolve (`resolved`): the call may run a
///   native or an unseen function, which the recorded arguments never reach. A callee read off a key (`reads`) holds
///   only the closures written under that key, so only those escape, and a read of a key no write reaches (a host
///   builtin such as `print` or `Math.abs`) holds no program closure at all. A callee of any other origin is its cell;
/// - a value thrown, or a register live at a `yield` or `await` (`FunctionFlow::handed`): a consumer or a reaction runs it;
/// - a store into an element (`Key::Element`): a host builtin reads an array's elements through the array, which may
///   have been stored into through an alias the analysis does not name;
/// - a store into a global that new code can read by name (`Census::live`: never in a closed world, where no code
///   is added to call a global function by name, and no host builtin does);
/// - a return of an escaped function, of the root, or of an async or generator function (`FunctionFlow::returns_out`),
///   whose caller is a consumer the solve does not see;
/// - a store under a named property (`Key::Member`, `MemberOn`, `MemberAny`) that new code can move (`Census::live`), or
///   any store while an object reaches a sink (`object_reader`): unresolved code may read any named property of it.
/// A sink holds a closure by its cell (`mark`): the function its type names, else, where its identity is lost, every
/// materialized closure. A value passed on to a resolved callee reaches the callee's own cells, which its sinks read.
/// The rule is a fixpoint: an escaped function's return is a sink, which may make an object reader, which escapes
/// the named stores. A method call is resolved only where its receiver is a closed set of program classes whose own
/// methods are the targets (`resolved`): a builtin of the same name, which a program's `map` does not shadow, gets
/// the arguments as sinks.
///
/// It also returns the element classes the held values' arrays are in (`Escapes::classes`, which `Types::of` joins to the
/// unnamed class): an array a sink holds may be read or written by code the solve does not follow, so its class is not one
/// the solve sees whole, and every array of the class shares the unnamed class's elements. The held cells are its sinks
/// (`Escapes::sinks`), which the readers take as the values unseen code may read.
pub(super) fn escapes(flows: &[FunctionFlow], facts: &AotFacts, census: &Census, scope: &Scope) -> Escapes {
    let materialized: BTreeSet<u32> = flows.iter().flat_map(|flow| flow.closures.iter().copied()).collect();
    let roots: BTreeSet<u32> = flows.iter().enumerate().filter(|(_, flow)| flow.returns_out).map(|(id, _)| id as u32).collect();
    let writes: Vec<(Key, Word)> = flows.iter().flat_map(|flow| &flow.writes).map(|&(_, key, word)| (key, word)).collect();
    let writers: Vec<u32> = flows.iter().enumerate().flat_map(|(id, flow)| flow.writes.iter().map(move |_| id as u32)).collect();
    // The calls each function makes may run (`types::targets`, and the one closure the census names for a register call).
    let callees: Vec<BTreeSet<u32>> = flows.iter().enumerate().map(|(id, flow)| flow.calls.iter().flat_map(|call| {
        let mut callees = targets(id as u32, call, facts, census);
        if call.target == Target::Register && let Some(function) = census.target(call.callee) { callees.push(function); }
        callees
    }).collect()).collect();
    // The functions that run with no call the flows record: the roots (and a Module's declarations, which the host imports), the
    // accessors, and the closures stored under a name a builtin reads with no instruction of the program (a conversion's
    // `valueOf`, a thenable's `then`, `scope::Scope::protocols`). Every other function runs only where a live one calls it, or
    // where it escapes. A function that never runs hands nothing to unseen code and stores nothing.
    let mut seeds: BTreeSet<u32> = (0..flows.len() as u32).filter(|&id| scope.parent(id).is_none() || scope.is_module() && scope.parent(id).is_some_and(|parent| scope.parent(parent).is_none())).collect();
    seeds.extend(census.accessors.functions());
    for &(key, word) in &writes {
        if let Key::Member(atom) | Key::MemberOn { atom, .. } | Key::MemberAny(atom) = key && scope.protocols.contains(&atom) {
            mark(&word.cell, &materialized, &mut seeds);
        }
    }
    // A least fixpoint: the live functions and the escaped ones grow together, each round over the last round's.
    let mut escaped = BTreeSet::new();
    loop {
        let mut live: BTreeSet<u32> = seeds.union(&escaped).copied().collect();
        let mut pending: Vec<u32> = live.iter().copied().collect();
        while let Some(function) = pending.pop() {
            for &callee in callees.get(function as usize).into_iter().flatten() {
                if live.insert(callee) { pending.push(callee); }
            }
        }
        let open = escapes_live(flows, facts, census, scope, &live, &escaped, &writes, &writers, &materialized, &roots);
        if crate::trace::Channel::AotLanes.on() { eprintln!("aot-lanes: escapes over {} live functions: {:?}", live.len(), open.functions); }
        if open.functions.is_subset(&escaped) { return open; }
        escaped.extend(open.functions.iter().copied());
    }
}

/// `escapes` over the functions `live` holds: their sinks, and the writes they make (`writers`).
#[allow(clippy::too_many_arguments)]
fn escapes_live(flows: &[FunctionFlow], facts: &AotFacts, census: &Census, scope: &Scope, live: &BTreeSet<u32>, escaped: &BTreeSet<u32>,
    writes: &[(Key, Word)], writers: &[u32], materialized: &BTreeSet<u32>, roots: &BTreeSet<u32>,
) -> Escapes {
    let handed = Handed::of(flows, live, facts, census, scope);
    let live_writes: Vec<bool> = writers.iter().map(|writer| live.contains(writer)).collect();
    // A name-wide store (`Key::Member`) made with its own-class store (`MemberOn`, `MemberAny`) at the same instruction is that
    // store's mirror: the own-class store, which names the object, decides whether it is held.
    let own: BTreeSet<(usize, u32, u32)> = flows.iter().enumerate()
        .flat_map(|(id, flow)| flow.writes.iter().filter_map(move |&(pc, key, _)| match key {
            Key::MemberOn { atom, .. } | Key::MemberAny(atom) => Some((id, pc, atom)),
            _ => None,
        }))
        .collect();
    let mirrors: Vec<bool> = flows.iter().enumerate()
        .flat_map(|(id, flow)| flow.writes.iter().map(move |&(pc, key, _)| (id, pc, key)))
        .map(|(id, pc, key)| matches!(key, Key::Member(atom) if own.contains(&(id, pc, atom))))
        .collect();
    // Every `extends` link any flow makes, agreeing or not: an instance reaches each parent's prototype a link may give it.
    let mut parents: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    for &(child, parent) in flows.iter().flat_map(|flow| &flow.parents) {
        parents.entry(child).or_default().insert(parent);
    }
    // The classes of each body: the body's own and its synthetic classes (`ClassIds::body`), which a closure of it may construct.
    let mut bodies: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    for class in 0..census.classes.count() {
        bodies.entry(census.classes.body(class)).or_default().insert(class);
    }
    let round = Round {
        handed: &handed,
        reached: read_words(writes, &handed.read),
        plain_reached: read_words(writes, &handed.plain_read),
        writes,
        live_writes: &live_writes,
        mirrors: &mirrors,
        elements: {
            let mut links: Vec<(ElementClass, ElementClass)> = flows.iter().flat_map(|flow| &flow.links).copied().collect();
            super::types::entry_links(flows, facts, census, escaped, &mut links);
            super::census::ElementClasses::of(links.iter())
        },
        defined: flows.iter().enumerate().filter(|(id, _)| live.contains(&(*id as u32)))
            .flat_map(|(_, flow)| flow.defined.iter().filter(|(class, _)| class.is_none()).map(|&(_, word)| word)
                .chain(flow.literals.values().flatten().map(|&(_, word)| word))).collect(),
        defined_elements: flows.iter().enumerate().filter(|(id, _)| live.contains(&(*id as u32)))
            .flat_map(|(_, flow)| flow.defined.iter().filter_map(|&(class, word)| Some((class?, word)))).collect(),
        census,
        flows,
        parents: &parents,
        bodies: &bodies,
        roots,
        // Every accessor's function, which code the solve does not see may run with any receiver where an object reaches such code.
        accessor_words: census.accessors.functions().map(Word::closure).collect(),
    };
    let (mut escaped, mut plain) = (BTreeSet::new(), BTreeSet::new());
    loop {
        let (held, plain_held) = round.held(&escaped, census);
        let next = marked(&held, materialized, &escaped, &round);
        let next_plain = marked(&plain_held, materialized, &plain, &round);
        if next == escaped && next_plain == plain {
            let classes = held.iter().filter(|word| word.cell.elements != Elements::NEVER).map(|word| word.elements_key()).collect();
            let passed_this = handed.this_passes.iter().flat_map(|&(receiver, this)| {
                let mut functions = BTreeSet::new();
                mark(&receiver.cell, materialized, &mut functions);
                functions.into_iter().map(move |function| (function, this.cell))
            }).collect();
            if crate::trace::Channel::AotLanes.on() {
                eprintln!("aot-lanes: escapes {} functions, {} plain", escaped.len(), plain.len());
                for origin in &handed.read {
                    let words = read_words(writes, std::slice::from_ref(origin));
                    let closures: Vec<u32> = words.iter().filter_map(|word| word.cell.ty.body_index()).collect();
                    if !closures.is_empty() { eprintln!("aot-lanes:   unresolved read {origin:?} reaches {closures:?}"); }
                }
                let (sinks, reached) = (handed.sinks.len(), round.reached.len());
                for (index, word) in held.iter().enumerate() {
                    let mut functions = BTreeSet::new();
                    mark(&word.cell, materialized, &mut functions);
                    if functions.is_empty() { continue; }
                    let from = if index < sinks { "sink" } else if index < sinks + reached { "reached" } else { "held" };
                    eprintln!("aot-lanes:   escape {functions:?} by {from} {:?}: {:?} {:?}", handed.sources.get(index).map(|source| (source.0, source.1, source.2.as_str())), word.cell.ty, word.origin);
                }
            }
            return Escapes { functions: escaped, plain, passed_this, classes, sinks: held.iter().map(|word| word.cell).collect() };
        }
        escaped = next;
        plain = next_plain;
    }
}

/// The values the flows hand to code the solve does not follow (`escapes`): every one of them is held (`sinks`), and the ones
/// unknown code may call with no receiver are `plain`. `read` holds the origins an unresolved call reads its callee from, and
/// `plain_read` those of an unresolved call that may run its callee with no receiver (a plain call, or one on the global object).
#[derive(Default)]
struct Handed {
    sinks: Vec<Word>,
    plain: Vec<Word>,
    read: Vec<Origin>,
    plain_read: Vec<Origin>,
    /// Per `call`, `apply` or `bind` the solve does not resolve: the function it is called on and its first argument, the
    /// `this` the function runs with.
    this_passes: Vec<(Word, Word)>,
    /// Per sink, the function and PC that hand it (the `aot-lanes` trace).
    sources: Vec<(u32, u32, String)>,
}

impl Handed {
    fn of(flows: &[FunctionFlow], live: &BTreeSet<u32>, facts: &AotFacts, census: &Census, scope: &Scope) -> Self {
        let mut handed = Self::default();
        for (id, flow) in flows.iter().enumerate().filter(|(id, _)| live.contains(&(*id as u32))) {
            for call in &flow.calls {
                handed.call(id as u32, call, facts, census, scope);
                if crate::trace::Channel::AotLanes.on() {
                    let tag = format!("{:?} callee {:?} construct {}", call.target, call.callee.origin, call.construct);
                    handed.sources.resize(handed.sinks.len(), (id as u32, call.pc, tag));
                }
            }
            for word in &flow.handed {
                handed.hand(*word);
                if crate::trace::Channel::AotLanes.on() { handed.sources.resize(handed.sinks.len(), (id as u32, 0, String::from("handed"))); }
            }
        }
        handed
    }

    /// Hands `word` to code the solve does not follow, which may call it with no receiver.
    fn hand(&mut self, word: Word) {
        self.sinks.push(word);
        self.plain.push(word);
    }

    /// Records the values one recorded call hands to code the solve does not follow. A call the solve resolves, and an array
    /// builtin the program does not shadow (`builtin_array_call`), hands none.
    fn call(&mut self, function: u32, call: &Call, facts: &AotFacts, census: &Census, scope: &Scope) {
        // A named access the census proves runs its accessor, whose flow reads the operands as its parameters do. One it
        // proves no accessor of hands its operands to the accessors of its name, which the solve does not follow.
        if let Target::Accessor { atom, .. } = call.target {
            if targets(function, call, facts, census).is_empty() && census.accessors.defines(atom) {
                self.sinks.push(call.receiver);
                self.sinks.extend(call.arguments.iter().copied());
            }
            return;
        }
        // A method call on a receiver no value reaches yet (`Type::NEVER`, a pass from the bottom) runs nothing yet.
        // One whose receiver is only `null` or `undefined` throws before it reads the method, and runs nothing either.
        if call.receiver.cell.ty.is_subtype(Type::NULLISH) && method_atom(call).is_some() { return; }
        let callees = targets(function, call, facts, census);
        if resolved(call, &callees, census) || builtin_array_call(call, &callees, scope) { return; }
        // A plain call, or one on the global object, may run its callee with a `this` the solve does not follow.
        let plain_form = call.plain || call.receiver.cell.global;
        if call.target == Target::Register {
            match call.callee.origin {
                Some(origin @ (Origin::Binding(_) | Origin::Member(_) | Origin::MemberOn { .. })) => {
                    self.read.push(origin);
                    if plain_form { self.plain_read.push(origin); }
                    // A global unseen code may write holds only what unseen code holds, which the escapes hold already: only a
                    // global store of the program's own that names no global makes the callee any closure.
                    let wild = match origin {
                        Origin::Binding(Key::Global(_)) => census.program_global_wild(),
                        _ => census.wild_read(origin),
                    };
                    if wild {
                        let any = Word::of_cell(Cell::ANY);
                        self.sinks.push(any);
                        if plain_form { self.plain.push(any); }
                    }
                }
                _ => self.hand(call.callee),
            }
        }
        match this_argument(call, scope) {
            // The function runs with the first argument as its `this` (`passed_this`), so it is held but not handed as a value:
            // the argument is held, and the arguments after it are handed to the function as its arguments.
            Some(this) => {
                self.sinks.push(call.receiver);
                self.this_passes.push((call.receiver, this));
                self.sinks.push(this);
                for argument in call.arguments.iter().skip(1) {
                    self.hand(*argument);
                }
            }
            None => {
                // A construction's receiver is the object it makes: fresh, holding nothing but its prototype, which is the callee's
                // and reached through the callee, a sink of its own. So it hands nothing the callee does not.
                // A plain call's receiver is `undefined` or, sloppy, the global object, whose globals unseen code of a closed world
                // reads by no name (`Census::live`).
                if !call.construct && !call.plain { self.hand(call.receiver); }
                for argument in &call.arguments {
                    self.hand(*argument);
                }
            }
        }
    }
}

/// The `this` a call hands its callee as its first argument, where the call is `call`, `apply` or `bind` (`Scope::passes_this`):
/// those run their receiver with that argument as its `this`. None for any other call.
fn this_argument(call: &Call, scope: &Scope) -> Option<Word> {
    let passes_this = method_atom(call).is_some_and(|atom| scope.passes_this(atom));
    if passes_this { call.arguments.first().copied() } else { None }
}

/// What `escapes` holds in one round: the held values (`Round::held`), built from the flows' sinks, the closures unresolved
/// calls may read (`reached`, `plain_reached`), the writes the program makes, the escaped returns, and the accessors' functions.
struct Round<'a> {
    handed: &'a Handed,
    reached: Vec<Word>,
    plain_reached: Vec<Word>,
    writes: &'a [(Key, Word)],
    /// Per write, whether a live function makes it (`escapes_live`).
    live_writes: &'a [bool],
    /// Per write, whether it is a name-wide mirror of an own-class store at the same instruction (`escapes`).
    mirrors: &'a [bool],
    /// The element classes, every link joined (the flows' and the resolved calls' entries, `types::entry_links`).
    elements: super::census::ElementClasses,
    /// Every value an object literal of a live function defines (`FunctionFlow::defined`, `FunctionFlow::literals`).
    defined: Vec<Word>,
    /// Every element an array literal of a live function defines, by the literal's element class (`FunctionFlow::defined`).
    defined_elements: Vec<(ElementClass, Word)>,
    census: &'a Census,
    flows: &'a [FunctionFlow],
    /// Every `extends` link: the parents each class's prototype may link to.
    parents: &'a BTreeMap<u32, BTreeSet<u32>>,
    /// Per function body, the classes a closure of it constructs.
    bodies: &'a BTreeMap<u32, BTreeSet<u32>>,
    roots: &'a BTreeSet<u32>,

    accessor_words: Vec<Word>,
}

impl Round<'_> {
    /// Whether `word` is a host value: an object a host constructor makes (`Origin::Foreign`), or a value read off a key no
    /// program write reaches (a host global such as `console`, a builtin's method), where no write the analysis cannot follow
    /// reaches either. It holds a program object or closure only through a store into it: a named store (`MemberAny`, held where
    /// any object is) or an element store (always held). So it names no class and marks no closure of its own.
    fn host(&self, word: Word) -> bool {
        match word.origin {
            // A global unseen code may write (the global object it holds) holds a value unseen code had, which is held already.
            Some(Origin::Foreign) => !self.census.program_global_wild(),
            Some(origin @ Origin::Binding(Key::Global(_))) =>
                !self.census.program_global_wild() && !self.writes.iter().any(|&(key, _)| reads(origin, key)),
            Some(origin @ (Origin::Binding(_) | Origin::Member(_))) =>
                !self.census.wild_read(origin) && !self.writes.iter().any(|&(key, _)| reads(origin, key)),
            _ => false,
        }
    }

    /// The values held once `escaped` is marked, and the ones unknown code may call plainly (`Escapes::plain`). A named store
    /// is held where new code can move it, or where unknown code can reach its object: a class's own store (`MemberOn`) where
    /// an instance, the prototype or the constructor of the class or of a class that extends it is held (`Reach`); any other
    /// named store where any object is held. A held value adds the objects it may be, so the rule is a fixpoint. A named store
    /// is no plain hand: the method is called with its object.
    fn held(&self, escaped: &BTreeSet<u32>, census: &Census) -> (Vec<Word>, Vec<Word>) {
        let mut held: Vec<Word> = self.handed.sinks.iter().chain(&self.reached).copied().collect();
        let mut plain: Vec<Word> = self.handed.plain.iter().chain(&self.plain_reached).copied().collect();
        // An element store is held where its array's class (`ElementClasses::root`, every link joined) is one a held array is of,
        // or the unnamed class, which every array of no origin is of: unknown code reads those arrays. Held arrays add classes.
        let unnamed = self.elements.root(ElementClass::Unnamed);
        let mut held_classes: BTreeSet<ElementClass> = BTreeSet::from([unnamed]);
        // An array literal's definitions are its elements: held as its element stores are.
        let mut elements: Vec<(ElementClass, Word)> = self.defined_elements.iter().map(|&(class, word)| (self.elements.root(class), word)).collect();
        let mut named = Vec::new();
        for ((&(key, word), &mirror), _) in self.writes.iter().zip(self.mirrors).zip(self.live_writes).filter(|(_, live)| **live) {
            match key {
                Key::Element(class) => elements.push((self.elements.root(class), word)),
                Key::Global(_) if census.live(fact_key(key)) => { held.push(word); plain.push(word); }
                Key::Global(_) => {}
                Key::Return(function) if escaped.contains(&function) || self.roots.contains(&function) => {
                    held.push(word);
                    plain.push(word);
                }
                Key::Member(_) if mirror && !census.live(fact_key(key)) => {}
                Key::Member(_) | Key::MemberOn { .. } | Key::MemberAny(_) => named.push((key, word)),
                Key::Return(_) | Key::Slot { .. } => {}
            }
        }
        let mut reach = Reach::default();
        let (sinks, reached) = (self.handed.sinks.len(), self.reached.len());
        let mut defined_held = false;
        for (index, word) in held.clone().into_iter().enumerate() {
            let every = reach.every;
            reach.add(word, self, &mut held);
            if !every && reach.every && crate::trace::Channel::AotLanes.on() {
                let from = if index < sinks { "sink" } else if index < sinks + reached { "reached" } else { "write" };
                eprintln!("aot-lanes: escapes open every class from {from} {:?}: {word:?}", self.handed.sources.get(index));
            }
        }
        loop {
            // A program object of no known site may be an object literal, whose definitions unseen code then reads.
            if reach.unsited && !defined_held {
                defined_held = true;
                for word in self.defined.clone() { held.push(word); reach.add(word, self, &mut held); }
            }
            let before = named.len() + elements.len();
            let (taken, rest): (Vec<_>, Vec<_>) = named.into_iter().partition(|&(key, _)| census.live(fact_key(key)) || reach.reaches(key));
            named = rest;
            for (_, word) in taken {
                held.push(word);
                reach.add(word, self, &mut held);
            }
            held_classes.extend(held.iter().filter(|word| word.cell.elements != Elements::NEVER).map(|word| self.elements.root(word.elements_key())));
            let (taken, rest): (Vec<_>, Vec<_>) = elements.into_iter().partition(|(class, _)| held_classes.contains(class));
            elements = rest;
            for (_, word) in taken {
                held.push(word);
                plain.push(word);
                reach.add(word, self, &mut held);
            }
            if named.len() + elements.len() == before && (defined_held || !reach.unsited) { break; }
        }
        if reach.object { held.extend(self.accessor_words.iter().copied()); }
        // An instance reaches its constructor (`constructor` up its chain), which unknown code may then call.
        // Where the held objects name no class set, every class's constructor may be reached so.
        let constructors: Box<dyn Iterator<Item = u32>> = if reach.every { Box::new(self.bodies.keys().copied()) } else { Box::new(reach.classes.iter().map(|&class| census.classes.body(class))) };
        held.extend(constructors.map(Word::closure));
        (held, plain)
    }
}

/// The objects held values may be, as far as unknown code can reach them (`Round::held`): whether any object is held, and the
/// classes whose own stores it can read (an instance's, its prototype's or its constructor's, closed under `extends`), or all
/// of them (`every`) where a held object names no class set (a prototype read, a closure of unknown identity, `Classes::Many`).
#[derive(Default)]
struct Reach {
    object: bool,
    /// Whether a held object may be a program object literal of no known site (`Sites::Many`, not a host value).
    unsited: bool,
    every: bool,
    classes: BTreeSet<u32>,
    /// The literal sites whose values are added.
    sites: BTreeSet<(u32, u32)>,
}

impl Reach {
    /// Adds the objects `word` may be, and the values of the literal objects it may be (`copies::literal_fields`), which unknown
    /// code reads through it: those are held too, so they are added in turn.
    fn add(&mut self, word: Word, round: &Round<'_>, held: &mut Vec<Word>) {
        let mut pending = vec![word];
        while let Some(word) = pending.pop() {
            // A primitive (by its type) is no object, whatever classes its cell defaults to (`Classes::of_type`).
            if !word.cell.ty.may_be_languages(Type::languages(language_bit(ValueType::Object) | language_bit(ValueType::Function))) { continue; }
            self.object = true;
            self.unsited |= word.cell.sites == Sites::Many && !round.host(word) && word.cell.ty.body_index().is_none()
                && !matches!(word.origin, Some(Origin::Alloc { .. } | Origin::Instance(_)));
            if let Some(sites) = word.cell.sites.set() {
                for site in sites {
                    if !self.sites.insert((site.function, site.pc)) { continue; }
                    match literal_fields(round.flows, Sites::one(*site)) {
                        Some(fields) => for (_, field) in fields { held.push(field); pending.push(field); },
                        None => self.every = true,
                    }
                }
            }
            if self.every { continue; }
            self.add_classes(word, round);
        }
    }

    fn add_classes(&mut self, word: Word, round: &Round<'_>) {
        let cell = word.cell;
        if matches!(word.origin, Some(Origin::Prototype(_))) { self.every = true; return; }
        if round.host(word) { return; }
        let mut found: Vec<u32> = Vec::new();
        let array = matches!(word.origin, Some(Origin::Alloc { .. }));
        // A closure is no class's instance; it is a constructor, whose static members and `prototype` unknown code reads.
        // An allocated array is no class's instance either: its cell's classes are its type's default (`Classes::of_type`).
        match cell.ty.body_index() {
            Some(body) => found.extend(round.bodies.get(&body).into_iter().flatten().copied()),
            None if cell.ty.may_be_languages(Type::language(ValueType::Function)) => { self.every = true; return; }
            None if array => {}
            None => match cell.classes {
                Classes::Never => {}
                Classes::Many => { self.every = true; return; }
                classes => found.extend_from_slice(classes.ids()),
            },
        }
        // An array's elements are its definitions and its element stores, which are held themselves (`Round::held`).
        while let Some(class) = found.pop() {
            if self.classes.insert(class) {
                found.extend(round.parents.get(&class).into_iter().flatten().copied());
            }
        }
    }

    /// Whether unknown code holding these objects can read a store under `key`.
    fn reaches(&self, key: Key) -> bool {
        match key {
            Key::MemberOn { class, .. } => self.every || self.classes.contains(&class),
            _ => self.object,
        }
    }
}

/// The words the writes hold under a key one of `origins` may read (`reads`): the closures an unresolved call's callee may be.
fn read_words(writes: &[(Key, Word)], origins: &[Origin]) -> Vec<Word> {
    writes.iter().filter(|(key, _)| origins.iter().any(|&origin| reads(origin, *key))).map(|&(_, word)| word).collect()
}

/// `start` with the functions each of `words` may hold marked (`mark`).
/// A host value (`Round::host`) holds no program closure but one a store put in it, which is held itself.
fn marked(words: &[Word], materialized: &BTreeSet<u32>, start: &BTreeSet<u32>, round: &Round<'_>) -> BTreeSet<u32> {
    let mut next = start.clone();
    for word in words.iter().filter(|word| !round.host(**word)) {
        mark(&word.cell, materialized, &mut next);
    }
    next
}

/// Whether a read off `origin` may take the value `key` stores (`escapes`): the binding itself; a named property of the
/// read's name, whichever object holds it (`Key::Member`, `MemberAny`), or of a class's own method for a read off one
/// class (`MemberOn` of that class and instance). A read off any object may see a class's own method of that name, so
/// `Origin::Member` reaches every `MemberOn` of the name.
fn reads(origin: Origin, key: Key) -> bool {
    match (origin, key) {
        (Origin::Binding(read), _) => read == key,
        (Origin::MemberOn { class, instance, atom, .. }, Key::MemberOn { class: own, instance: own_instance, atom: name }) => own == class && own_instance == instance && name == atom,
        (Origin::Member(atom), Key::Member(name) | Key::MemberAny(name) | Key::MemberOn { atom: name, .. }) => atom == name,
        (Origin::MemberOn { atom, .. }, Key::Member(name) | Key::MemberAny(name)) => atom == name,
        _ => false,
    }
}

/// Whether a recorded call resolves: its targets are known functions (`targets` is not empty) and the program's own code
/// runs (`resolved` below). A method call's arguments reach the callees' parameters (`parameters`) only then, so an
/// unresolved call's arguments escape instead (`escapes`).
///
/// A callee read off a receiver (`Target::Method`, or a method read's `Origin::Member`/`MemberOn`) is a guess of which
/// method runs: a builtin of the same name (`[1].map(g)` beside a program's `map`) may be the one that runs, and `g` is
/// its argument. Only a receiver whose classes are a closed program set, each holding the targets as its own method
/// (`Census::own_methods`), proves the program's method runs.
pub(super) fn resolved(call: &Call, callees: &[u32], census: &Census) -> bool {
    if callees.is_empty() { return false; }
    match (call.target, call.callee.origin) {
        (Target::Method(atom), _) => census.own_methods(call.receiver.cell.classes, atom, callees),
        // A method read off a receiver whose solved classes each hold their own method of the name, all among the callees, runs
        // one of them: the classes are the solve's own (a guess to a lane, which guards them), so within the solve the call is
        // resolved, and its arguments flow to the callees' parameters, not to unseen code. A key new code can move is no proof.
        (Target::Register, Some(Origin::Member(atom) | Origin::MemberOn { atom, .. })) => {
            !census.live_callee(call.callee) && census.own_methods(call.receiver.cell.classes, atom, callees)
        }
        _ => !census.guessed(call.callee),
    }
}

/// The method a call names off its receiver: a method call's name, or the name a method read's origin carries.
pub(super) fn method_atom(call: &Call) -> Option<u32> {
    match (call.target, call.callee.origin) {
        (Target::Method(atom), _) => Some(atom),
        (Target::Register, Some(Origin::Member(atom) | Origin::MemberOn { atom, .. })) => Some(atom),
        _ => None,
    }
}

/// Whether `call` is an array builtin (`Scope::is_array_builtin`) on a receiver that may be an array, with no program
/// closure under its name (`callees` empty). It runs no code the solve does not see: its receiver is read and written
/// through the receiver's own class, and its arguments are not escapes (a mutator stores them, `record::Record::observe`).
fn builtin_array_call(call: &Call, callees: &[u32], scope: &Scope) -> bool {
    callees.is_empty() && call.receiver.cell.elements != Elements::NEVER && method_atom(call).is_some_and(|atom| scope.is_array_builtin(atom))
}

/// Adds to `escaped` the functions a cell may hold a closure of: its type's identity and its elements' type's identity. A
/// type with a function alternative but no body (identity lost: a join of two closures, or `Type::ANY`) may hold any
/// materialized closure, so each of them escapes.
pub(super) fn mark(cell: &Cell, materialized: &BTreeSet<u32>, escaped: &mut BTreeSet<u32>) {
    if let Some(function) = cell.ty.body_index() {
        escaped.insert(function);
    } else if cell.ty.may_be_languages(Type::language(ValueType::Function)) {
        escaped.extend(materialized.iter().copied());
    }
    // An array's elements hold a closure its definitions or its element stores put there: a definition's value is held where a
    // program object of unknown origin is (`Round::held`, `FunctionFlow::defined`), and an element store's always, so only an
    // element type that names its closure marks one here.
    if let Some(function) = cell.elements.ty.body_index() {
        escaped.insert(function);
    }
}

/// Whether a cell may hold an object: an object, a function or an instance of a class. Code the solve does not see may read
/// any named property of such a value, so a store under a named property escapes wherever one reaches a sink (`escapes`).
fn may_be_object(cell: Cell) -> bool {
    let objects = Type::languages(language_bit(ValueType::Object) | language_bit(ValueType::Function));
    cell.ty.may_be_languages(objects) || cell.classes != Classes::Never
}
