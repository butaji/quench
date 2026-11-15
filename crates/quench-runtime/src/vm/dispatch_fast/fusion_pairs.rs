//! The fused pairs of direct records: a load or add and the jump, compare or select after it.

use super::*;

/// The load-pair handler for i32 load `load` and register operator `op`.
pub(super) fn load_pair_binary_handler<H: Host>(load: Op, op: Op, first_left: bool) -> Option<Handler> {
    macro_rules! pairs {
        ($(($load:ident, $op:ident)),+) => {
            match (load as u16, op as u16, first_left) {
                $(
                    (opc::$load, opc::$op, false) => Some(lane_load_pair_binary::<H, { opc::$load }, { opc::$op }, false> as Handler),
                    (opc::$load, opc::$op, true) => Some(lane_load_pair_binary::<H, { opc::$load }, { opc::$op }, true> as Handler),
                )+
                _ => None,
            }
        };
    }
    pairs!(
        (WasmI32Load, WasmI32Add),
        (WasmI32Load, WasmI32Multiply),
        (WasmI32Load16S, WasmI32Multiply),
        (WasmI32Load16U, WasmI32Multiply),
        (WasmI32Load8U, WasmI32Add)
    )
}

lane_handler! {
    /// An i32 add or subtract of a constant (`FIRST`), reading through field `ACC`, the and of
    /// a constant over its result, and the immediate comparison jump `JUMP` testing that
    /// (`(c - '0') & 0xff <= 9`): three records run here, each result written as its own
    /// record writes it.
    fn lane_offset_mask_jump<FIRST: u16, JUMP: u16, BACKWARD: bool, ACC: u8>(cx, r, ip, i, view) {
        let offset = const {
            match I32BinaryOperator::from_immediate_op(opcode(FIRST)) {
                Some(operator) => operator,
                None => panic!("not an i32 immediate operator"),
            }
        };
        let comparison = const {
            match I32BinaryOperator::from_immediate_jump_op(opcode(JUMP)) {
                Some(comparison) => comparison,
                None => panic!("not an i32 immediate comparison jump"),
            }
        };
        let Ok(shifted) = offset.evaluate(read_i32::<ACC, ACC_B>(r, i.b, view), i.imm() as i32) else {
            return exit(ip);
        };
        r.set(i.a, view.integers.encode(shifted));
        let mask_ip = following(ip);
        // SAFETY: the view fuses an offset only with a following mask and comparison jump.
        let mask = unsafe { &*mask_ip };
        let masked = shifted & (mask.imm() as i32);
        let encoded = view.integers.encode(masked);
        r.set(mask.a, encoded);
        let view = Carried { acc: Slot::of(encoded), ..view };
        let jump_ip = following(mask_ip);
        // SAFETY: as above.
        let jump = unsafe { &*jump_ip };
        let Ok(taken) = comparison.evaluate(masked, i32::from(jump.b as i16)) else {
            return exit(jump_ip);
        };
        if taken != 0 {
            resume!(cx, lane_jump::<H>(r, jump_ip, BACKWARD, view, None));
        }
        next!(cx, r, following(jump_ip), view)
    }
}

