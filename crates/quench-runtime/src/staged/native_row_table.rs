//! The natives the interpreter runs in place by identity, one line each: the row and the arguments
//! it reads after its receiver. Which `Native` a row serves is stated beside it (`native_row::native_row_of`).

/// A native run in place by identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NativeRow {
    /// `String.prototype.charCodeAt` of a flat string.
    CharCodeAt,
    /// `String.prototype.charAt` of a flat string, at an index of an ASCII unit.
    CharAt,
    /// `Number.isInteger` of a Number.
    IsInteger,
    /// `Math.imul` of two int32 operands.
    Imul,
    /// `Array.prototype.push` of one value onto a dense array (`staged::builtins::array_push_one`).
    Push,
    /// `Array.prototype.pop` of a dense array (`staged::builtins::array_pop`).
    Pop,
}

impl NativeRow {
    /// The arguments the row reads after its receiver: a call passing another count takes the native.
    pub const fn arguments(self) -> u16 {
        match self {
            NativeRow::Pop => 0,
            NativeRow::CharCodeAt | NativeRow::CharAt | NativeRow::IsInteger | NativeRow::Push => 1,
            NativeRow::Imul => 2,
        }
    }
}
