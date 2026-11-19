//! The register frame every staged operation runs on, and an operation's outcome: its own file
//! so the b-jit stencil build (`quench-bjit/build.rs`) compiles the same trait the interpreter
//! and the specializers use.

use super::{Allocation, CallOut, Edge, Stage};
use crate::bytecode::Register;
use crate::typed_ir::Type;

/// A constant an instruction materializes: its value and the semantic type a stage that
/// keeps type facts records for it.
#[derive(Clone, Copy)]
pub(crate) struct Constant {
    pub(crate) value: crate::Value,
    pub(crate) ty: Type,
}

impl Constant {
    /// A constant whose type the stage does not keep.
    #[inline(always)]
    pub(crate) const fn untyped(value: crate::Value) -> Self {
        Self { value, ty: Type::ANY }
    }
}

/// A register frame: each register's word and the static fact the stage keeps about it. A
/// stage with no register facts uses `()`.
pub(crate) trait Frame<S: Stage> {
    /// The static fact a stage keeps about a register.
    type Fact: Copy;

    /// The fact `register` currently carries.
    fn fact(&self, register: Register) -> Self::Fact;

    /// The semantic type `register` is proved to have; `None` where the stage keeps none, as the
    /// interpreter does not.
    #[inline(always)]
    fn register_type(&self, _register: Register) -> Option<Type> { None }

    /// `register` holds a value of type `ty` along `edge` of the jump the running step ends in:
    /// a frame that keeps type facts narrows them for that edge's successor.
    #[inline(always)]
    fn refine(&mut self, _register: Register, _ty: Type, _edge: Edge) {}

    /// `register`'s word in the representation its fact names; tagged where it is tagged.
    fn read(&mut self, stage: &mut S, register: Register) -> S::Word;

    /// `register`'s tagged word (`Value::to_bits`), boxing an unboxed representation.
    #[inline(always)]
    fn tagged(&mut self, stage: &mut S, register: Register) -> S::Word {
        self.read(stage, register)
    }

    /// Define `register` with `value`, carrying `fact`.
    fn write(&mut self, stage: &mut S, register: Register, value: S::Word, fact: Self::Fact);

    /// Copy `source` into `destination` with its value and every fact derived from it; the
    /// word copied.
    #[inline(always)]
    fn move_register(&mut self, stage: &mut S, destination: Register, source: Register) -> S::Word {
        let fact = self.fact(source);
        let value = self.read(stage, source);
        self.write(stage, destination, value, fact);
        value
    }

    /// The running activation's `this` word: the bound value, or `Value::DELETED` where the
    /// activation does not hold it.
    fn this(&mut self, stage: &mut S) -> S::Word;

    /// Define `destination` with the activation's `this`, carrying every fact proved of it.
    fn define_this(&mut self, stage: &mut S, destination: Register, this: S::Word);

    /// `register`'s value from `constant`, with the fact the stage keeps of it.
    fn constant(&mut self, stage: &mut S, constant: Constant) -> (S::Word, Self::Fact);

    /// Store `value` at the dense array element `index`, under the proof.
    #[inline(always)]
    fn index_write(&mut self, _stage: &mut S, _index: S::Word, _value: S::Word, _proof: S::Proof) -> Option<()> { None }

    /// The heap value `allocation` makes, defined into `destination`; `None` where the frame
    /// has no allocator for it or the value is the general path's (a due collection).
    #[inline(always)]
    fn allocate_value(&mut self, _stage: &mut S, _destination: Register, _allocation: Allocation, _proof: S::Proof) -> Option<S::Word> { None }

    /// Continue where no interrupt is requested; `None` leaves at the frame's miss edge.
    #[inline(always)]
    fn safepoint(&mut self, _stage: &mut S, _proof: S::Proof) -> Option<()> { Some(()) }

    /// The code's invalidation word (`code_dependencies::Invalidation`), zero while the facts it holds
    /// still hold (`guard::live`); `None` where the frame has none to read.
    fn invalidation(&mut self, _stage: &mut S) -> Option<S::Word> { None }

    /// The active realm's global object word the code folded facts of (`guard::realm`); `None` where the
    /// frame does not hold the realm.
    fn realm_globals(&mut self, _stage: &mut S) -> Option<S::Word> { None }

    /// Counts a taken back edge in the loop's counter (`guard::back_edge`); `None` where the frame keeps
    /// no counter.
    fn count_edge(&mut self, _stage: &mut S) -> Option<()> { None }

    /// The heap's array and object record tables and its collection epoch, where the frame reads
    /// live records (the interpreter's lane); `None` where it does not, leaving a row that needs
    /// them to its call-out.
    #[inline(always)]
    fn heap_records(&mut self, _stage: &mut S) -> Option<(S::Word, S::Word, S::Word)> { None }