/// The offset-mask-test handler for offset operator `first`, immediate comparison jump `jump`.
pub(super) fn offset_mask_jump_handler<H: Host>(first: Op, jump: Op, backward: bool, acc: u8) -> Option<Handler> {
    macro_rules! jumps {
        ($first:ident; $($jump:ident),+) => {
            match (jump as u16, backward, acc) {
                $(
                    (opc::$jump, false, ACC_NONE) => Some(lane_offset_mask_jump::<H, { opc::$first }, { opc::$jump }, false, ACC_NONE> as Handler),
                    (opc::$jump, true, ACC_NONE) => Some(lane_offset_mask_jump::<H, { opc::$first }, { opc::$jump }, true, ACC_NONE> as Handler),
                    (opc::$jump, false, ACC_B) => Some(lane_offset_mask_jump::<H, { opc::$first }, { opc::$jump }, false, ACC_B> as Handler),
                    (opc::$jump, true, ACC_B) => Some(lane_offset_mask_jump::<H, { opc::$first }, { opc::$jump }, true, ACC_B> as Handler),
                )+
                _ => None,
            }
        };
    }
    macro_rules! tested {
        ($first:ident) => {
            jumps!(
                $first;
                WasmJumpI32EqualImmediate,
                WasmJumpI32NotEqualImmediate,
                WasmJumpI32LessUnsignedImmediate,
                WasmJumpI32LessEqualUnsignedImmediate,
                WasmJumpI32GreaterUnsignedImmediate,
                WasmJumpI32GreaterEqualUnsignedImmediate
            )
        };
    }
    match first {
        op if op.is_wasm_i32_add_immediate() => tested!(WasmI32AddImmediate),
        op if op.is_wasm_i32_subtract_immediate() => tested!(WasmI32SubtractImmediate),
        _ => None,
    }
}

lane_handler! {
    /// A bit-field extraction, reading its operand through field `ACC`, and a following i32
    /// operator `SECOND` (another extraction, or an operator of registers or a constant): both
    /// records run here, the extraction written before the operator reads its operands.
    fn lane_bit_field_then<SECOND: u16, ACC: u8>(cx, r, ip, i, view) {
        let (operator, immediate) = const {
            let second = opcode(SECOND);
            if second.class().eq(Class::WasmI32ShiftRightUnsignedAnd) { (None, true) }
            else if let Some(operator) = I32BinaryOperator::from_register_op(second) { (Some(operator), false) }
            else if let Some(operator) = I32BinaryOperator::from_immediate_op(second) { (Some(operator), true) }
            else { panic!("not an i32 operator") }
        };
        let field = crate::wasm::integer::shift_right_unsigned_and(
            read_i32::<ACC, ACC_B>(r, i.b, view),
            i32::from(i.c as i16),
            i.imm() as i32,
        );
        r.set(i.a, view.integers.encode(field));
        let second_ip = following(ip);
        // SAFETY: the view fuses an extraction only with a following i32 operator.
        let second = unsafe { &*second_ip };
        let value = match operator {
            None => crate::wasm::integer::shift_right_unsigned_and(
                r.i32(second.b),
                i32::from(second.c as i16),
                second.imm() as i32,
            ),
            Some(operator) => {
                let right = if immediate { second.imm() as i32 } else { r.i32(second.c) };
                let Ok(value) = operator.evaluate(r.i32(second.b), right) else {
                    return exit(second_ip);
                };
                value
            }
        };
        produce!(cx, r, second_ip, second.a, view.integers.encode(value), view)
    }
}

/// The extraction-then-operator handler for i32 operator `second`, the extraction read
/// through field `acc`.
pub(super) fn bit_field_then_handler<H: Host>(second: Op, acc: u8) -> Option<Handler> {
    macro_rules! seconds {
        ($($second:ident),+) => {
            match (second as u16, acc) {
                $(
                    (opc::$second, ACC_NONE) => Some(lane_bit_field_then::<H, { opc::$second }, ACC_NONE> as Handler),
                    (opc::$second, ACC_B) => Some(lane_bit_field_then::<H, { opc::$second }, ACC_B> as Handler),
                )+
                _ => None,
            }
        };
    }
    seconds!(WasmI32ShiftRightUnsignedAndImmediate, WasmI32Multiply, WasmI32XorImmediate)
}

