//! The natives every tier runs on a staged row (`native_row_table`): one body per row, over the
//! words a tier's environment names (`RowEnv`). A tier supplies only where those words live: the
//! b-jit's stencils read them from the dispatch payload, the u-jit from the dispatch head, the
//! interpreter from its heap. The row's guards and steps are written here once.

#[cfg(stencil_build)]
pub use super::native_row_table::NativeRow;
#[cfg(not(stencil_build))]
pub use quench_bjit::call::BuiltinRow as NativeRow;

use super::region::Region;
use super::numeric::NumericStage;
use super::{builtins, call_native, elements, object, strings, word_offset};
use crate::heap::NATIVE_ARRAY_LAYOUT as ARRAY;

/// The words a row reads from its tier, beside its receiver and arguments.
pub trait RowEnv<S: NumericStage> {
    /// The base of the string records (`strings::flat_string`).
    fn string_records(&mut self, stage: &mut S) -> S::Word;
    /// The table of one-unit ASCII strings (`strings::StringNative::CharAt`).
    fn ascii_unit_strings(&mut self, stage: &mut S) -> S::Word;
    /// The base of the array records (`elements::array_backing`).
    fn array_records(&mut self, stage: &mut S) -> S::Word;
    /// The prototype link of the array chain proved to hold no index, or zero where none is proved.
    fn index_free_chain(&mut self, stage: &mut S) -> S::Word;
    /// Nonzero while array storage proofs are armed, which a row cannot retire.
    fn armed_array_proofs(&mut self, stage: &mut S) -> S::Word;
}

/// The value word of `row` for `receiver` and the argument window at `arguments`, or `None`
/// (leaving at `miss`, before any write) where the native's own steps decide. `charCodeAt` and
/// `charAt` read a flat string's unit (`strings::step_string_native_of`); `Number.isInteger` and
/// `Math.imul` are `call_native::step_number_native_of`; `push` and `pop` are
/// `builtins::step_array_native_of`. A `push` holds the index-free prototype chain proof and stores
/// only a value no write barrier logs; a `pop` leaves while array storage proofs are armed.
#[inline(always)]
pub fn step_native_row<S: NumericStage, E: RowEnv<S>>(stage: &mut S, env: &mut E, row: NativeRow, receiver: S::Word,
    arguments: S::Word, miss: S::Miss) -> Option<S::Word> {
    let argument = |stage: &mut S, index: u16| stage.load_u64(Region::Frame.mutable(), arguments, word_offset(index));
    match row {
        NativeRow::CharCodeAt => {
            let records = env.string_records(stage);
            let string = strings::flat_string(stage, records, receiver, miss)?;
            let index = argument(stage, 0);
            strings::step_string_native_of(stage, strings::StringNative::CharCodeAt { string, index }, miss)
        }
        NativeRow::CharAt => {
            let records = env.string_records(stage);
            let string = strings::flat_string(stage, records, receiver, miss)?;
            let (index, table) = (argument(stage, 0), env.ascii_unit_strings(stage));
            strings::step_string_native_of(stage, strings::StringNative::CharAt { string, index, table }, miss)
        }
        NativeRow::IsInteger => {
            let value = argument(stage, 0);
            call_native::step_number_native_of(stage, call_native::NumberNative::IsInteger, &[value], miss)
        }
        NativeRow::Imul => {
            let args = [argument(stage, 0), argument(stage, 1)];
            call_native::step_number_native_of(stage, call_native::NumberNative::Math(call_native::MathCall::Imul), &args, miss)
        }
        NativeRow::Push => {
            let records = env.array_records(stage);
            let backing = elements::array_backing(stage, records, receiver, miss)?;
            let object = stage.add_imm(backing.record, ARRAY.object_offset as u64);
            let chain = env.index_free_chain(stage);
            let proved = stage.ne_imm(chain, 0u64);
            stage.guard(proved, miss)?;
            let link = object::proto_link(stage, object);
            let same = stage.eq(link, chain);
            stage.guard(same, miss)?;
            // The room is the capacity word under its mask, read as `elements::append_in_room` reads it.
            let word = stage.load_u32(Region::Elements.mutable(), backing.header, ARRAY.header_capacity_offset as i32);
            let room = stage.and_imm(word, u64::from(ARRAY.capacity_mask));
            let value = argument(stage, 0);
            builtins::unreferenced(stage, value, miss)?;
            builtins::step_array_native_of(stage, builtins::ArrayNative::Push { room, value }, backing, miss)
        }
        NativeRow::Pop => {
            let armed = env.armed_array_proofs(stage);
            let disarmed = stage.eq_imm(armed, 0u64);
            stage.guard(disarmed, miss)?;
            let records = env.array_records(stage);
            let backing = elements::array_backing(stage, records, receiver, miss)?;
            builtins::step_array_native_of(stage, builtins::ArrayNative::Pop, backing, miss)
        }
    }
}

/// The row `native` runs on, if every tier runs it on one: the one map from natives to rows.
#[cfg(not(stencil_build))]
pub const fn native_row_of(native: crate::heap::Native) -> Option<NativeRow> {
    use crate::heap::Native;
    Some(match native {
        Native::StringCharCodeAt => NativeRow::CharCodeAt,
        Native::StringCharAt => NativeRow::CharAt,
        Native::NumberIsInteger => NativeRow::IsInteger,
        Native::MathImul => NativeRow::Imul,
        Native::ArrayPush => NativeRow::Push,
        Native::ArrayPop => NativeRow::Pop,
        _ => return None,
    })
}