    /// The element store's host helper; `None` where the frame has no helper table.
    #[inline(always)]
    fn index_set_helper(&mut self, _stage: &mut S, _base: S::Word, _key: S::Word, _value: S::Word, _proof: S::Proof) -> Option<()> { None }

    /// The VM's field-store hit (`LaneHelper::FieldSet`).
    #[inline(always)]
    fn field_store_miss(&mut self, _stage: &mut S, _object: S::Word, _atom: u32, _site: u16, _value: S::Word, _proof: S::Proof) -> Option<()> { None }

    /// Leave the function with the Number register `register`, in the representation its fact
    /// names; `None` where the frame has no numeric completion.
    #[inline(always)]
    fn ret(&mut self, _stage: &mut S, _register: Register) -> Option<Self::Control> { None }

    /// The activation's environment storage word and the fact its slot reads carry
    /// (`environment::environment_slot`); `None` where the frame holds no environment.
    #[inline(always)]
    fn environment(&mut self, _stage: &mut S) -> Option<(S::Word, Self::Fact)> { None }

    /// Store `value` to environment slot `slot`, or with `initialize` define it as the first value
    /// of its binding (`environment::environment_slot_define`) rather than store to an initialized
    /// one (`environment_slot_store`); `None` where the frame cannot, leaving the binding rules (an
    /// uninitialized or constant binding, a `with` scope) to the host.
    #[inline(always)]
    fn environment_write(&mut self, _stage: &mut S, _slot: u16, _value: S::Word, _initialize: bool) -> Option<()> { None }

    /// The count of the arguments binding in local `slot`, an int32 word; `None` where the
    /// binding is not one only kept values answer.
    #[inline(always)]
    fn arguments_length(&mut self, _stage: &mut S, _slot: u16) -> Option<(S::Word, Self::Fact)> { None }

    /// A rest view's read (`Protocol::RestView`): `rest.length` (`get` false) or `rest[key]`
    /// of the values its activation kept past its first `fixed` parameters; `None` where the
    /// frame keeps none of its own or `key` is no index they hold. The row answers the value and
    /// its runner defines the destination.
    #[inline(always)]
    fn rest_view(&mut self, _stage: &mut S, _get: bool, _fixed: usize, _key: Option<Register>) -> Option<(S::Word, Self::Fact)> { None }

    /// The VM's own operation `operation` (`CallOut`), run in place where a row's words do not
    /// decide it, defining `result` where the operation has one: the word it defined with the
    /// fact the frame keeps of it, or a placeholder word where it defines none. `None` where the
    /// frame has no host, or the operation raised, leaving the instruction to the interpreter.
    #[inline(always)]
    fn vm_step(&mut self, _stage: &mut S, _operation: CallOut, _result: Option<Register>)
        -> Option<(S::Word, Self::Fact)> { None }

    /// Read the capture `slot`, `depth` scopes out (a static operand: the walk's length), with the
    /// fact the frame keeps of it; `root` reads the root environment's projected binding (a
    /// script's global var or lexical) the walk ends at instead of a function's slots. `None`
    /// where the frame cannot (an uninitialized or shared binding, no captures), leaving the
    /// read to the interpreter.
    #[inline(always)]
    fn capture_slot(&mut self, _stage: &mut S, _depth: u16, _slot: u16, _root: bool) -> Option<(S::Word, Self::Fact)> { None }

    /// Store `value` to the capture `slot`, `depth` scopes out; `None` where the binding does
    /// not take the direct store (a `const`, an uninitialized `let`).
    #[inline(always)]
    fn store_capture(&mut self, _stage: &mut S, _depth: u16, _slot: u16, _value: S::Word) -> Option<()> { None }

    /// Where control goes: the edge the interpreter returns, or the branch a specializer
    /// emits.
    type Control;

    /// ToBoolean of `register` from the frame's facts; `None` where a value's heap cell
    /// decides it, leaving the test to the interpreter's fallback.
    fn truthiness(&mut self, stage: &mut S, register: Register) -> Option<S::Cond>;

    /// Continue at the jump target when `condition` holds, else after this instruction.
    fn branch(&mut self, stage: &mut S, condition: S::Cond) -> Self::Control;

    /// Continue at the jump target.
    fn jump(&mut self, stage: &mut S) -> Self::Control;

    /// The canonical boolean `condition`, with the fact this stage keeps of it.
    fn boolean(&mut self, stage: &mut S, condition: S::Cond) -> (S::Word, Self::Fact);

