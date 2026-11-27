//! What an operation reads, writes and may do, as data (Stage v2): the alias regions a native
//! load or store belongs to, the effect row of an `Operation`. A
//! specializer reads them to keep facts across operations (a fact about a region survives
//! until an operation that writes the region), and the interpreter ignores them.


use super::{Allocation, CallOperation, Operation};
pub use super::region::Region;
use crate::typed_ir::ElementClass;

/// The most element classes a row names before its element writes are `ElementWrites::Any`: the classes a
/// composed signature keeps apart, the same bound as a closed union of receiver classes (`typed_ir::MAX_CLASSES`).
pub const MAX_WRITE_CLASSES: usize = 4;

/// An affine index over the parameters of a function: the parameter `param` (its value on entry) plus a constant, or
/// the constant alone where `param` is `None`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Affine {
    pub param: Option<u16>,
    pub offset: i64,
}

impl Affine {
    /// Parameter `index`'s value on entry.
    pub const fn parameter(index: u16) -> Self {
        Self { param: Some(index), offset: 0 }
    }

    /// The constant `offset`.
    pub const fn constant(offset: i64) -> Self {
        Self { param: None, offset }
    }

    /// This index as a caller sees it: each parameter `p` names the caller's parameter `parameter(p)` (the argument's
    /// own parameter), or nothing where the argument is no parameter of the caller. None where a parameter names nothing.
    fn substitute(self, parameter: &dyn Fn(u16) -> Option<u16>) -> Option<Self> {
        match self.param {
            None => Some(self),
            Some(index) => parameter(index).map(|param| Self { param: Some(param), offset: self.offset }),
        }
    }
}

/// Which indices of an array an element write touches: any index (`Whole`), or the indices `start .. start + len`
/// with both bounds affine over the function's parameters. A `Span` is a superset of what its write touches, so a
/// store the solve cannot bound is `Whole`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Indices {
    Whole,
    Span { start: Affine, len: Affine },
}

impl Indices {
    /// The indices either names: equal spans stay, any other pair is `Whole`.
    pub fn join(self, other: Self) -> Self {
        if self == other { self } else { Self::Whole }
    }

    /// The indices both name, where a bound is known: `Whole` gives way to the other, and two different spans, which
    /// have no common span, stay `Whole` (a sound superset of both).
    pub fn meet(self, other: Self) -> Self {
        match (self, other) {
            (Self::Whole, rest) | (rest, Self::Whole) => rest,
            _ => self.join(other),
        }
    }

    /// The indices as a caller sees them (`Affine::substitute`): `Whole` where a parameter names no caller parameter.
    pub fn substitute(self, parameter: &dyn Fn(u16) -> Option<u16>) -> Self {
        match self {
            Self::Whole => Self::Whole,
            Self::Span { start, len } => match (start.substitute(parameter), len.substitute(parameter)) {
                (Some(start), Some(len)) => Self::Span { start, len },
                _ => Self::Whole,
            },
        }
    }
}

/// The indices of one array class an element write touches.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClassWrite {
    pub class: ElementClass,
    pub indices: Indices,
}

impl ClassWrite {
    const NONE: Self = Self { class: ElementClass::Unnamed, indices: Indices::Whole };
}

/// Which array elements a row writes, by the class of the array (`ElementClass`): nothing (bottom), up to
/// `MAX_WRITE_CLASSES` classes each with its indices, or any element of any array (top). The classes are sorted and
/// distinct, and `Classes` holds at least one, so equal writes are equal values.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ElementWrites {
    #[default]
    Nothing,
    Classes { len: u8, writes: [ClassWrite; MAX_WRITE_CLASSES] },
    Any,
}

