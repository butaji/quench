//! The primitives of a machine a specializer emits for, one set for every backend. `Machine` names
//! the word; `MachineScalar`, `MachineAccess`, `MachineFlow`, `MachineTail` and `MachineVector` are the
//! operations a stage is composed from (constants, arithmetic by op row, typed memory, control
//! transfer, vector views) and carry no layout or language meaning; `MachineStage` is the Wasm
//! operator layer the interpreter (`ConcreteWasm`) and a specializer both run. Rules written over
//! them (`staged::wasm`, `staged::wasm_float`) hold the semantics once; a machine only supplies
//! the operators. The op rows are `machine/rows.rs`; `vm/compiled/emit_machine.rs` implements the
//! traits as Cranelift instructions, the one place that builds them by hand.

// The stencil build includes this file by path, where nested modules resolve from `staged/`.
#[cfg_attr(stencil_build, path = "machine/rows.rs")]
mod rows;

pub(crate) use rows::{Binary, Extend, Lane, MachineOperator, Relation, Unary};

use super::wasm::Width;
#[cfg(not(stencil_build))]
use super::wasm_memory::Access;
#[cfg(not(stencil_build))]
use super::wasm_simd::{Lanes, Native};
use super::wasm_float::{Float, Truncation};
#[cfg(not(stencil_build))]
use crate::wasm::V128_BYTES;
use crate::WasmTrap;
#[cfg(not(stencil_build))]
use crate::WasmType;
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::float::{F64BinaryOperator as FloatBinary, F64UnaryOperator as FloatUnary};
use crate::wasm::integer::{I64BinaryOperator as IntegerBinary, I64UnaryOperator as IntegerUnary};

/// What every machine has: a word, an integer or float of some `Lane`, or a vector. A comparison's
/// flag is a word that holds 0 or 1.
pub(crate) trait Machine {
    type Word: Copy;
}

