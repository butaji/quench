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

mod accessors;
mod blocks;
mod census;
mod class_ids;
mod class_layout;
mod copies;
mod effects;
mod escapes;
mod flow;
mod function;
mod gate;
mod length_settle;
mod lengths;
mod phase;
mod pipeline;
mod quiet;
mod readers;
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
use crate::typed_ir::{ElementClass, Representation};
use super::call_plan::{self, CallPlan};
use super::signatures::{CallSite, Signatures};
use class_layout::ClassLayouts;
use readers::Readers;
use record::ElementStore;
pub use class_layout::{FieldDefault, UnionField, UnionLayout};
pub use function::FunctionFacts;
pub use gate::World;
pub use pipeline::{analyze, analyze_as, world};
pub use registers::{RegisterCells, Source};
pub use scope::constant_type;

/// The length rounds: round 1 solves with no feedback, round 2 with the facts round 1 proved (`length_settle::Feedback`).
const LENGTH_ROUNDS: usize = 2;

/// The facts of one program, solved in `world`.
pub struct AotFacts {
    /// The world the facts were solved in: which keys new code can move (`live`).
    world: World,
    /// `(function, pc)` of a plain call → the function it always calls, or, for a method read
    /// (`guessed_calls`), the one closure every store of its name writes.
    pub(crate) call_targets: BTreeMap<(u32, u32), u32>,
    /// `(function, pc)` of a field access that runs an accessor the census proves (`accessors::accessor_arms`) → the arms
    /// of its call: the getter (a read) or the setter (a store) per class of the receiver. The access's callees
    /// (`FunctionFacts::call_targets`) and its call plan (`call_plan::call_plans`) are these arms.
    pub(crate) accessor_arms: BTreeMap<(u32, u32), accessors::AccessorArms>,
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
    /// `(function, pc)` of a construction (`new`) → the class id of the instance it allocates (`class_ids::ClassIds`): the
    /// function's own class, or the synthetic class of its body and the key the constructor was read from. The class is a
    /// fact of the site, so a runtime that records a class's header records it under this id.
    pub(crate) construct_classes: BTreeMap<(u32, u32), u32>,
    /// The class ids of the program's constructors (`class_ids::ClassIds`): the functions, and a synthetic class for each body
    /// that several binding keys construct. Every class id of this table names a body through `ClassIds::body`.
    pub(crate) classes: class_ids::ClassIds,
    /// Function id → register → the Number representation it always holds.
    pub(crate) numbers: BTreeMap<u32, Box<[Option<Representation>]>>,
    /// Per field name, the type of the values every store of it writes, into an object of any class: the
    /// join of the name's `FactKey::Field` and `FactKey::AnyField` cells, which a tier keeps only under the
    /// slot's dependency.
    pub(crate) field_types: BTreeMap<u32, crate::typed_ir::Type>,
    /// The settled cell of every key of the program (`typed_ir::FactKey`): each class's own fields (joined
    /// with the stores into objects of no one class), returns, bindings, the elements of each element class
    /// (`Elements`), parameters and receivers. The one store of the solve's facts: code that
    /// trusts a cell subscribes to its key.
    pub(crate) cells: BTreeMap<crate::typed_ir::FactKey, crate::typed_ir::Cell>,
    /// Per `FactKey::Elements`, the keys of every element class its class shares (`element_class_keys`).
    pub(crate) element_classes: BTreeMap<crate::typed_ir::FactKey, Box<[crate::typed_ir::FactKey]>>,
    /// `(function, pc)` of a named field access → the classes of the objects it reads or stores, and of a call →
    /// the classes of its receiver, where the settled cells list them (`typed_ir::Classes::Set`): a guess.
    pub(crate) access_classes: BTreeMap<(u32, u32), crate::typed_ir::Classes>,
    /// Per function, the cells and sources its registers hold at every point of the settled solve (`registers::RegisterCells`):
    /// what a tier lowering the function reads instead of re-deriving a register's type, range or classes.
    pub(crate) registers: BTreeMap<u32, RegisterCells>,
    /// Per function, the length relations each pc holds before its instruction (`length_settle::Settled`): an index proven below
    /// an array's length, which `FunctionFacts::below_length` answers.
    pub(crate) lengths: BTreeMap<u32, length_settle::Settled>,
    /// Whether no index accessor can run for an element access (`gate::index_accessors_absent`): an element access then runs
    /// no guest code for a key that converts to none (`lengths::transfer::element_runs_guest_code`).
    pub(crate) index_accessors_absent: bool,
    /// Whether no builtin prototype object (`Array.prototype`, `Object.prototype`) has an index property, of any kind, in the closed
    /// world (`AotFacts::prototype_indices_empty`): `index_accessors_absent` and no store writes an index of one.
    pub(crate) prototype_indices_absent: bool,
    /// Whether the property `apply` of every function is `Function.prototype.apply` (`gate::apply_is_builtin`): the callee
    /// register of a `f.apply(this, arguments)` forwarding call then holds the builtin, which calls `f` with the caller's arguments.
    pub(crate) apply_builtin: bool,
    /// Per function, the keys its settled cells were read from (`FunctionFacts::dependencies`).
    pub(crate) dependencies: BTreeMap<u32, Box<[crate::typed_ir::FactKey]>>,
    /// `(function, pc)` of an element access (a read or a store) whose base is a typed array of one native kind
    /// (`typed_ir::View::One`) → that kind (`FunctionFacts::element_view`).
    pub(crate) element_views: BTreeMap<(u32, u32), crate::heap::TypedArrayKind>,
    /// Per function, the capture slots it reads from its parent's environment, as (owner, slot), in slot order
    /// (`FunctionFacts::capture_cells`).
    pub(crate) captures: BTreeMap<u32, Box<[(u32, u16)]>>,
    /// `(function, pc)` of a call whose callee was read from a key → the method name it reads (a member read) and
    /// that key: what a call plan's miss subscribes to (`call_plan::Miss::Edge`).
    pub(crate) call_keys: BTreeMap<(u32, u32), (Option<u32>, crate::typed_ir::FactKey)>,
    /// `(function, pc)` of a call → what it passes its callee (`CallSite`): each argument's array class and the caller's
    /// parameter it is on entry. The call's writes are lifted through it (`signatures::solve`).
    pub(crate) call_sites: BTreeMap<(u32, u32), CallSite>,
    /// `(function, pc)` of an element store → the array class it writes and the indices it touches (`ElementStore`), where
    /// the settled solve keeps the store.
    pub(crate) element_stores: BTreeMap<(u32, u32), ElementStore>,
    /// The composed effect signatures of the program's functions and call forms, solved over the settled facts
    /// above (`signatures::solve`); none where the type solve did not settle, so no signature is taken from it.
    pub(crate) signatures: Option<Signatures>,
    /// The closed-world layouts of the program's classes (`class_layout`): each instance layout and prototype layout
    /// the solve proved, read by `AotFacts::class_layout` and `AotFacts::prototype_layout`. The only home of the names
    /// a class's `prototype` takes.
    pub(crate) layouts: ClassLayouts,
    /// The union layouts of the constructed classes the closed world proves one for (`class_layout::unions_of`), read by
    /// `AotFacts::class_union_layout`.
    unions: BTreeMap<u32, UnionLayout>,
    /// The reads and escapes of the settled program (`readers`): which field keys some read may observe, and the calls
    /// whose result no read follows. None where the type solve did not settle.
    readers: Option<Readers>,
}