impl ElementWrites {
    /// The writes of `entries`: a class listed twice joins its indices, and more classes than `MAX_WRITE_CLASSES` is `Any`.
    pub fn of(entries: impl IntoIterator<Item = ClassWrite>) -> Self {
        let mut writes = [ClassWrite::NONE; MAX_WRITE_CLASSES];
        let mut len = 0;
        for entry in entries {
            match writes[..len].iter().position(|write| write.class == entry.class) {
                Some(at) => writes[at].indices = writes[at].indices.join(entry.indices),
                None if len == MAX_WRITE_CLASSES => return Self::Any,
                None => {
                    writes[len] = entry;
                    len += 1;
                }
            }
        }
        if len == 0 {
            return Self::Nothing;
        }
        writes[..len].sort_by_key(|write| write.class);
        Self::Classes { len: len as u8, writes }
    }

    /// The listed classes, none for `Nothing` or `Any`.
    pub fn entries(&self) -> &[ClassWrite] {
        match self {
            Self::Classes { len, writes } => &writes[..usize::from(*len)],
            Self::Nothing | Self::Any => &[],
        }
    }

    /// The writes either row makes.
    pub fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Any, _) | (_, Self::Any) => Self::Any,
            (Self::Nothing, rest) | (rest, Self::Nothing) => rest,
            (Self::Classes { .. }, Self::Classes { .. }) => Self::of(self.entries().iter().chain(other.entries()).copied()),
        }
    }

    /// The writes both rows allow: a class only one lists is not written, and a class both list keeps the indices both
    /// name (`Indices::meet`). A kind row's `Any` gives way to the facts' writes.
    pub fn meet(self, other: Self) -> Self {
        match (self, other) {
            (Self::Nothing, _) | (_, Self::Nothing) => Self::Nothing,
            (Self::Any, rest) | (rest, Self::Any) => rest,
            (Self::Classes { .. }, Self::Classes { .. }) => Self::of(self.entries().iter().filter_map(|write| {
                other.entries().iter().find(|kept| kept.class == write.class)
                    .map(|kept| ClassWrite { class: write.class, indices: write.indices.meet(kept.indices) })
            })),
        }
    }

    /// The writes with each class and index `lift` maps to the caller's: a callee's writes seen from a call site.
    pub fn map(self, lift: impl Fn(ClassWrite) -> ClassWrite) -> Self {
        match self {
            Self::Classes { .. } => Self::of(self.entries().iter().map(|&write| lift(write))),
            other => other,
        }
    }
}

/// A set of regions, one bit each, so a fact's invalidation is a mask intersection.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RegionSet(pub u16);

impl RegionSet {
    pub const NONE: Self = Self(0);

    pub const fn of(region: Region) -> Self {
        Self(1 << region as u16)
    }

    pub const fn with(self, region: Region) -> Self {
        Self(self.0 | 1 << region as u16)
    }

    pub const fn contains(self, region: Region) -> bool {
        self.0 & 1 << region as u16 != 0
    }

    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

/// An operation's effect row. `allocates` is the one bit for everything that can collect: the collector runs at an
/// allocation's slow path and at a safepoint, so a row that cannot allocate cannot collect.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Effects {
    pub reads: RegionSet,
    pub writes: RegionSet,
    /// The elements of which arrays the row writes (`ElementWrites`). A row that writes the `Elements` region names its
    /// arrays here: `Any` where the kind row cannot name them, narrowed by the settled facts where they can.
    pub elements: ElementWrites,
    /// Whether the row may allocate, which is also whether it may collect. A throw allocates its error object, a
    /// Number or element read or property read may run guest code that allocates, and a safepoint collects.
    pub allocates: bool,
    pub may_call_js: bool,
    pub may_throw: bool,
}

impl Effects {
    pub const NONE: Self = Self { reads: RegionSet::NONE, writes: RegionSet::NONE, elements: ElementWrites::Nothing,
        allocates: false, may_call_js: false, may_throw: false };

    pub const fn reads(region: Region) -> Self { Self { reads: RegionSet::of(region), ..Self::NONE } }

    pub const fn writes(region: Region) -> Self { Self { writes: RegionSet::of(region), ..Self::NONE } }