/// The Wasm operator layer of a machine that runs integer, float and conversion operators: what a
/// stage computes without a trap. The interpreter runs them on machine words (`ConcreteWasm`), a
/// specializer composes them from its primitives (`Emitter`, `vm/compiled/emit_machine/scalar.rs`).
pub(crate) trait MachineStage: Machine {
    /// What a trap leaves with: the trap for the interpreter; nothing for a specializer, whose
    /// code branches to the trap's exit.
    type Exit;
    fn constant(&mut self, width: Width, value: i64) -> Self::Word;
    /// `operator` on `width` operands; a comparison yields an `I32` flag word. A division's divisor
    /// is nonzero, and a signed division's quotient fits (`integer_binary` has trapped otherwise).
    fn operate(
        &mut self,
        operator: IntegerBinary,
        width: Width,
        left: Self::Word,
        right: Self::Word,
    ) -> Self::Word;
    fn operate_unary(&mut self, operator: IntegerUnary, width: Width, value: Self::Word) -> Self::Word;
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

/// The scalar primitives: constants, arithmetic, comparison, select and conversion by op row.
pub(crate) trait MachineScalar: Machine {
    /// The lane `value` holds.
    fn raw_lane(&mut self, value: Self::Word) -> Lane;
    /// The `lane` constant whose bits are `bits`; a float lane reads them as its IEEE bits.
    fn raw_const(&mut self, lane: Lane, bits: i64) -> Self::Word;
    fn raw_binary(&mut self, operation: Binary, a: Self::Word, b: Self::Word) -> Self::Word;
    /// A signed `Add`, `Sub` or `Mul` of two integers: its wrapped result and the flag that it overflowed.
    fn raw_binary_overflow(&mut self, operation: Binary, a: Self::Word, b: Self::Word) -> (Self::Word, Self::Word);
    fn raw_compare(&mut self, relation: Relation, a: Self::Word, b: Self::Word) -> Self::Word;
    /// `raw_compare` against the immediate `bits` at `a`'s own width.
    fn raw_compare_imm(&mut self, relation: Relation, a: Self::Word, bits: i64) -> Self::Word;
    /// `raw_binary` with the immediate `bits` at `a`'s own lane.
    fn raw_binary_imm(&mut self, operation: Binary, a: Self::Word, bits: i64) -> Self::Word {
        let lane = self.raw_lane(a);
        let b = self.raw_const(lane, bits);
        self.raw_binary(operation, a, b)
    }
    fn raw_select(&mut self, cond: Self::Word, then: Self::Word, otherwise: Self::Word) -> Self::Word;
    fn raw_unary(&mut self, operation: Unary, a: Self::Word) -> Self::Word;
}

/// A machine that addresses memory and calls code: loads and stores of a lane, or of an `Access`'s
/// bytes, at an address with the flags the caller proved (`Flags`), and calls of code that takes
// The stencil build runs the operator layer only; access, flow and vectors are the backends'.
#[cfg(not(stencil_build))]
/// and returns words. Open to extension: the rules over it name no layout.
pub(crate) trait MachineAccess: MachineScalar {
    /// What an access carries beside its address: its alias region and whether it may trap.
    type Flags: Copy;
    /// The `lane` `offset` bytes from `base`.
    fn raw_load(&mut self, lane: Lane, flags: Self::Flags, base: Self::Word, offset: i32) -> Self::Word;
    /// The `from` cell `offset` bytes from `base` extended into the lane `to`.
    fn raw_load_extended(&mut self, from: Lane, extend: Extend, to: Lane, flags: Self::Flags, base: Self::Word, offset: i32) -> Self::Word;
    /// A store of the low `lane` bits of `stored.0`; `stored.1` marks one no other thread observes, which
    /// the backend may defer to an exit.
    fn raw_store(&mut self, lane: Lane, flags: Self::Flags, stored: (Self::Word, bool), base: Self::Word, offset: i32);
    /// The `access` at `address`, narrow bytes extended as the access asks, in its type.
    fn load_access(&mut self, access: Access, flags: Self::Flags, address: Self::Word) -> Self::Word;
    /// The low bytes of `value` as `access` at `address`.
    fn store_access(&mut self, access: Access, flags: Self::Flags, address: Self::Word, value: Self::Word);
    /// An atomic load of the `lane` that `base` addresses: neither merged nor dropped.
    fn raw_atomic_load(&mut self, lane: Lane, flags: Self::Flags, base: Self::Word) -> Self::Word;
    /// The stack pointer.
    fn stack_pointer(&mut self) -> Self::Word;
    /// The `width` word `offset` bytes from `base`.
    fn load_word(&mut self, width: Width, flags: Self::Flags, base: Self::Word, offset: i32) -> Self::Word {
        self.raw_load(width.into(), flags, base, offset)
    }
    /// Stores `value`, a word of its own lane, `offset` bytes from `base`.
    fn store_word(&mut self, flags: Self::Flags, value: Self::Word, base: Self::Word, offset: i32) {
        let lane = self.raw_lane(value);
        self.raw_store(lane, flags, (value, false), base, offset);
    }
    /// A call of the code at `target` on word `arguments`: its word.
    fn call_words(&mut self, target: Self::Word, arguments: &[Self::Word]) -> Self::Word;
    /// A call of the code at `target` whose `signature` is its parameter lanes and its result lane.
    fn call_lanes(&mut self, target: Self::Word, signature: (&[Lane], Lane), arguments: &[Self::Word]) -> Self::Word;
    /// A call of the leaf at address `leaf` on word `arguments`: its word.
    fn call_leaf(&mut self, leaf: usize, arguments: &[Self::Word]) -> Self::Word {
        let target = self.raw_const(Lane::I64, leaf as i64);
        self.call_words(target, arguments)
    }
    /// A call of the leaf at address `leaf` under `signature` (parameter lanes and result lane).
    fn call_leaf_lanes(&mut self, leaf: usize, signature: (&[Lane], Lane), arguments: &[Self::Word]) -> Self::Word {
        let target = self.raw_const(Lane::I64, leaf as i64);
        self.call_lanes(target, signature, arguments)
    }
    /// A function the machine calls directly, linked where the code is placed.
    type Function: Copy;
    /// A call of `target` on `arguments`: all of its results.
    fn call_function(&mut self, target: Self::Function, arguments: &[Self::Word]) -> Vec<Self::Word>;
    /// What a call's signature is: its parameter and result machine types.
    type Signature: Copy;
    /// A linked symbol whose address a caller reads.
    type Symbol: Copy;
    /// The address of `symbol`.
    fn symbol_address(&mut self, symbol: Self::Symbol) -> Self::Word;
    /// A stack slot a caller spills words into.
    type Slot: Copy;
    /// The address `offset` bytes into `slot`.
    fn slot_address(&mut self, slot: Self::Slot, offset: i32) -> Self::Word;
    /// The low `lane` bits of `value` into `slot`, `offset` bytes in.
    fn slot_store(&mut self, slot: Self::Slot, lane: Lane, value: Self::Word, offset: i32);
    /// A call of the code at `target` under `signature`: all of its results.
    fn call_signature(&mut self, signature: Self::Signature, target: Self::Word, arguments: &[Self::Word]) -> Vec<Self::Word>;
}

/// A machine with blocks: the transfers between them, each carrying words as the target's
// The stencil build runs the operator layer only; access, flow and vectors are the backends'.
#[cfg(not(stencil_build))]
/// parameters.
pub(crate) trait MachineFlow: MachineScalar {
    type Block: Copy;
    /// A block with no parameters, not yet entered.
    fn new_block(&mut self) -> Self::Block;
    /// Continues building at `block`, whose every predecessor has been built.
    fn enter(&mut self, block: Self::Block);
    /// Continues building at `block`, which may still gain predecessors until `seal_all`.
    fn enter_open(&mut self, block: Self::Block);
    /// Declares that every block built has all its predecessors.
    fn seal_all(&mut self);
    /// Continues at `block` with `arguments` as its parameters.
    fn goto(&mut self, block: Self::Block, arguments: &[Self::Word]);
    /// What a jump table is: the targets a selector indexes, the last being the default.
    type Table: Copy;
    /// Continues at the target of `table` that `selector` indexes.
    fn dispatch(&mut self, selector: Self::Word, table: Self::Table);
    /// Leaves the function with `values`.
    fn leave(&mut self, values: &[Self::Word]);
    /// A parameter of `lane` added to `block`: the word it holds once `block` is entered.
    fn block_parameter(&mut self, block: Self::Block, lane: Lane) -> Self::Word;
    /// The jump table of `cases`, which a selector indexes, past which it takes `default` with its arguments.
    fn jump_table(&mut self, cases: &[Self::Block], default: (Self::Block, &[Self::Word])) -> Self::Table;
    /// Continues at `then` where `flag` is nonzero, else at `otherwise`, each with its arguments.
    fn branch_if(
        &mut self,
        flag: Self::Word,
        then: (Self::Block, &[Self::Word]),
        otherwise: (Self::Block, &[Self::Word]),
    );
}

/// Mutable locals of an emitting machine: a value a block redefines, read where control joins.
#[cfg(not(stencil_build))]
pub(crate) trait MachineVariables: Machine {
    type Variable: Copy;
    fn declare_variable(&mut self, lane: Lane) -> Self::Variable;
    fn define_variable(&mut self, variable: Self::Variable, value: Self::Word);
    fn read_variable(&mut self, variable: Self::Variable) -> Self::Word;
    /// The opposite comparison where `flag` is an integer comparison this machine emitted; `None` otherwise.
    fn compare_complement(&mut self, _flag: Self::Word) -> Option<Self::Word> { None }
}

/// A machine that continues in the code it calls, so the call is the end of the function: a tail
/// call of `target`, or of the code at an address under a `signature`, on `arguments`.
#[cfg(all(feature = "jit", not(stencil_build)))]
pub(crate) trait MachineTail: MachineAccess + MachineFlow {
    fn tail_call(&mut self, target: Self::Function, arguments: &[Self::Word]);
    fn tail_call_indirect(&mut self, signature: Self::Signature, target: Self::Word, arguments: &[Self::Word]);
}

/// A machine with vector registers: lane-wise operations by a (lane shape, operator) row, views
/// of a vector's bits in a lane shape, constants, shuffles, and moves through stack words. A
// The stencil build runs the operator layer only; access, flow and vectors are the backends'.
#[cfg(not(stencil_build))]
/// vector is a `Word` of the machine, loaded and stored as the `Lane::V128`.
pub(crate) trait MachineVector: MachineAccess {
    /// The row's operation on `left` (and `right`, a vector or a scalar shift count).
    fn operation_in_lane(&mut self, native: Native, left: Self::Word, right: Option<Self::Word>) -> Self::Word;
    /// `value`, a vector, with the lane shape `shape`: the same bits.
    fn vector_view(&mut self, shape: Lanes, value: Self::Word) -> Self::Word;
    /// The vector whose bits are `bits`.
    fn vector_immediate(&mut self, bits: u128) -> Self::Word;
    /// The vector of the byte `lanes` selected from the concatenation of `left` and `right`.
    fn vector_shuffle_lanes(&mut self, lanes: [u8; V128_BYTES], left: Self::Word, right: Self::Word) -> Self::Word;
    /// An operand into the two words of `slot` at word position `at`: a vector whole, a scalar
    /// in the low word.
    fn spill_operand(&mut self, slot: Self::Slot, at: usize, value: Self::Word);
    /// The value of type `ty` the word position `at` of `slot` holds.
    fn reload_result(&mut self, slot: Self::Slot, at: usize, ty: WasmType) -> Self::Word;
}