impl AotFacts {
    /// No fact yet, solved in `world`: the solve fills the maps.
    fn unsolved(world: World) -> Self {
        Self {
            world,
            call_targets: BTreeMap::new(),
            accessor_arms: BTreeMap::new(),
            guessed_calls: std::collections::BTreeSet::new(),
            call_candidates: BTreeMap::new(),
            construct_targets: BTreeMap::new(),
            construct_classes: BTreeMap::new(),
            classes: class_ids::ClassIds::default(),
            numbers: BTreeMap::new(),
            field_types: BTreeMap::new(),
            cells: BTreeMap::new(),
            element_classes: BTreeMap::new(),
            access_classes: BTreeMap::new(),
            registers: BTreeMap::new(),
            lengths: BTreeMap::new(),
            index_accessors_absent: false,
            prototype_indices_absent: false,
            apply_builtin: false,
            dependencies: BTreeMap::new(),
            element_views: BTreeMap::new(),
            captures: BTreeMap::new(),
            call_keys: BTreeMap::new(),
            call_sites: BTreeMap::new(),
            element_stores: BTreeMap::new(),
            signatures: None,
            layouts: ClassLayouts::default(),
            unions: BTreeMap::new(),
            readers: None,
        }
    }

    /// The composed effect signatures the solve settled (`signatures`), none where the type solve did not settle.
    pub fn signatures(&self) -> Option<&Signatures> {
        self.signatures.as_ref()
    }

    /// The call plan of every call `function` makes, by pc (`call_plan::call_plans`), over the settled facts and the
    /// composed signatures. Inlining is not decided here: the interpreter's records never inline a callee, so every
    /// plan is taken with `inlinable` false and its form is the call's own (`Framed` or `Frameless`). Empty where the
    /// type solve did not settle (no signatures), so no site takes a plan from it; a site absent from the map is one
    /// the solve did not settle.
    pub fn call_plans(&self, program: &ResidualProgram, function: u32) -> BTreeMap<u32, CallPlan> {
        match self.signatures.as_ref() {
            Some(signatures) => call_plan::call_plans(program, self, signatures, function, |_| false),
            None => BTreeMap::new(),
        }
    }

