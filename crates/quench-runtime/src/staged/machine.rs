//! The primitive set of a machine that executes operators: constants, integer and float
//! arithmetic, comparison flags, select, conversions and the trap exit. Rules written over it
//! (`staged::wasm`, `staged::wasm_float`) hold the semantics once; a machine only supplies
//! the operators.

use super::wasm::Width;
use super::wasm_memory::Access;
use super::wasm_simd::{Lanes, Native};
use super::wasm_float::{Float, Truncation};
use crate::wasm::V128_BYTES;
use crate::{WasmTrap, WasmType};
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::float::{F64BinaryOperator as FloatBinary, F64UnaryOperator as FloatUnary};
use crate::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};

/// The primitives of a machine that runs integer, float and conversion operators: what a stage
/// computes without a trap. The interpreter runs them on machine words (`ConcreteWasm`), a
/// specializer on its own values (`Emitter`, `vm/compiled/emit_machine.rs`).
pub(crate) trait MachineStage {
    /// A `width` integer; a comparison's flag is an `I32` word holding 0 or 1.
    type Word: Copy;
    /// What a trap leaves with: the trap for the interpreter; nothing for a specializer, whose
    /// code branches to the trap's exit.
    type Exit;
    fn constant(&mut self, width: Width, value: i64) -> Self::Word;
    /// `operator` on `width` operands; a comparison yields a flag. A division's divisor is
    /// nonzero, and a signed division's quotient fits (`integer_binary` has trapped otherwise).
    fn operate(
        &mut self,
        operator: Binary,
        width: Width,
        left: Self::Word,
        right: Self::Word,
    ) -> Self::Word;
    fn operate_unary(&mut self, operator: Unary, width: Width, value: Self::Word) -> Self::Word;
    /// A `float` operator on `float` operands; a comparison yields a flag. The extrema are the
    /// machine's (signed zeros ordered; a NaN operand's result is `float_binary`'s to replace).
    fn operate_float(
        &mut self,
        operator: FloatBinary,
        float: Float,
        left: Self::Word,
        right: Self::Word,
    ) -> Self::Word;
    /// A float unary operator; rounding and square root quiet a signaling NaN.
    fn operate_float_unary(
        &mut self,
        operator: FloatUnary,
        float: Float,
        value: Self::Word,
    ) -> Self::Word;
    /// The flag that `left` or `right` is a NaN.
    fn float_unordered(&mut self, float: Float, left: Self::Word, right: Self::Word) -> Self::Word;
    /// The float with IEEE bits `bits`.
    fn float_constant(&mut self, float: Float, bits: u64) -> Self::Word;
    /// `taken` where `flag` is 1, `other` where it is 0.
    fn select(&mut self, flag: Self::Word, taken: Self::Word, other: Self::Word) -> Self::Word;
    /// The flag that the float `value` truncates outside the integers of `truncation`.
    fn truncation_overflows(&mut self, truncation: Truncation, value: Self::Word) -> Self::Word;
    /// `operator` on a source-typed `value`; a trapping truncation's operand is in range
    /// (`convert` has trapped otherwise) and the machine truncates it.
    fn convert(&mut self, operator: ScalarConversionOperator, value: Self::Word) -> Self::Word;
    /// Continues where `flag` is 0; traps where it is 1.
    fn trap_when(&mut self, flag: Self::Word, trap: WasmTrap) -> Result<(), Self::Exit>;
}

