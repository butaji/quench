//! The builtins of strings, arrays and Numbers a body calls by identity (`native_calls`),
//! run inline on their staged rows (`staged::builtins`, `staged::strings`): the interpreter
//! runs the same rows on `Concrete`, so a case a row leaves at its miss is the case the
//! interpreter's own step takes, and the call leaves at its PC to run it. Each row reads what
//! it proves once: a string record's form in place, an array's backing through its record.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::bytecode::ImmediateLayout;
use quench_runtime::staged::array_search::{DenseNeedle, scan_dense};
use quench_runtime::staged::region::Region;
use quench_runtime::staged::{Domain, builtins, elements, strings};
use quench_runtime::tier_surface::Native;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// A builtin a call runs on its staged row.
#[derive(Clone, Copy)]
pub(crate) enum Builtin {
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
    /// `Array.isArray` of one value (`plain_array_argument`).
    IsArray,
    /// `Array.prototype.at` of one int32 over a dense array: its present element (`dense_element`).
    At,
}

impl Builtin {
    /// The builtin `native` is, if it is one a call runs inline.
    pub(crate) fn of(native: Native) -> Option<Self> {
        Some(match native {
            Native::StringCharCodeAt => Self::CharCodeAt,
            Native::StringCharAt => Self::CharAt,
            Native::ArrayPush => Self::Push,
            Native::ArrayPop => Self::Pop,
            Native::NumberIsInteger => Self::IsInteger,
            Native::ArrayIncludes => Self::Includes,
            Native::ArrayIndexOf => Self::IndexOf,
            Native::ArrayIsArray => Self::IsArray,
            Native::ArrayAt => Self::At,
            _ => return None,
        })
    }

    /// The arguments the builtin reads; a call passing another count takes the native.
    pub(crate) const fn arguments(self) -> u16 {
        match self { Self::Pop => 0, Self::CharCodeAt | Self::CharAt | Self::Push | Self::IsInteger | Self::Includes | Self::IndexOf | Self::IsArray | Self::At => 1 }
    }

    /// Whether the builtin changes an array's elements.
    pub(crate) const fn mutates_arrays(self) -> bool {
        matches!(self, Self::Push | Self::Pop)
    }
}

/// The row of an Emit stage, which continues past each guard it makes.
pub(crate) fn continues<T>(row: Option<T>) -> T {
    row.expect("the emit stage continues past its guards")
}