    /// Whether new code (`World`) can move `key`. A tier that trusts a live key's fact subscribes to
    /// the key (its `code_dependencies` cell readers), and an edge retires those readers. A key that is
    /// not live needs no subscription and no recovery: a closed unit has no exits.
    pub fn live(&self, key: crate::typed_ir::FactKey) -> bool {
        self.world.live(key)
    }

    /// Whether the property `apply` of every function is `Function.prototype.apply` (`AotFacts::apply_builtin`): a closed-world
    /// fact that a forwarding `f.apply(this, arguments)` call's callee is the builtin.
    pub fn apply_is_builtin(&self) -> bool {
        self.apply_builtin
    }

    /// Whether no builtin prototype object (`Array.prototype`, `Object.prototype`) has an index property, of any kind, in the
    /// closed world (`gate::prototype_writes_absent`, with `index_accessors_absent`). The JIT lane reads an unwritten element of
    /// an array allocated in the frame as `undefined` only where this holds: an edge fact, never checked in execution.
    pub fn prototype_indices_empty(&self) -> bool {
        self.prototype_indices_absent
    }

    /// Whether no code can be added to the program (`World::Closed`): no key is live, so a unit that
    /// trusts these facts reads no invalidation word and keeps no exit for them.
    pub fn closed(&self) -> bool {
        self.world == World::Closed
    }

    /// Whether some read of the program may observe the value stored under field key `key` (`readers`). False only
    /// for a field key (`Field`, `AnyField`) that no reachable read reaches and that the world cannot move: its stores
    /// are dead, and its slot need not exist in the layout. A key the world can move has a reader, since code added
    /// later may read it. Every other key is observed (`true`).
    pub fn field_has_reader(&self, key: crate::typed_ir::FactKey) -> bool {
        self.live(key) || self.readers.as_ref().map_or(true, |readers| readers.observes(key))
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

    /// The settled cell of captured slot `slot` of the environment of function `owner` (`FactKey::Slot`): the join of what its
    /// owner and every closure that captures it store, so a read of the slot from an inner function takes the owner's cell.
    /// `Cell::ANY` where the solve did not settle.
    pub fn captured_slot_cell(&self, owner: u32, slot: u16) -> crate::typed_ir::Cell {
        self.cell(crate::typed_ir::FactKey::Slot { owner, slot }).unwrap_or(crate::typed_ir::Cell::ANY)
    }

    /// The keys of every element class `key`'s class shares (`element_classes`): a store through any array of the class
    /// moves all of them, so an edge retires them together. Empty where `key` is no element class.
    pub fn element_class_keys(&self, key: crate::typed_ir::FactKey) -> Box<[crate::typed_ir::FactKey]> {
        self.element_classes.get(&key).cloned().unwrap_or_default()
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
    /// object of unknown class receives another name (`class_layout`); None otherwise. A loader's guess of the prototype's
    /// layout reads it too (`aot_image::symbols`).
    pub fn prototype_layout(&self, class: u32) -> Option<&[u32]> {
        self.layouts.prototypes.get(&class).map(|fields| &**fields)
    }

    /// The union layout of class `class`'s instances (`class_layout::unions_of`): every named field an instance may hold, in
    /// slot order, the constructor's own fields first. Each field carries its settled cell and what an instance with no own
    /// field of the name reads (`FieldDefault`). A store outside the union departs the instance, so MEM's absent fields are
    /// pre-declared deleted slots, one per union field. Proven only in a closed world whose instances no code enumerates
    /// (for-in, `Object.keys`, a `JSON` walk, a spread, `Reflect.ownKeys`) and that no computed-key store or delete reaches;
    /// None otherwise.
    pub fn class_union_layout(&self, class: u32) -> Option<UnionLayout> {
        self.unions.get(&class).cloned()
    }

    /// Whether two element classes are apart: no link of the program joins them, so an array of one class is never an array
    /// of the other (`element_classes`, each class's listed members). A class no link names is apart from every other.
    /// False where the solve did not settle, so no two classes are taken apart from nothing.
    pub fn element_classes_distinct(&self, first: ElementClass, second: ElementClass) -> bool {
        self.signatures.is_some() && first != second
            && !self.element_classes.get(&crate::typed_ir::FactKey::Elements(first)).is_some_and(|members| members.contains(&crate::typed_ir::FactKey::Elements(second)))
    }

    /// The classes some construction (`new`) always constructs (`construct_classes`).
    pub fn constructed_classes(&self) -> std::collections::BTreeSet<u32> {
        self.construct_classes.values().copied().collect()
    }

    /// The facts of function `function`, for its lowering.
    pub fn function_facts(&self, function: u32) -> FunctionFacts<'_> {
        FunctionFacts { facts: self, function }
    }
}
