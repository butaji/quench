//! The machine primitives a specializer's stage is written from: typed memory access, integer
//! and float arithmetic and comparison by an operation row, constants, conversions, and control
//! transfer. They carry no layout or language meaning; every `Stage` operation of a
//! specializer is these primitives applied to the layout `staged` states, and `Emit` implements
//! them in `vm/compiled/emit_machine.rs`, the one place that builds instructions by hand.

/// The width and kind of a machine value or memory cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lane { I8, I16, I32, I64, F32, F64 }

/// How a load narrower than its result extends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Extend { Zero, Sign }

/// A two-operand arithmetic row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Binary { Add, Sub, Mul, And, Or, ShiftRightUnsigned, ShiftRightSigned, RemainderSigned, FloatMax, FloatMin }

/// A comparison row; the integer relations test the operands at their own width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Relation { Equal, NotEqual, UnsignedLess, UnsignedGreater, SignedLess, FloatUnordered }

/// A one-operand row: a conversion to the lane it names, or a float rounding or negation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unary { ZeroExtend(Lane), Narrow(Lane), Promote, Demote, FromSigned(Lane), FromUnsigned(Lane), ToUnsignedSaturating(Lane), Bitcast(Lane), Nearest, FloatNegate }

/// The primitives of a stage that emits machine code.
pub(crate) trait MachineStage {
    type Value: Copy;
    /// What an access's alias region and mutability compile to.
    type Flags: Copy;
    type Block: Copy;
    type Signature: Copy;
    /// The lane `value` holds.
    fn raw_lane(&self, value: Self::Value) -> Lane;
    fn raw_load(&mut self, lane: Lane, flags: Self::Flags, base: Self::Value, offset: i32) -> Self::Value;
    /// A load of `from` extended: a zero extension answers a full word, a sign extension an int32.
    fn raw_load_extended(&mut self, from: Lane, extend: Extend, flags: Self::Flags, base: Self::Value, offset: i32) -> Self::Value;
    /// A store of the low `lane` bits of `value`; `private` marks one no other thread observes, which
    /// the backend may defer to an exit.
    fn raw_store(&mut self, lane: Lane, flags: Self::Flags, (value, private): (Self::Value, bool), base: Self::Value, offset: i32);
    fn raw_const(&mut self, lane: Lane, bits: i64) -> Self::Value;
    fn raw_float_const(&mut self, value: f64) -> Self::Value;
    fn raw_binary(&mut self, operation: Binary, a: Self::Value, b: Self::Value) -> Self::Value;
    fn raw_compare(&mut self, relation: Relation, a: Self::Value, b: Self::Value) -> Self::Value;
    /// `raw_compare` against the immediate `bits` at `a`'s own width.
    fn raw_compare_imm(&mut self, relation: Relation, a: Self::Value, bits: i64) -> Self::Value;
    fn raw_select(&mut self, cond: Self::Value, then: Self::Value, otherwise: Self::Value) -> Self::Value;
    fn raw_unary(&mut self, operation: Unary, a: Self::Value) -> Self::Value;
    fn raw_jump(&mut self, block: Self::Block, args: &[Self::Value]);
    fn raw_branch(&mut self, cond: Self::Value, then: (Self::Block, &[Self::Value]), otherwise: (Self::Block, &[Self::Value]));
    /// The results of calling the address `entry` as `signature`.
    fn raw_call(&mut self, signature: Self::Signature, entry: Self::Value, args: &[Self::Value]) -> Vec<Self::Value>;
    #[cfg(feature = "aot")]
    fn raw_return(&mut self, values: &[Self::Value]);
}