/// A machine that addresses memory and calls leaves: loads and stores of a word, or of an
/// `Access`'s bytes, at an address with the flags the caller proved (`Flags`), and a call of a
/// leaf that takes and returns words. Open to extension: the rules over it name no layout.
pub(crate) trait MachineAccess: MachineStage {
    /// What an access carries beside its address: its alias region and whether it may trap.
    type Flags: Copy;
    /// The `access` at `address`, narrow bytes extended as the access asks, in its type.
    fn load_access(&mut self, access: Access, flags: Self::Flags, address: Self::Word) -> Self::Word;
    /// The low bytes of `value` as `access` at `address`.
    fn store_access(&mut self, access: Access, flags: Self::Flags, address: Self::Word, value: Self::Word);
    /// The `width` word `offset` bytes from `base`.
    fn load_word(&mut self, width: Width, flags: Self::Flags, base: Self::Word, offset: i32) -> Self::Word;
    /// Stores `value`, a word of its own width, `offset` bytes from `base`.
    fn store_word(&mut self, flags: Self::Flags, value: Self::Word, base: Self::Word, offset: i32);
    /// A call of the code at `target` on word `arguments`: its word.
    fn call_words(&mut self, target: Self::Word, arguments: &[Self::Word]) -> Self::Word;
    /// A call of the leaf at address `leaf` on word `arguments`: its word.
    fn call_leaf(&mut self, leaf: usize, arguments: &[Self::Word]) -> Self::Word {
        let target = self.constant(Width::I64, leaf as i64);
        self.call_words(target, arguments)
    }
    /// A function the machine calls directly, linked where the code is placed.
    type Function: Copy;
    /// A call of `target` on `arguments`: all of its results.
    fn call_function(&mut self, target: Self::Function, arguments: &[Self::Word]) -> Vec<Self::Word>;
    /// What a call's signature is: its parameter and result machine types.
    type Signature: Copy;
    /// A stack slot a caller spills words into.
    type Slot: Copy;
    /// The address `offset` bytes into `slot`.
    fn slot_address(&mut self, slot: Self::Slot, offset: i32) -> Self::Word;
    /// A call of the code at `target` under `signature`: all of its results.
    fn call_signature(&mut self, signature: Self::Signature, target: Self::Word, arguments: &[Self::Word]) -> Vec<Self::Word>;
}

/// A machine with blocks: the transfers between them, each carrying words as the target's
/// parameters.
pub(crate) trait MachineFlow: MachineStage {
    type Block: Copy;
    /// Continues at `block` with `arguments` as its parameters.
    fn goto(&mut self, block: Self::Block, arguments: &[Self::Word]);
    /// What a jump table is: the targets a selector indexes, the last being the default.
    type Table: Copy;
    /// Continues at the target of `table` that `selector` indexes.
    fn dispatch(&mut self, selector: Self::Word, table: Self::Table);
    /// Leaves the function with `values`.
    fn leave(&mut self, values: &[Self::Word]);
    /// Continues at `then` where `flag` is nonzero, else at `otherwise`, each with its arguments.
    fn branch_if(
        &mut self,
        flag: Self::Word,
        then: (Self::Block, &[Self::Word]),
        otherwise: (Self::Block, &[Self::Word]),
    );
}

/// A machine with vector registers: lane-wise operations by a (lane shape, operator) row, views
/// of a vector's bits in a lane shape, constants, shuffles, and moves through memory and stack
/// words. A vector is a `Word` of the machine.
pub(crate) trait MachineVector: MachineAccess {
    /// The row's operation on `left` (and `right`, a vector or a scalar shift count).
    fn lane_operation(&mut self, native: Native, left: Self::Word, right: Option<Self::Word>) -> Self::Word;
    /// `value`, a vector, with the lane shape `shape`: the same bits.
    fn vector_view(&mut self, shape: Lanes, value: Self::Word) -> Self::Word;
    /// The vector whose bits are `bits`.
    fn vector_immediate(&mut self, bits: u128) -> Self::Word;
    /// The vector of the byte `lanes` selected from the concatenation of `left` and `right`.
    fn vector_shuffle_lanes(&mut self, lanes: [u8; V128_BYTES], left: Self::Word, right: Self::Word) -> Self::Word;
    /// The vector at `address`.
    fn load_vector(&mut self, flags: Self::Flags, address: Self::Word) -> Self::Word;
    /// An operand into the two words of `slot` at word position `at`: a vector whole, a scalar
    /// in the low word.
    fn spill_operand(&mut self, slot: Self::Slot, at: usize, value: Self::Word);
    /// The value of type `ty` the word position `at` of `slot` holds.
    fn reload_result(&mut self, slot: Self::Slot, at: usize, ty: WasmType) -> Self::Word;
}
