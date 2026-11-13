//! The builtins of strings, arrays and Numbers a body calls by identity (`native_calls`),
//! run inline on their staged rows (`staged::builtins`, `staged::strings`): the interpreter
//! runs the same rows on `Concrete`, so a case a row leaves at its miss is the case the
//! interpreter's own step takes, and the call leaves at its PC to run it. Each row reads what
//! it proves once: a string record's form in place, an array's backing through its record.

use super::*;
use crate::bytecode::ImmediateLayout;
use crate::staged::{Domain, builtins, elements, strings};
use crate::vm::Native;

/// A builtin a call runs on its staged row.
#[derive(Clone, Copy)]
pub(super) enum Builtin {
    /// `String.prototype.charCodeAt` of a flat string at an int32 index.
    CharCodeAt,
    /// `String.prototype.charAt` of a flat string at an int32 index of an ASCII unit.
    CharAt,
    /// `Array.prototype.push` of one Number, boolean or other value that is no reference.
    Push,
    /// `Array.prototype.pop`.
    Pop,
    /// `Number.isInteger` of a Number.
    IsInteger,
}

impl Builtin {
    /// The builtin `native` is, if it is one a call runs inline.
    pub(super) fn of(native: Native) -> Option<Self> {
        Some(match native {
            Native::StringCharCodeAt => Self::CharCodeAt,
            Native::StringCharAt => Self::CharAt,
            Native::ArrayPush => Self::Push,
            Native::ArrayPop => Self::Pop,
            Native::NumberIsInteger => Self::IsInteger,
            _ => return None,
        })
    }

    /// The arguments the builtin reads; a call passing another count takes the native.
    pub(super) const fn arguments(self) -> u16 {
        match self { Self::Pop => 0, Self::CharCodeAt | Self::CharAt | Self::Push | Self::IsInteger => 1 }
    }

    /// Whether the builtin changes an array's elements.
    pub(super) const fn mutates_arrays(self) -> bool {
        matches!(self, Self::Push | Self::Pop)
    }
}

/// The row of an Emit stage, which continues past each guard it makes.
fn continues<T>(row: Option<T>) -> T {
    row.expect("the emit stage continues past its guards")
}

impl Lowering<'_, '_> {
    /// The call `builtin` whose callee `exit` guarded: its result is the call's register, or
    /// the call leaves through `exit` before it changes anything.
    pub(super) fn builtin_call(&mut self, state: &mut State, instruction: WideInstruction, builtin: Builtin, exit: ir::Block) {
        let argument = ImmediateLayout::call_window_base(instruction.imm());
        let (receiver, result) = (instruction.register_c(), instruction.result_register());
        match builtin {
            Builtin::CharCodeAt => {
                let code = self.char_code_at(state, receiver, argument, exit);
                self.set_tagged(state, result, code);
            }
            Builtin::CharAt => {
                let character = self.char_at(state, receiver, argument, exit);
                self.define(state, result, character, RegisterFact::definition(Type::language(crate::ValueType::String), Representation::Tagged));
            }
            Builtin::Push => {
                let length = self.push(state, receiver, argument, exit);
                self.set_int(state, result, length);
            }
            Builtin::Pop => {
                let popped = self.pop(state, receiver, exit);
                self.set_tagged(state, result, popped);
            }
            Builtin::IsInteger => {
                let number = self.number(state, argument, exit);
                let mut stage = Emit { builder: &mut self.builder, property_values: None };
                let integral = builtins::is_integral(&mut stage, Domain::Float64, number);
                let boolean = self.boolean(integral);
                self.set_boolean(state, result, boolean);
            }
        }
    }

    /// The word of the dispatch payload at `offset`.
    fn dispatch_word(&mut self, offset: usize) -> ir::Value {
        emit_js_payload_word(&mut self.builder, self.payload, offset)
    }