lane_handler! {
    /// An immediate comparison jump `JUMP`, reading its operand through field `ACC`, that
    /// falls through to direct i32 load `LOAD` (`if c == k goto L; v = *p`): both records run
    /// here, the load only when the jump falls through.
    fn lane_compare_jump_load<JUMP: u16, LOAD: u16, BACKWARD: bool, ACC: u8>(cx, r, ip, i, view) {
        let comparison = const {
            match I32BinaryOperator::from_immediate_jump_op(opcode(JUMP)) {
                Some(comparison) => comparison,
                None => panic!("not an i32 immediate comparison jump"),
            }
        };
        let load = const { direct_load(LOAD) };
        let Ok(taken) = comparison.evaluate(read_i32::<ACC, ACC_A>(r, i.a, view), i32::from(i.b as i16)) else {
            return exit(ip);
        };
        if taken != 0 {
            resume!(cx, lane_jump::<H>(r, ip, BACKWARD, view, None));
        }
        let load_ip = following(ip);
        // SAFETY: the view fuses a comparison jump only with a following plain load.
        let access = unsafe { &*load_ip };
        let address = u64::from(r.get(access.c).wasm_bits32()) + u64::from(access.imm());
        // SAFETY: as for `lane_load`.
        let Some(value) = (unsafe { view.memory.read(load, address) }) else {
            return exit(load_ip);
        };
        let Some(value) = lane_encode(view, value) else {
            return exit(load_ip);
        };
        produce!(cx, r, load_ip, access.a, value, view)
    }
}

/// The compare-jump-load handler for immediate comparison jump `jump` and i32 load `load`.
pub(super) fn compare_jump_load_handler<H: Host>(jump: Op, load: Op, backward: bool, acc: u8) -> Option<Handler> {
    macro_rules! accs {
        ($jump:ident, $load:ident) => {
            match (backward, acc) {
                (false, ACC_NONE) => Some(lane_compare_jump_load::<H, { opc::$jump }, { opc::$load }, false, ACC_NONE> as Handler),
                (true, ACC_NONE) => Some(lane_compare_jump_load::<H, { opc::$jump }, { opc::$load }, true, ACC_NONE> as Handler),
                (false, ACC_A) => Some(lane_compare_jump_load::<H, { opc::$jump }, { opc::$load }, false, ACC_A> as Handler),
                (true, ACC_A) => Some(lane_compare_jump_load::<H, { opc::$jump }, { opc::$load }, true, ACC_A> as Handler),
                _ => None,
            }
        };
    }
    macro_rules! loads {
        ($jump:ident) => {
            match load.class() {
                Class::WasmI32Load => accs!($jump, WasmI32Load),
                Class::WasmI32Load8U => accs!($jump, WasmI32Load8U),
                _ => None,
            }
        };
    }
    match jump.class() {
        Class::WasmJumpI32EqualImmediate => loads!(WasmJumpI32EqualImmediate),
        Class::WasmJumpI32NotEqualImmediate => loads!(WasmJumpI32NotEqualImmediate),
        _ => None,
    }
}

lane_handler! {
    /// Two i32 adds (`FIRST`, reading through field `ACC`, then `SECOND`), each of a register
    /// or a constant, or an add and a counted loop's step and test (`SECOND` is
    /// `WasmI32AddImmediateJumpNonZero`): a strided loop's pointer and counter updates. The
    /// first result is written before the second reads its operands, as two records do.
    fn lane_add_pair<FIRST: u16, SECOND: u16, BACKWARD: bool, ACC: u8>(cx, r, ip, i, view) {
        let first_immediate = const {
            match opcode(FIRST).class() {
                Class::WasmI32AddRegister => false,
                Class::WasmI32AddImmediate => true,
                _ => panic!("not an i32 add"),
            }
        };
        let right = if first_immediate { i.imm() as i32 } else { read_i32::<ACC, ACC_C>(r, i.c, view) };
        let first = view.integers.encode(read_i32::<ACC, ACC_B>(r, i.b, view).wrapping_add(right));
        r.set(i.a, first);
        let second_ip = following(ip);
        // SAFETY: the view fuses an add only with a following add or loop step.
        let second = unsafe { &*second_ip };
        let view = Carried { acc: Slot::of(first), ..view };
        match const { opcode(SECOND).class() } {
            Class::WasmI32AddJump => {
                let sum = r.i32(second.b).wrapping_add(i32::from(second.c as i16));
                let encoded = view.integers.encode(sum);
                if sum != 0 {
                    resume!(cx, lane_jump::<H>(r, second_ip, BACKWARD, view, Some((second.a, encoded))));
                }
                produce!(cx, r, second_ip, second.a, encoded, view)
            }
            Class::WasmI32AddImmediate => {
                let sum = r.i32(second.b).wrapping_add(second.imm() as i32);
                produce!(cx, r, second_ip, second.a, view.integers.encode(sum), view)
            }
            _ => {
                let sum = r.i32(second.b).wrapping_add(r.i32(second.c));
                produce!(cx, r, second_ip, second.a, view.integers.encode(sum), view)
            }
        }
    }
}

