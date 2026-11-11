//! The row-builder interface of the handler table (handler-table-schema.md, "RowBuilder trait"):
//! one generic row body lowers through UltraJIT, FastJIT (`StencilBuilder`) and the interpreter.
//! The vocabulary types below mirror the schema; they move to the shared crate when the table
//! lands.

/// Machine type of a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    I32,
    I64,
    F64,
    /// A tagged `Value` word.
    Word,
}

/// Memory region of an access (alias class).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Frame,
    Field,
    Element,
    Shape,
    Context,
    Gc,
}

/// How a load may be treated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemKind {
    Plain,
    /// Readonly, cannot trap, may be hoisted.
    Invariant,
}

/// Side effects of an `op`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effects {
    Pure,
    MayCollect,
}

/// Machine operations a row applies to values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Add,
    Sub,
    And,
    Or,
    Xor,
    Eq,
    Ne,
    SignedLess,
    /// `(sum, overflow)` packed as the sum; `overflow` is read with `Op::Overflow` on the same args.
    Overflow,
    Not,
}

/// An operand known at emit time: a hole in a stencil, a constant elsewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operand {
    Hole(u32),
    Const(i64),
}

/// Where a dynamic value lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source<V> {
    /// A frame slot by byte offset.
    Frame(Operand),
    /// A pinned register: 0 = frame pointer, 1 = context.
    Pinned(u8),
    Value(V),
}

pub trait RowBuilder {
    type V: Copy;
    type Exit: Copy;

    fn read(&mut self, src: Source<Self::V>, ty: Ty) -> Self::V;
    fn write(&mut self, dst: Source<Self::V>, v: Self::V);
    fn operand(&mut self, op: Operand, ty: Ty) -> Self::V;
    fn op(&mut self, o: Op, ty: Ty, args: &[Self::V], eff: Effects) -> Self::V;
    /// Continue when `cond` is nonzero, else leave through `exit`.
    fn guard(&mut self, cond: Self::V, exit: Self::Exit);
    fn load(&mut self, region: Region, base: Self::V, off: Operand, ty: Ty, kind: MemKind) -> Self::V;
    fn store(&mut self, region: Region, base: Self::V, off: Operand, v: Self::V);
    /// An out-of-line block that rejoins with a value.
    fn cold<F: FnOnce(&mut Self) -> Self::V>(&mut self, f: F) -> Self::V;
    /// A chain comparing `key` with each arm's operand, one arm per case, `miss` otherwise.
    fn if_eq_chain<F: FnOnce(&mut Self) -> Self::V>(
        &mut self,
        key: Self::V,
        arms: &[(Operand, &dyn Fn(&mut Self) -> Self::V)],
        miss: F,
    ) -> Self::V;
    fn next(&mut self);
    /// Continue at the branch target when `cond` is nonzero, else at the next row.
    fn branch(&mut self, cond: Self::V);
}