    /// An operation that may run guest code or allocate: it reads and writes every region. The top of every grade.
    pub const ANY: Self = Self { reads: RegionSet(!0), writes: RegionSet(!0), elements: ElementWrites::Any, allocates: true,
        may_call_js: true, may_throw: true };

    /// A row that touches every region and runs nothing: no guest code, throw, collection or allocation. The settled
    /// facts grade a site with it where they prove it cannot leave and allocates nothing (`signatures::facts_grade`).
    /// Met with a kind row (`meet`) it clears those flags and keeps the kind's regions.
    pub(crate) const RUNS_NOTHING: Self = Self { reads: RegionSet(!0), writes: RegionSet(!0), elements: ElementWrites::Any, allocates: false,
        may_call_js: false, may_throw: false };

    /// Whether the row may write `region`.
    pub const fn writes_region(self, region: Region) -> bool {
        self.writes.contains(region)
    }

    /// Whether the row may run guest code, allocate or collect: any of these may move records or change
    /// what a call leaves behind, so a word read across it is not fixed.
    pub const fn may_move_or_call(self) -> bool {
        self.may_call_js || self.allocates
    }

    /// The union of two rows: a region is read or written if either row reads or writes it, and a
    /// flag holds if either does, so a sequence of steps keeps every effect any of them may have.
    pub const fn join(self, other: Self) -> Self {
        Self { reads: RegionSet(self.reads.0 | other.reads.0), writes: RegionSet(self.writes.0 | other.writes.0),
            elements: self.elements.join(other.elements),
            allocates: self.allocates || other.allocates,
            may_call_js: self.may_call_js || other.may_call_js, may_throw: self.may_throw || other.may_throw }
    }

    /// The regions an object's shape, property slots, record words and payload live in: a write to one of them, outside an
    /// allocation (which writes only the new object's words), changes an existing object's exact shape or prototype link.
    const OBJECT_REGIONS: RegionSet = RegionSet::of(Region::ShapeWord).with(Region::ObjectSlots).with(Region::Records)
        .with(Region::Payload).with(Region::HeapMeta);

    /// Whether a row with this effect may change an existing object's exact shape or prototype link: it may run guest code (a
    /// getter, a setter, a proxy trap), or it writes an object's shape, slots or record words without allocating.
    pub const fn reshapes(self) -> bool {
        self.may_call_js || (!self.allocates && self.writes.intersects(Self::OBJECT_REGIONS))
    }

    /// Whether a row with this effect reads and nothing else: it writes no region, runs no guest code and allocates nothing.
    pub const fn reads_alone(self) -> bool {
        self.writes.0 == RegionSet::NONE.0 && matches!(self.elements, ElementWrites::Nothing) && !self.may_call_js && !self.allocates
    }

    /// Whether a row with this effect may rewrite the run's stores word: guest code may run in a nested run and specialize or
    /// unspecialize stores. An allocation runs no guest code, so it clears nothing.
    pub const fn clears_stores(self) -> bool {
        self.may_call_js
    }

    /// The meet of a kind row with a grade: a region is read or written only if both allow it, and a flag holds
    /// only if both do. A grade can clear what its kind row may do and never adds an effect, so a site's effects
    /// are its operation's kind row met with what the settled facts prove of the site: the one place a grade is computed.
    pub(crate) fn meet(self, other: Self) -> Self {
        Self { reads: RegionSet(self.reads.0 & other.reads.0), writes: RegionSet(self.writes.0 & other.writes.0),
            elements: self.elements.meet(other.elements),
            allocates: self.allocates && other.allocates,
            may_call_js: self.may_call_js && other.may_call_js, may_throw: self.may_throw && other.may_throw }
    }

    /// Whether a step with this row can leave the activation: it may throw, run guest code or allocate (collect). A site's
    /// leave is this flag of its graded row (`signatures::site_grade`), so a fact that proves a site cannot leave
    /// clears those flags there, and the row's leave is never decided twice.
    pub(crate) const fn can_leave(self) -> bool {
        self.may_throw || self.may_call_js || self.allocates
    }
}