/// The add-pair handler for add `first` read through field `acc`, then `second`.
pub(super) fn add_pair_handler<H: Host>(first: Op, second: Op, backward: bool, acc: u8) -> Option<Handler> {
    macro_rules! accs {
        ($first:ident, $second:ident, $backward:literal) => {
            match acc {
                ACC_NONE => Some(lane_add_pair::<H, { opc::$first }, { opc::$second }, $backward, ACC_NONE> as Handler),
                ACC_B => Some(lane_add_pair::<H, { opc::$first }, { opc::$second }, $backward, ACC_B> as Handler),
                ACC_C => Some(lane_add_pair::<H, { opc::$first }, { opc::$second }, $backward, ACC_C> as Handler),
                _ => None,
            }
        };
    }
    macro_rules! seconds {
        ($first:ident) => {
            match (second.class(), backward) {
                (Class::WasmI32AddRegister, _) => accs!($first, WasmI32Add, false),
                (Class::WasmI32AddImmediate, _) => accs!($first, WasmI32AddImmediate, false),
                (Class::WasmI32AddJump, false) => accs!($first, WasmI32AddImmediateJumpNonZero, false),
                (Class::WasmI32AddJump, true) => accs!($first, WasmI32AddImmediateJumpNonZero, true),
                _ => None,
            }
        };
    }
    match first.class() {
        Class::WasmI32AddRegister => seconds!(WasmI32Add),
        Class::WasmI32AddImmediate => seconds!(WasmI32AddImmediate),
        _ => None,
    }
}

lane_handler! {
    /// An i32 operator of a constant (`FIRST`) and the register comparison jump `JUMP` reading
    /// its result as either operand (`(x & mask) == y`): two records run here, the result
    /// written as its own record writes it.
    fn lane_immediate_compare_jump<FIRST: u16, JUMP: u16, BACKWARD: bool>(cx, r, ip, i, view) {
        let operator = const {
            match I32BinaryOperator::from_immediate_op(opcode(FIRST)) {
                Some(operator) => operator,
                None => panic!("not an i32 immediate operator"),
            }
        };
        let comparison = const {
            match I32BinaryOperator::from_jump_op(opcode(JUMP)) {
                Some(comparison) => comparison,
                None => panic!("not an i32 register comparison jump"),
            }
        };
        let Ok(result) = operator.evaluate(r.i32(i.b), i.imm() as i32) else {
            return exit(ip);
        };
        let encoded = view.integers.encode(result);
        r.set(i.a, encoded);
        let view = Carried { acc: Slot::of(encoded), ..view };
        let jump_ip = following(ip);
        // SAFETY: the view fuses an immediate operator only with a following comparison jump.
        let jump = unsafe { &*jump_ip };
        let operand = |register: u16| if register == i.a { result } else { r.i32(register) };
        let Ok(taken) = comparison.evaluate(operand(jump.a), operand(jump.b)) else {
            return exit(jump_ip);
        };
        if taken != 0 {
            resume!(cx, lane_jump::<H>(r, jump_ip, BACKWARD, view, None));
        }
        next!(cx, r, following(jump_ip), view)
    }
}