impl Lowering<'_, '_> {
    /// The call `builtin` whose callee `exit` guarded: its result is the call's register, or
    /// the call leaves through `exit` before it changes anything.
    pub(crate) fn builtin_call(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, builtin: Builtin, exit: ir::Block) {
        let argument = ImmediateLayout::call_window_base(instruction.imm());
        let (receiver, result) = (instruction.register_c(), instruction.result_register());
        match builtin {
            Builtin::CharCodeAt => {
                // The row answers NaN past the string's end, which leaves at `exit` before the call
                // changes anything: the result is an int32 wherever the call completes.
                // A read whose index the point proves below the string's length takes no bounds test.
                let within = self.ranges.string_in_bounds(pc);
                let code = match self.loop_string(pc, Some(receiver)) {
                    Some(string) => self.held_char_code_at(state, string, argument, within, exit),
                    None => self.char_code_at(state, receiver, argument, within, exit),
                };
                // The within row (`char_code_at_within`) answers an int32 by construction, so only the row that may answer NaN is tested.
                if !within {
                    let integer = is_int_tagged(&mut self.builder, code);
                    emit_js_guard(&mut self.builder, integer, exit);
                }
                let int = self.raw_unary(Unary::Narrow(Lane::I32), code);
                self.set_int(state, result, int);
            }
            Builtin::CharAt => {
                let character = self.char_at(state, receiver, argument, exit);
                self.define(state, result, character, RegisterFact::definition(Type::language(quench_runtime::ValueType::String), Representation::Tagged));
            }
            Builtin::Push => {
                let length = self.push(state, pc, receiver, argument, exit);
                self.set_int(state, result, length);
            }
            Builtin::Pop => {
                let popped = self.pop(state, pc, receiver, exit);
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
            Builtin::At => {
                let element = self.element_at(state, pc, receiver, argument, exit);
                self.set_tagged(state, result, element);
            }
            Builtin::IsArray => {
                // An ordinary array answers true; any other value leaves before the call changes anything, and the
                // call's own step answers it (`Vm::is_array`).
                self.plain_array_argument(state, argument, exit);
                let holds = self.raw_const(Lane::I8, 1);
                let boolean = self.boolean(holds);
                self.set_boolean(state, result, boolean);
            }
        }
    }

    /// Guards that the value in `register` is an ordinary array, one whose record holds no extras: a direct array
    /// among the array records, with no extras in its payload word. An arguments object always holds extras (its
    /// brand is one), so such a record is no arguments object and `Array.isArray` holds of it. Any other value leaves
    /// at `exit`.
    fn plain_array_argument(&mut self, state: &mut State, register: Register, exit: ir::Block) {
        let base = self.tagged(state, register);
        let records = self.dispatch_word(std::mem::offset_of!(JsStepDispatch, head.array_record_base));
        let mut stage = super::super::object_slots::emit(&mut self.builder);
        continues(quench_runtime::staged::builtins::is_plain_array(&mut stage, records, base, exit));
    }

    /// The word of the dispatch payload at `offset`.
    pub(super) fn dispatch_word(&mut self, offset: usize) -> ir::Value {
        emit_js_payload_word(&mut self.builder, self.payload, offset)
    }

    /// The flat string `register` holds, its units and length (`strings::flat_string`).
    fn flat_string(&mut self, state: &mut State, register: Register, exit: ir::Block) -> strings::FlatString<ir::Value> {
        let string = self.tagged(state, register);
        let records = self.dispatch_word(std::mem::offset_of!(JsStepDispatch, head.string_record_base));
        let mut stage = super::super::object_slots::emit(&mut self.builder);
        continues(strings::flat_string(&mut stage, records, string, exit))
    }

    /// `charCodeAt` of the string in `receiver` at the index in `index`: a tagged int32, or NaN. An
    /// index `within` proved below the length has no NaN, so the row reads the unit alone.
    fn char_code_at(&mut self, state: &mut State, receiver: Register, index: Register, within: bool, exit: ir::Block) -> ir::Value {
        let string = self.flat_string(state, receiver, exit);
        let index = self.tagged(state, index);
        let mut stage = super::super::object_slots::emit(&mut self.builder);
        continues(if within {
            strings::string_char_code_at_within(&mut stage, string, index, exit)
        } else {
            strings::step_string_native_of(&mut stage, strings::StringNative::CharCodeAt { string, index }, exit)
        })
    }

    /// `charAt` of the string in `receiver` at the index in `index`, the unit's shared string.
    fn char_at(&mut self, state: &mut State, receiver: Register, index: Register, exit: ir::Block) -> ir::Value {
        let string = self.flat_string(state, receiver, exit);
        let index = self.tagged(state, index);
        let table = self.dispatch_word(std::mem::offset_of!(JsStepDispatch, head.ascii_unit_strings));
        let mut stage = super::super::object_slots::emit(&mut self.builder);
        continues(strings::step_string_native_of(&mut stage, strings::StringNative::CharAt { string, index, table }, exit))
    }

    /// The `length` of the flat string `base` (tagged) as an int32; any other value, or a
    /// length past int32, leaves through `miss`.
    pub(crate) fn string_length(&mut self, base: ir::Value, miss: ir::Block) -> ir::Value {
        let records = self.dispatch_word(std::mem::offset_of!(JsStepDispatch, head.string_record_base));
        let mut stage = super::super::object_slots::emit(&mut self.builder);
        let string = continues(strings::flat_string(&mut stage, records, base, miss));
        let length = continues(quench_runtime::staged::value::length_value(&mut stage, string.length, miss));
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
        let length = continues(builtins::step_array_native_of(&mut stage, builtins::ArrayNative::Push { room, value }, backing, exit));
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

    /// Whether no builtin prototype object has an index property in the closed world (`AotFacts::prototype_indices_empty`),
    /// an edge fact: a hole then reads undefined, with no run-time check.
    fn prototype_indices_empty(&self) -> bool {
        self.vm.facts.is_some_and(|facts| facts.function_facts(self.function_id).prototype_indices_empty())
    }

    /// Continues where the prototypes of `base` hold no index (`LaneHelper::PrototypesHoldNoIndex`): at once where the flag
    /// `checked` is set, else by the helper, which leaves at `exit` where a prototype holds one, and sets the flag.
    fn prototypes_checked(&mut self, base: ir::Value, checked: Variable, exit: ir::Block) {
        let flag = self.builder.use_var(checked);
        let (proven, check) = (self.builder.create_block(), self.builder.create_block());
        self.branch_if(flag, (proven, &[]), (check, &[]));
        self.builder.switch_to_block(check);
        self.builder.seal_block(check);
        let clear = self.helper_call(LaneHelper::PrototypesHoldNoIndex, &[base]);
        let clear = self.raw_compare_imm(Relation::NotEqual, clear, 0);
        emit_js_guard(&mut self.builder, clear, exit);
        let set = self.raw_const(Lane::I8, 1);
        self.builder.def_var(checked, set);
        self.goto(proven, &[]);
        self.builder.switch_to_block(proven);
        self.builder.seal_block(proven);
    }

    /// The position of the first element of the dense array in `receiver` equal to the int32 in
    /// `argument`, as an int32, or -1. Strict equality and SameValueZero agree on a non-NaN
    /// search, so `includes` and `indexOf` share the scan. The scan reads the array's view (the
    /// loop's hoisted one, else proved here) and changes nothing, so it leaves at `exit`, before
    /// any result is defined, for a receiver the view does not prove, a prototype holding an index
    /// (`LaneHelper::PrototypesHoldNoIndex`: a hole then reads undefined, which no int32 equals, so
    /// the scans need no hole test), and an empty view (an encoded backing reads as empty).
    /// The scans are `staged::array_search::scan_dense` on this stage. Pass 1 finds the first word
    /// equal to the int's tagged word. Pass 2 finds the first double float-equal to the int, and
    /// runs only where the backing may hold one (its count of other values is nonzero): over
    /// [0, k) where pass 1 hit at k, over the whole backing where it missed.
    fn element_search(&mut self, state: &mut State, pc: usize, receiver: Register, argument: Register, exit: ir::Block) -> ir::Value {
        let base = self.tagged(state, receiver);
        let hoisted = self.loop_array(pc, Some(receiver)).filter(|hoisted| hoisted.views == ElementViews::Dense);
        let (values, length) = match hoisted {
            Some(hoisted) => self.hoisted_view(hoisted),
            None => {
                let view = emit_dense_array_view(&mut self.builder, self.payload, base, false, self.array_access(), exit);
                (view.values, view.length)
            }
        };
        // The closed world proves the prototypes hold no index (`prototype_indices_empty`): no guard is emitted. Otherwise a
        // loop's entry proved the prototypes once (`hoist_arrays`); an invalidation unsets the flag, and the next search re-checks.
        if !self.prototype_indices_empty() {
            match hoisted.and_then(|hoisted| self.prototype_clear.get(&hoisted.source).copied()) {
                Some(checked) => self.prototypes_checked(base, checked, exit),
                None => {
                    let clear = self.helper_call(LaneHelper::PrototypesHoldNoIndex, &[base]);
                    let clear = self.raw_compare_imm(Relation::NotEqual, clear, 0);
                    emit_js_guard(&mut self.builder, clear, exit);
                }
            }
        }
        let populated = self.raw_compare_imm(Relation::NotEqual, length, 0);
        emit_js_guard(&mut self.builder, populated, exit);
        let int = self.int(state, argument, exit);
        let key = self.tagged(state, argument);
        let search = self.raw_unary(Unary::FromSigned(Lane::F64), int);
        let ints_only = self.holds_no_double(values);
        let length = self.raw_unary(Unary::ZeroExtend(Lane::I64), length);
        let mut stage = super::super::object_slots::emit(&mut self.builder);
        let zero = stage.constant(0);
        // Pass 1: the first word equal to the key, or `length`.
        let words = [key];
        let at = scan_dense(&mut stage, values, zero, length, DenseNeedle { words: &words, number: None, hole: None }, 8);
        let hit = stage.ult(at, length);
        // What the search answers when no double is equal: the word hit, else -1.
        let fallback = {
            let none = stage.constant(u64::MAX);
            stage.select(hit, at, none)
        };
        // Pass 2, over [0, end): the first double equal to the int, else `fallback`.
        let end = stage.select(hit, at, length);
        let index = stage.choose(ints_only, |_| fallback, |stage| {
            let double = scan_dense(stage, values, zero, end, DenseNeedle { words: &[], number: Some(search), hole: None }, 8);
            let none = stage.eq(double, end);
            stage.select(none, fallback, double)
        });
        self.raw_unary(Unary::Narrow(Lane::I32), index)
    }

    /// `at` of the dense array in `receiver` at the int32 in `argument`: the element at the index, or at
    /// the index past the length where it is negative. The read changes nothing, so an index outside the
    /// backing (undefined) and a hole (a prototype's element) leave at `exit` before any result is defined.
    fn element_at(&mut self, state: &mut State, pc: usize, receiver: Register, argument: Register, exit: ir::Block) -> ir::Value {
        let base = self.tagged(state, receiver);
        let hoisted = self.loop_array(pc, Some(receiver)).filter(|hoisted| hoisted.views == ElementViews::Dense);
        let (values, length) = match hoisted {
            Some(hoisted) => self.hoisted_view(hoisted),
            None => {
                let view = emit_dense_array_view(&mut self.builder, self.payload, base, false, self.array_access(), exit);
                (view.values, view.length)
            }
        };
        let int = self.int(state, argument, exit);
        let negative = self.raw_compare_imm(Relation::SignedLess, int, 0);
        let mut stage = super::super::object_slots::emit(&mut self.builder);
        let from_end = stage.add(int, length);
        let index = stage.select(negative, from_end, int);
        continues(elements::dense_element(&mut stage, values, length, index, exit)).1
    }

    /// Whether the backing at `values` holds only int32 words, by its count of other values
    /// (`OTHERS_FROM_VALUES`, zero exactly when every value is an int32; an uncounted backing
    /// reads nonzero, so the double scan runs).
    fn holds_no_double(&mut self, values: ir::Value) -> ir::Value {
        let others = {
            let mut stage = super::super::object_slots::emit(&mut self.builder);
            stage.load_u32(Region::Elements.mutable(), values, quench_runtime::tier_surface::OTHERS_FROM_VALUES)
        };
        self.raw_compare_imm(Relation::Equal, others, 0)
    }

    /// `pop` of the array in `receiver`: its last element, which the backing no longer holds,
    /// and the lane's array storage proofs retire (`LaneHelper::RetireArrayProofs`). Each view a
    /// loop around `pc` hoisted reads its count again (`reload_dense_views`): the pop shortened one in
    /// place, through whichever register names it.
    fn pop(&mut self, state: &mut State, pc: usize, receiver: Register, exit: ir::Block) -> ir::Value {
        let array = self.tagged(state, receiver);
        let records = self.dispatch_word(std::mem::offset_of!(JsStepDispatch, head.array_record_base));
        let mut stage = super::super::object_slots::emit(&mut self.builder);
        let backing = continues(elements::array_backing(&mut stage, records, array, exit));
        let popped = continues(builtins::step_array_native_of(&mut stage, builtins::ArrayNative::Pop, backing, exit));
        self.helper_call(LaneHelper::RetireArrayProofs, &[]);
        self.reload_dense_views(pc);
        popped
    }
}