/// How far a step can take the running activation out of native code (a leave). The lattice is a
/// chain: a function that cannot leave is `Never`; one that can leave only before its first effect
/// is `RestartCall`, so the call starts again from its entry; one that can leave after an effect
/// needs its frame rebuilt from the registers, `Materialize`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Leaves {
    /// No step can leave the activation under the facts.
    Never,
    /// The activation may leave only before its first write or allocation, so the call restarts.
    RestartCall,
    /// A leave needs the frame rebuilt.
    Materialize,
}

impl Leaves {
    /// The larger of the two: the chain's join is its maximum.
    pub const fn join(self, other: Self) -> Self {
        if other as u8 > self as u8 { other } else { self }
    }
}

/// How a step that can leave (`can_leave` of its graded row) leaves, given whether an earlier step of the
/// same function wrote or allocated. Before any
/// write or allocation nothing the call did is visible, so the call restarts from its entry; after one,
/// the frame must be rebuilt. Every tier reads this one rule.
const fn leave_of(can_leave: bool, written_before: bool) -> Leaves {
    match (can_leave, written_before) {
        (false, _) => Leaves::Never,
        (true, false) => Leaves::RestartCall,
        (true, true) => Leaves::Materialize,
    }
}

/// One step of a function's code as the fold sees it: an operation's graded effect row (`signatures::site_grade`),
/// whose leave is its `can_leave`, or a call, which stands for the signature joined over the callees it may reach.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Row { effects: Effects },
    Call(Signature),
}

/// The composed effect signature of a function: the leaves its steps can cause and the effects they
/// have together. Composing steps is Kleisli composition in an effect-graded monad: each step is
/// graded by its row, sequencing joins the rows, and a later step's leave is judged against the
/// writes of the steps before it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Signature {
    pub leaves: Leaves,
    pub effects: Effects,
}

impl Signature {
    /// Bottom: no leave and no effect. A function whose steps no solve has seen yet has it.
    pub const NEVER: Self = Self { leaves: Leaves::Never, effects: Effects::NONE };
    /// Top: any leave and any effect. A call whose targets the facts do not name has it.
    pub const ANY: Self = Self { leaves: Leaves::Materialize, effects: Effects::ANY };

    /// The component-wise join: the larger leave and the union of the effect rows.
    pub fn join(self, other: Self) -> Self {
        Self { leaves: self.leaves.join(other.leaves), effects: self.effects.join(other.effects) }
    }

    /// The signature of `steps` in code order. A call step's leave is its callee's, judged after
    /// the writes of the steps before it: a callee that restarts is only a restart before any write.
    pub fn of_steps(steps: impl IntoIterator<Item = Step>) -> Self {
        let mut signature = Self::NEVER;
        let mut written = false;
        for step in steps {
            let (leaves, effects) = match step {
                Step::Row { effects } => (leave_of(effects.can_leave(), written), effects),
                Step::Call(callee) => {
                    let leaves = match callee.leaves {
                        Leaves::Never => Leaves::Never,
                        _ if written => Leaves::Materialize,
                        leaves => leaves,
                    };
                    (leaves, callee.effects)
                }
            };
            signature = signature.join(Self { leaves, effects });
            written |= effects.writes != RegionSet::NONE || effects.elements != ElementWrites::Nothing || effects.allocates;
        }
        signature
    }
}

/// How a call site is lowered, by one rule in every tier (interpreter, b-jit, u-jit, AOT): an
/// inlinable callee is inlined; a callee that cannot leave the activation, allocate or run guest code
/// needs no frame, so it is called frameless; any other callee is called with a frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CallForm {
    Inline,
    Frameless,
    Framed,
}

/// The call form of a call whose callee has signature `callee`.
pub fn call_form(callee: Signature, inlinable: bool) -> CallForm {
    if inlinable {
        CallForm::Inline
    } else if callee.leaves == Leaves::Never && !callee.effects.allocates && !callee.effects.may_call_js {
        CallForm::Frameless
    } else {
        CallForm::Framed
    }
}