    /// The flat string `register` holds, its units and length (`strings::flat_string`).
    fn flat_string(&mut self, state: &mut State, register: Register, exit: ir::Block) -> strings::FlatString<ir::Value> {
        let string = self.tagged(state, register);
        let records = self.dispatch_word(std::mem::offset_of!(JsStepDispatch, string_record_base));
        let mut stage = Emit { builder: &mut self.builder, property_values: None };
        continues(strings::flat_string(&mut stage, records, string, exit))
    }

    /// `charCodeAt` of the string in `receiver` at the index in `index`: a tagged int32, or NaN.
    fn char_code_at(&mut self, state: &mut State, receiver: Register, index: Register, exit: ir::Block) -> ir::Value {
        let string = self.flat_string(state, receiver, exit);
        let index = self.tagged(state, index);
        let mut stage = Emit { builder: &mut self.builder, property_values: None };
        continues(strings::string_char_code_at(&mut stage, string, index, exit))
    }

    /// `charAt` of the string in `receiver` at the index in `index`, the unit's shared string.
    fn char_at(&mut self, state: &mut State, receiver: Register, index: Register, exit: ir::Block) -> ir::Value {
        let string = self.flat_string(state, receiver, exit);
        let index = self.tagged(state, index);
        let table = self.dispatch_word(std::mem::offset_of!(JsStepDispatch, ascii_unit_strings));
        let mut stage = Emit { builder: &mut self.builder, property_values: None };
        continues(strings::string_char_at(&mut stage, string, index, table, exit))
    }

    /// The `length` of the flat string `base` (tagged) as an int32; any other value, or a
    /// length past int32, leaves through `miss`.
    pub(super) fn string_length(&mut self, base: ir::Value, miss: ir::Block) -> ir::Value {
        let records = self.dispatch_word(std::mem::offset_of!(JsStepDispatch, string_record_base));
        let mut stage = Emit { builder: &mut self.builder, property_values: None };
        let string = continues(strings::flat_string(&mut stage, records, base, miss));
        let length = continues(builtins::length_value(&mut stage, string.length, miss));
        self.narrow_word(length)
    }

    /// `push` of the value in `argument` onto the array in `receiver`: its new length, an
    /// int32. The VM reserves the room and proves the append plain (`LaneHelper::AppendRoom`:
    /// dense, extensible, its backing its own, no prototype holding an index), which it
    /// declines where a collection is due; the row then appends a value no write barrier logs.
    fn push(&mut self, state: &mut State, receiver: Register, argument: Register, exit: ir::Block) -> ir::Value {
        let (array, value) = (self.tagged(state, receiver), self.tagged(state, argument));
        let mut stage = Emit { builder: &mut self.builder, property_values: None };
        continues(builtins::unreferenced(&mut stage, value, exit));
        let room = self.helper_call(LaneHelper::AppendRoom, &[array]);
        let granted = self.word_cmp_imm(IntCC::NotEqual, room, 0);
        emit_js_guard(&mut self.builder, granted, exit);
        let records = self.dispatch_word(std::mem::offset_of!(JsStepDispatch, array_record_base));
        let mut stage = Emit { builder: &mut self.builder, property_values: None };
        let backing = continues(elements::array_backing(&mut stage, records, array, exit));
        let length = continues(builtins::array_push_one(&mut stage, backing, room, value, exit));
        self.narrow_word(length)
    }

    /// `pop` of the array in `receiver`: its last element, which the backing no longer holds,
    /// and the lane's array storage proofs retire (`LaneHelper::RetireArrayProofs`).
    fn pop(&mut self, state: &mut State, receiver: Register, exit: ir::Block) -> ir::Value {
        let array = self.tagged(state, receiver);
        let records = self.dispatch_word(std::mem::offset_of!(JsStepDispatch, array_record_base));
        let mut stage = Emit { builder: &mut self.builder, property_values: None };
        let backing = continues(elements::array_backing(&mut stage, records, array, exit));
        let popped = continues(builtins::array_pop(&mut stage, backing, exit));
        self.helper_call(LaneHelper::RetireArrayProofs, &[]);
        popped
    }
}