/// The immediate-operator comparison-jump handler for operator `first`, register comparison
/// jump `jump`.
pub(super) fn immediate_compare_jump_handler<H: Host>(first: Op, jump: Op, backward: bool) -> Option<Handler> {
    macro_rules! jumps {
        ($first:ident; $($jump:ident),+) => {
            match (jump as u16, backward) {
                $(
                    (opc::$jump, false) => Some(lane_immediate_compare_jump::<H, { opc::$first }, { opc::$jump }, false> as Handler),
                    (opc::$jump, true) => Some(lane_immediate_compare_jump::<H, { opc::$first }, { opc::$jump }, true> as Handler),
                )+
                _ => None,
            }
        };
    }
    macro_rules! tested {
        ($first:ident) => {
            jumps!($first; WasmJumpI32Equal, WasmJumpI32NotEqual)
        };
    }
    // Masked tag tests; each further pairing is another family of handler copies.
    match first as u16 {
        opc::WasmI32AndImmediate => tested!(WasmI32AndImmediate),
        _ => None,
    }
}

lane_handler! {
    /// An i32 operator `OP` (reading through field `ACC`; a comparison, typically), a constant
    /// load and a select choosing among their results and other registers (`cur = cur > clip
    /// ? 0 : cur`): three records run here, each result written as its own record writes it,
    /// and the select reads the two results as computed rather than back from registers.
    fn lane_operator_constant_select<OP: u16, ACC: u8>(cx, r, ip, i, view) {
        let (operator, immediate) = const {
            match I32BinaryOperator::from_register_op(opcode(OP)) {
                Some(operator) => (operator, false),
                None => match I32BinaryOperator::from_immediate_op(opcode(OP)) {
                    Some(operator) => (operator, true),
                    None => panic!("not an i32 binary operator"),
                },
            }
        };
        let right = if immediate { i.imm() as i32 } else { read_i32::<ACC, ACC_C>(r, i.c, view) };
        let Ok(result) = operator.evaluate(read_i32::<ACC, ACC_B>(r, i.b, view), right) else {
            return exit(ip);
        };
        let result = view.integers.encode(result);
        r.set(i.a, result);
        let constant_ip = following(ip);
        // SAFETY: the view fuses an operator only with a following constant and select.
        let load = unsafe { &*constant_ip };
        let constant = load.constant();
        r.set(load.a, constant);
        let select_ip = following(constant_ip);
        // SAFETY: as above.
        let select = unsafe { &*select_ip };
        // The constant's record runs second, so it wins a register both write.
        let value_of = |register: u16| {
            if register == load.a {
                constant
            } else if register == i.a {
                result
            } else {
                r.get(register)
            }
        };
        let (condition, _) = ImmediateLayout::register_pair(select.imm());
        let value = select_loaded(
            value_of(condition).wasm_bits32() != 0,
            value_of(select.b),
            value_of(select.c),
        );
        produce!(cx, r, select_ip, select.a, value, view)
    }
}

/// The operator-constant-select handler for i32 operator `op` reading through field `acc`.
pub(super) fn operator_constant_select_handler<H: Host>(op: Op, acc: u8) -> Option<Handler> {
    macro_rules! operators {
        ($($op:ident),+) => {
            match (op as u16, acc) {
                $(
                    (opc::$op, ACC_NONE) => Some(lane_operator_constant_select::<H, { opc::$op }, ACC_NONE> as Handler),
                    (opc::$op, ACC_B) => Some(lane_operator_constant_select::<H, { opc::$op }, ACC_B> as Handler),
                    (opc::$op, ACC_C) => Some(lane_operator_constant_select::<H, { opc::$op }, ACC_C> as Handler),
                )+
                _ => None,
            }
        };
    }
    operators!(
        WasmI32GreaterSigned,
        WasmI32GreaterUnsigned,
        WasmI32LessSigned,
        WasmI32LessUnsigned,
        WasmI32Equal,
        WasmI32NotEqual,
        WasmI32GreaterEqualSigned,
        WasmI32LessEqualSigned
    )
}