/// Whether a call site is a tail site, by one rule in every tier: the call's completion is its activation's
/// (`returns`: the record carries the return flag, so the callee's value is the caller's result), and the caller
/// has nothing to do after the callee returns (`caller_done`: no handler covers the call, and the caller is
/// strict, which leaves no `with` scope open and no `arguments` object mapped to its parameters). A tier that
/// takes a tail site replaces the caller's frame with the callee's (`CallPlan::tail`), so a chain of tail calls
/// does not grow the stack. The callee's own kind (async or generator) is a separate fact of the callee.
pub const fn tail_site(returns: bool, caller_done: bool) -> bool {
    returns && caller_done
}

impl<W> CallOperation<W> {
    /// The effect row of a call-protocol step: the guards leave and write nothing, the window
    /// writes the callee's registers, and a site names a call, which may run anything.
    const fn effects(&self) -> Effects {
        match self {
            Self::Callee { .. } | Self::Receiver { .. } | Self::BindThis { .. } | Self::Room { .. } | Self::Complete { .. } => Effects::NONE,
            Self::Window { .. } => Effects::writes(Region::Frame),
            Self::Site(_) => Effects::ANY,
        }
    }
}

impl<W, P> Operation<W, P> {
    /// Whether the operation may move an array's backing or the record table: it adds or removes
    /// elements (writes `Region::Elements`), allocates (a new record block) or may run guest code,
    /// which can do either. Native code leaves before a collection. An element read's host arm
    /// answers by a pure lookup or leaves where the VM would run a getter, so a read does not.
    pub fn moves_arrays(&self) -> bool {
        let effects = self.effects();
        !matches!(self, Self::ElementRead { .. }) && (effects.writes.contains(Region::Elements) || effects.allocates || effects.may_call_js)
    }

    /// Whether the operation may create or replace the running activation's environment, which its environment storage word
    /// names: a closure's allocation promotes the activation's captured locals into one (it allocates and writes
    /// `Region::Frame`), and anything that may run any code is conservative. A guard, a read or a store does not.
    pub fn makes_environment(&self) -> bool {
        let effects = self.effects();
        effects.allocates && effects.writes.contains(Region::Frame)
    }

    /// Whether the operation may change an existing object's exact shape or prototype link (`Effects::reshapes`).
    pub fn reshapes(&self) -> bool {
        self.effects().reshapes()
    }

    /// Whether the operation reads and nothing else (`Effects::reads_alone`).
    pub fn reads_alone(&self) -> bool {
        self.effects().reads_alone()
    }

    /// Whether the operation may rewrite the run's stores word (`Effects::clears_stores`).
    pub fn clears_stores(&self) -> bool {
        self.effects().clears_stores()
    }

    /// The effect row of the operation, from its variant alone.
    pub const fn effects(&self) -> Effects {
        match self.memory_effects() {
            Some(effects) => effects,
            None => self.control_effects(),
        }
    }