    /// Read local slot `slot` with the fact this stage keeps of it; `None` where this
    /// frame cannot read the activation's locals (the tier then leaves at its exact PC).
    #[inline(always)]
    fn load_local(&mut self, _stage: &mut S, _slot: u16, _miss: Option<S::Miss>)
        -> Option<(S::Word, Self::Fact)> { None }

    /// Write `value` to local slot `slot`; `false` where this frame cannot write them.
    #[inline(always)]
    fn store_local(&mut self, _stage: &mut S, _slot: u16, _value: S::Word, _miss: Option<S::Miss>)
        -> bool { false }
}

/// A conditional jump on the register's truthiness, taken when it is `taken`; unresolved where
/// the frame cannot tell its truthiness.
#[inline(always)]
pub(crate) fn jump_if<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, register: Register, taken: bool) -> Step<S::Word, F::Control> {
    let Some(condition) = frame.truthiness(stage, register) else { return Step::Unresolved };
    let condition = if taken { condition } else { stage.not(condition) };
    Step::Control(frame.branch(stage, condition))
}

/// `destination` defined with the logical negation of `source`'s truthiness.
#[inline(always)]
pub(crate) fn logical_not<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, destination: Register, source: Register) -> Step<S::Word, F::Control> {
    let Some(truthy) = frame.truthiness(stage, source) else { return Step::Unresolved };
    let falsy = stage.not(truthy);
    let (value, fact) = frame.boolean(stage, falsy);
    frame.write(stage, destination, value, fact);
    Step::Value(value)
}

/// The activation's `this` word where it holds one; an unbound `this` leaves at `miss`.
#[inline(always)]
pub(crate) fn bound_this<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, miss: S::Miss) -> Option<S::Word> {
    let this = frame.this(stage);
    let bound = stage.ne_imm(this, crate::Value::DELETED.to_bits());
    stage.guard(bound, miss)?;
    Some(this)
}

/// `destination` defined with the activation's `this`, where it holds one (not the deleted
/// marker); `miss` where it does not.
#[inline(always)]
pub(crate) fn load_this<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, destination: Register, miss: S::Miss) -> Step<S::Word, F::Control> {
    match bound_this(stage, frame, miss) {
        Some(this) => {
            frame.define_this(stage, destination, this);
            Step::Value(this)
        }
        None => Step::Unresolved,
    }
}

/// `destination` defined from local slot `slot`; `None` where the frame cannot read it.
#[inline(always)]
pub(crate) fn read_local<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, destination: Register, slot: u16, miss: Option<S::Miss>) -> Option<S::Word> {
    let (value, fact) = frame.load_local(stage, slot, miss)?;
    frame.write(stage, destination, value, fact);
    Some(value)
}

/// `source`'s tagged word written to local slot `slot`; `None` where the frame cannot write it.
#[inline(always)]
pub(crate) fn write_local<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, source: Register, slot: u16, miss: Option<S::Miss>) -> Option<S::Word> {
    let value = frame.tagged(stage, source);
    frame.store_local(stage, slot, value, miss).then_some(value)
}

/// `destination` defined from local slot `slot` where its binding is initialized (not the deleted
/// marker, which a binding in its temporal dead zone holds); `None` where the frame cannot read the
/// slot, and `miss` where the binding is uninitialized: the host's read raises the `ReferenceError`.
#[inline(always)]
pub(crate) fn read_initialized_local<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, destination: Register, slot: u16, miss: S::Miss) -> Option<S::Word> {
    let (value, fact) = frame.load_local(stage, slot, Some(miss))?;
    let initialized = stage.ne_imm(value, crate::Value::DELETED.to_bits());
    stage.guard(initialized, miss)?;
    frame.write(stage, destination, value, fact);
    Some(value)
}

/// `source`'s tagged word written to local slot `slot` where its binding is initialized: an assignment
/// that is no initialization (`let x; x = 1`) of a binding still in its temporal dead zone is the
/// host's `ReferenceError`, which `miss` leaves to.
#[inline(always)]
pub(crate) fn write_initialized_local<S: Stage, F: Frame<S>>(stage: &mut S, frame: &mut F, source: Register, slot: u16, miss: S::Miss) -> Option<S::Word> {
    let (current, _) = frame.load_local(stage, slot, Some(miss))?;
    let initialized = stage.ne_imm(current, crate::Value::DELETED.to_bits());
    stage.guard(initialized, miss)?;
    write_local(stage, frame, source, slot, Some(miss))
}

/// A staged instruction's outcome: the word it produced, or where control goes.
pub(crate) enum Step<W, C> {
    /// The operation produced `W`; control falls through.
    Value(W),
    /// The operation transferred control to `C`.
    Control(C),
    /// The operation left at its miss edge (a failed guard), or a value's heap cell decides
    /// its truthiness: the interpreter must take its fallback path.
    Unresolved,
}
