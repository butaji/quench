//! The builtins of strings, arrays and Numbers a body calls by identity (`native_calls`),
//! run inline on their staged rows (`staged::builtins`, `staged::strings`): the interpreter
//! runs the same rows on `Concrete`, so a case a row leaves at its miss is the case the
//! interpreter's own step takes, and the call leaves at its PC to run it. Each row reads what
//! it proves once: a string record's form in place, an array's backing through its record.

use super::*;
use crate::bytecode::ImmediateLayout;
use crate::staged::region::Region;
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
    /// `Array.prototype.includes` of one int32 over a dense array (`element_search`).
    Includes,
    /// `Array.prototype.indexOf` of one int32 over a dense array (`element_search`).
    IndexOf,
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
            Native::ArrayIncludes => Self::Includes,
            Native::ArrayIndexOf => Self::IndexOf,
            _ => return None,
        })
    }

    /// The arguments the builtin reads; a call passing another count takes the native.
    pub(super) const fn arguments(self) -> u16 {
        match self { Self::Pop => 0, Self::CharCodeAt | Self::CharAt | Self::Push | Self::IsInteger | Self::Includes | Self::IndexOf => 1 }
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
    pub(super) fn builtin_call(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, builtin: Builtin, exit: ir::Block) {
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
                let length = self.push(state, pc, receiver, argument, exit);
                self.set_int(state, result, length);
            }
            Builtin::Pop => {
                let popped = self.pop(state, receiver, exit);
                self.set_tagged(state, result, popped);
            }
            Builtin::IsInteger => {
                let number = self.number(state, argument, exit);
                let mut stage = super::super::object_slots::emit(&mut self.builder);
                let integral = builtins::is_integral(&mut stage, Domain::Float64, number);
                let boolean = self.boolean(integral);
                self.set_boolean(state, result, boolean);
            }
            Builtin::Includes => {
                let position = self.element_search(state, pc, receiver, argument, exit);
                let found = self.raw_compare_imm(Relation::NotEqual, position, -1);
                let boolean = self.boolean(found);
                self.set_boolean(state, result, boolean);
            }
            Builtin::IndexOf => {
                let position = self.element_search(state, pc, receiver, argument, exit);
                self.set_int(state, result, position);
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
        let records = self.dispatch_word(std::mem::offset_of!(JsStepDispatch, head.string_record_base));
        let mut stage = super::super::object_slots::emit(&mut self.builder);
        continues(strings::flat_string(&mut stage, records, string, exit))
    }

    /// `charCodeAt` of the string in `receiver` at the index in `index`: a tagged int32, or NaN.
    fn char_code_at(&mut self, state: &mut State, receiver: Register, index: Register, exit: ir::Block) -> ir::Value {
        let string = self.flat_string(state, receiver, exit);
        let index = self.tagged(state, index);
        let mut stage = super::super::object_slots::emit(&mut self.builder);
        continues(strings::string_char_code_at(&mut stage, string, index, exit))
    }

    /// `charAt` of the string in `receiver` at the index in `index`, the unit's shared string.
    fn char_at(&mut self, state: &mut State, receiver: Register, index: Register, exit: ir::Block) -> ir::Value {
        let string = self.flat_string(state, receiver, exit);
        let index = self.tagged(state, index);
        let table = self.dispatch_word(std::mem::offset_of!(JsStepDispatch, ascii_unit_strings));
        let mut stage = super::super::object_slots::emit(&mut self.builder);
        continues(strings::string_char_at(&mut stage, string, index, table, exit))
    }

    /// The `length` of the flat string `base` (tagged) as an int32; any other value, or a
    /// length past int32, leaves through `miss`.
    pub(super) fn string_length(&mut self, base: ir::Value, miss: ir::Block) -> ir::Value {
        let records = self.dispatch_word(std::mem::offset_of!(JsStepDispatch, head.string_record_base));
        let mut stage = super::super::object_slots::emit(&mut self.builder);
        let string = continues(strings::flat_string(&mut stage, records, base, miss));
        let length = continues(crate::staged::value::length_value(&mut stage, string.length, miss));
        self.raw_unary(Unary::Narrow(Lane::I32), length)
    }

    /// `push` of the value in `argument` onto the array in `receiver`: its new length, an
    /// int32. A loop's hoisted dense array appends within its proved room (`hoisted_push`);
    /// any other array has the VM reserve the room and prove the append plain
    /// (`LaneHelper::AppendRoom`: dense, extensible, its backing its own, no prototype holding
    /// an index), which it declines where a collection is due; the row then appends a value no
    /// write barrier logs.
    fn push(&mut self, state: &mut State, pc: usize, receiver: Register, argument: Register, exit: ir::Block) -> ir::Value {
        let (array, value) = (self.tagged(state, receiver), self.tagged(state, argument));
        let mut stage = super::super::object_slots::emit(&mut self.builder);
        continues(builtins::unreferenced(&mut stage, value, exit));
        if let Some(hoisted) = self.loop_array(pc, Some(receiver)).filter(|hoisted| hoisted.views == ElementViews::Dense) {
            return self.hoisted_push(hoisted, pc, array, value, exit);
        }
        let room = self.helper_call(LaneHelper::AppendRoom, &[array]);
        let granted = self.raw_compare_imm(Relation::NotEqual, room, 0);
        emit_js_guard(&mut self.builder, granted, exit);
        // The reserve may have moved the backing of every view hoisted around `pc`.
        self.reload_dense_arrays(pc);
        let records = self.dispatch_word(std::mem::offset_of!(JsStepDispatch, head.array_record_base));
        let mut stage = super::super::object_slots::emit(&mut self.builder);
        let backing = continues(elements::array_backing(&mut stage, records, array, exit));
        let length = continues(builtins::array_push_one(&mut stage, backing, room, value, exit));
        self.raw_unary(Unary::Narrow(Lane::I32), length)
    }

    /// `push` of `value` onto the loop's hoisted dense array `array` (`base` tagged): the element
    /// stored at its length within the proved room, else after a reserve (`dense_append`), and the
    /// new length, which the array's view holds. The general push leaves at `exit` before it
    /// changes anything.
    fn hoisted_push(&mut self, array: LoopArray, pc: usize, base: ir::Value, value: ir::Value, exit: ir::Block) -> ir::Value {
        let (_, length) = self.hoisted_view(array);
        let done = self.builder.create_block();
        self.dense_append(array, pc as u32, length, value, base, done);
        self.goto(exit, &[]);
        self.builder.switch_to_block(done);
        self.builder.seal_block(done);
        let variables = self.array_variables(array);
        self.builder.use_var(variables.length)
    }

    /// The position of the first element of the dense array in `receiver` equal to the int32 in
    /// `argument`, as an int32, or -1. Strict equality and SameValueZero agree on a non-NaN
    /// search, so `includes` and `indexOf` share the scan. The scan reads the array's view (the
    /// loop's hoisted one, else proved here) and changes nothing, so it leaves at `exit`, before
    /// any result is defined, for a receiver the view does not prove, an empty view (an encoded
    /// backing reads as empty), and a hole: a prototype may hold the hole's index, which the
    /// generic algorithm reads.
    fn element_search(&mut self, state: &mut State, pc: usize, receiver: Register, argument: Register, exit: ir::Block) -> ir::Value {
        let (values, length) = match self.loop_array(pc, Some(receiver)).filter(|hoisted| hoisted.views == ElementViews::Dense) {
            Some(hoisted) => self.hoisted_view(hoisted),
            None => {
                let base = self.tagged(state, receiver);
                let view = emit_dense_array_view(&mut self.builder, self.payload, base, false, exit);
                (view.values, view.length)
            }
        };
        let populated = self.raw_compare_imm(Relation::NotEqual, length, 0);
        emit_js_guard(&mut self.builder, populated, exit);
        let int = self.int(state, argument, exit);
        let key = self.tagged(state, argument);
        let search = self.raw_unary(Unary::FromSigned(Lane::F64), int);
        let (header, body, next, found, missing, done) = (self.builder.create_block(), self.builder.create_block(),
            self.builder.create_block(), self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        self.builder.append_block_param(header, ir::types::I64);
        self.builder.append_block_param(found, ir::types::I64);
        self.builder.append_block_param(done, ir::types::I32);
        // The scan walks the backing's words from its first to its end, one 8-byte element a step.
        let end = {
            let length = self.raw_unary(Unary::ZeroExtend(Lane::I64), length);
            let mut stage = super::super::object_slots::emit(&mut self.builder);
            let bytes = stage.mul_imm(length, size_of::<crate::Value>() as u64);
            stage.add(values, bytes)
        };
        self.goto(header, &[values]);
        self.builder.switch_to_block(header);
        let cursor = self.builder.block_params(header)[0];
        let at_end = self.raw_compare(Relation::Equal, cursor, end);
        self.branch_if(at_end, (missing, &[]), (body, &[]));
        self.builder.switch_to_block(body);
        self.builder.seal_block(body);
        let word = {
            let mut stage = super::super::object_slots::emit(&mut self.builder);
            stage.load_u64(Region::Elements.mutable(), cursor, 0)
        };
        let present = self.raw_compare_imm(Relation::NotEqual, word, crate::Value::DELETED.to_bits() as i64);
        emit_js_guard(&mut self.builder, present, exit);
        // A word equal to the int's tagged word, or a double word of its value: any other word
        // is a NaN-space tag or a NaN, which no Number compares equal to.
        let same = self.raw_compare(Relation::Equal, word, key);
        let number = self.raw_unary(Unary::Bitcast(Lane::F64), word);
        let equal = self.raw_compare(Relation::FloatEqual, number, search);
        let hit = self.raw_binary(Binary::Or, same, equal);
        self.branch_if(hit, (found, &[cursor]), (next, &[]));
        self.builder.switch_to_block(next);
        self.builder.seal_block(next);
        let advanced = {
            let mut stage = super::super::object_slots::emit(&mut self.builder);
            stage.add_imm(cursor, size_of::<crate::Value>() as u64)
        };
        self.goto(header, &[advanced]);
        self.builder.seal_block(header);
        self.builder.switch_to_block(found);
        self.builder.seal_block(found);
        let at = self.builder.block_params(found)[0];
        let position = {
            let offset = self.raw_binary(Binary::Sub, at, values);
            let mut stage = super::super::object_slots::emit(&mut self.builder);
            let index = stage.shr_imm(offset, size_of::<crate::Value>().trailing_zeros());
            self.raw_unary(Unary::Narrow(Lane::I32), index)
        };
        self.goto(done, &[position]);
        self.builder.switch_to_block(missing);
        self.builder.seal_block(missing);
        let none = self.raw_const(Lane::I32, -1);
        self.goto(done, &[none]);
        self.builder.switch_to_block(done);
        self.builder.seal_block(done);
        self.builder.block_params(done)[0]
    }

    /// `pop` of the array in `receiver`: its last element, which the backing no longer holds,
    /// and the lane's array storage proofs retire (`LaneHelper::RetireArrayProofs`).
    fn pop(&mut self, state: &mut State, receiver: Register, exit: ir::Block) -> ir::Value {
        let array = self.tagged(state, receiver);
        let records = self.dispatch_word(std::mem::offset_of!(JsStepDispatch, head.array_record_base));
        let mut stage = super::super::object_slots::emit(&mut self.builder);
        let backing = continues(elements::array_backing(&mut stage, records, array, exit));
        let popped = continues(builtins::array_pop(&mut stage, backing, exit));
        self.helper_call(LaneHelper::RetireArrayProofs, &[]);
        popped
    }
}