    /// Effects of the field, element and allocation operations.
    const fn memory_effects(&self) -> Option<Effects> {
        Some(match self {
            Self::IndexWrite { .. } => Effects { elements: ElementWrites::Any, ..Effects::writes(Region::Elements) },
            Self::ElementRead { .. } => Effects { reads: RegionSet::of(Region::Elements), allocates: true, may_call_js: true, may_throw: true, ..Effects::NONE },
            Self::ElementWrite { .. } => Effects::ANY,
            Self::DefineElement { .. } => Effects { elements: ElementWrites::Any, ..Effects::writes(Region::Elements) },
            // A closure promotes the activation's captured locals into its environment.
            Self::Allocate { allocation: Allocation::Closure { .. }, .. } => Effects {
                writes: RegionSet::of(Region::HeapMeta).with(Region::ObjectSlots).with(Region::Frame)
                .with(Region::Records).with(Region::ShapeWord).with(Region::Payload), allocates: true, ..Effects::NONE },
            // A callee-argument regexp literal reads its callee register, and its shared object's
            // `lastIndex` and the proofs it is kept under (`Vm::regexp_literal_argument_of`).
            Self::Allocate { allocation: Allocation::RegExp { callee: Some(_), .. }, .. } => Effects {
                reads: RegionSet::of(Region::Frame).with(Region::ObjectSlots).with(Region::ShapeWord).with(Region::Records),
                writes: RegionSet::of(Region::HeapMeta).with(Region::ObjectSlots)
                .with(Region::Records).with(Region::ShapeWord).with(Region::Payload), allocates: true, ..Effects::NONE },
            Self::Allocate { allocation: Allocation::Empty | Allocation::Array { .. } | Allocation::RegExp { callee: None, .. } | Allocation::ConstArray { .. }, .. } => Effects {
                writes: RegionSet::of(Region::HeapMeta).with(Region::ObjectSlots)
                .with(Region::Records).with(Region::ShapeWord).with(Region::Payload), allocates: true, ..Effects::NONE },
            Self::Allocate { allocation: Allocation::ObjectLiteral { .. } | Allocation::ObjectPair { .. } | Allocation::ArrayOf { .. } | Allocation::Template { .. }, .. } => Effects {
                reads: RegionSet::of(Region::Frame), writes: RegionSet::of(Region::HeapMeta).with(Region::ObjectSlots)
                .with(Region::Records).with(Region::ShapeWord).with(Region::Payload), allocates: true, ..Effects::NONE },
            _ => return self.property_effects(),
        })
    }

    /// Effects of the property operations: a plan's reads and stores touch object slots and
    /// shapes; a host callout may run guest code (a getter, a proxy trap) or reshape objects.
    const fn property_effects(&self) -> Option<Effects> {
        Some(match self {
            Self::GetField { .. } => Effects { reads: RegionSet::of(Region::ObjectSlots).with(Region::ShapeWord)
                .with(Region::Records).with(Region::Payload), allocates: true, may_call_js: true, may_throw: true, ..Effects::NONE },
            Self::SetField { .. } | Self::StoreField { .. } | Self::FieldAdd { .. } | Self::Delete { .. } => Effects::ANY,
            Self::FieldGetHost { .. } | Self::IndexGetHost { .. } => Effects::ANY,
            _ => return None,
        })
    }

    const fn control_effects(&self) -> Effects {
        match self {
            Self::Move { .. } | Self::LoadConst { .. } | Self::LoadThis { .. } => Effects::NONE,
            Self::LoadLocalPlain { .. } => Effects::reads(Region::Frame),
            Self::StoreLocalPlain { .. } => Effects::writes(Region::Frame),
            Self::LoadLocalChecked { .. } => Effects::reads(Region::Frame),
            Self::StoreLocalChecked { .. } | Self::InitializeLocal { .. } => Effects::writes(Region::Frame),
            Self::Jump | Self::JumpIf { .. } | Self::LogicalNot { .. } | Self::Return { .. } => Effects::NONE,
            // A Number operation's miss (an operand that is not a Number: `valueOf`, `ToNumeric`) runs guest code and
            // throws; a site whose operands the facts prove are Numbers grades it to none (`signatures::facts_grade`).
            Self::Number(_) => Effects { allocates: true, may_call_js: true, may_throw: true, ..Effects::NONE },
            Self::CompareJump { comparison, .. } if !comparison.runs_guest_code() => Effects::NONE,
            Self::RequireObjectCoercible { .. } | Self::RequireIteratorResult { .. } => Effects { allocates: true, may_throw: true, ..Effects::NONE },
            Self::Safepoint { .. } => Effects { allocates: true, ..Effects::NONE },
            Self::Call(operation) => operation.effects(),
            // A suspension resumes the activation after anything: an `await` may run guest code
            // through PromiseResolve, and the frame's state is the generator's again.
            #[cfg(not(stencil_build))]
            Self::Suspend { .. } => Effects::ANY,
            // The memory operations answered in `memory_effects`; anything else is conservative.
            _ => Effects::ANY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::Operand;

    #[test]
    fn only_operations_that_store_allocate_or_run_guest_code_move_arrays() {
        type Row = Operation<u64, ()>;
        let stay: [Row; 5] = [Operation::ElementRead { base: 0, key: Operand(1), destination: 0 }, Operation::Move { destination: 0, source: 1 },
            Operation::Jump, Operation::Safepoint { proof: () }, Operation::LoadThis { destination: 0 }];
        assert!(stay.iter().all(|row| !row.moves_arrays()));
        let move_arrays: [Row; 4] = [Operation::ElementWrite { value: 0, base: 1, key: 2, strict: false }, Operation::IndexWrite { index: 0, value: 0, proof: () },
            Operation::Allocate { destination: 0, allocation: Allocation::Empty, proof: () }, Operation::Binary { destination: 0, left: crate::bytecode::Operand::register(1), right: crate::bytecode::Operand::register(2), operator: 0 }];
        assert!(move_arrays.iter().all(Row::moves_arrays));
    }

    #[test]
    fn element_writes_join_by_class_and_widen_past_the_bound() {
        let whole = |class| ClassWrite { class, indices: Indices::Whole };
        let span = |class| ClassWrite { class, indices: Indices::Span { start: Affine::parameter(1), len: Affine::constant(1) } };
        let written = ElementWrites::of([whole(ElementClass::Global(3)), span(ElementClass::Parameter { function: 2, index: 0 })]);
        assert_eq!(written.entries().len(), 2);
        assert_eq!(written.join(ElementWrites::Nothing), written);
        assert_eq!(written.join(ElementWrites::Any), ElementWrites::Any);
        // The same class twice joins its indices: equal spans stay, different ones widen to `Whole`.
        let joined = ElementWrites::of([span(ElementClass::Unnamed), span(ElementClass::Unnamed)]);
        assert_eq!(joined.entries()[0].indices, Indices::Span { start: Affine::parameter(1), len: Affine::constant(1) });
        let widened = ElementWrites::of([span(ElementClass::Unnamed), whole(ElementClass::Unnamed)]);
        assert_eq!(widened.entries()[0].indices, Indices::Whole);
        let many = (0..=MAX_WRITE_CLASSES as u32).map(|function| whole(ElementClass::Return(function)));
        assert_eq!(ElementWrites::of(many), ElementWrites::Any);
        assert_eq!(ElementWrites::of(std::iter::empty()), ElementWrites::Nothing);
    }

    #[test]
    fn a_grade_narrows_element_writes_and_keeps_its_flags() {
        let store = ElementWrites::of([ClassWrite { class: ElementClass::Field(4), indices: Indices::Whole }]);
        let graded = Effects::ANY.meet(Effects { elements: store, ..Effects::ANY });
        assert_eq!(graded.elements, store);
        assert!(graded.may_call_js && graded.may_throw && graded.allocates, "an element store's grade clears no flag");
        assert_eq!(Effects::NONE.meet(Effects::ANY).elements, ElementWrites::Nothing);
    }

    #[test]
    fn a_callee_index_is_read_through_the_arguments_parameters() {
        let index = Indices::Span { start: Affine::parameter(1), len: Affine::constant(1) };
        let kept = index.substitute(&|param: u16| (param == 1).then_some(0_u16));
        assert_eq!(kept, Indices::Span { start: Affine::parameter(0), len: Affine::constant(1) });
        assert_eq!(index.substitute(&|_: u16| None::<u16>), Indices::Whole, "an argument that is no parameter of the caller widens the index");
    }

    #[test]
    fn a_returning_call_with_nothing_after_it_is_a_tail_site() {
        assert!(tail_site(true, true));
        assert!(!tail_site(false, true), "a call whose value the caller keeps working on is not a tail site");
        assert!(!tail_site(true, false), "a handler or a sloppy caller must still see the call's completion");
    }
}
