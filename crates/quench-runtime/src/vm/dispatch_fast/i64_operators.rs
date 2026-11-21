//! i64 binary operators the lane runs with one handler per operator: the operator is the
//! handler's constant, so the selector decode and the operator's switch fold away.
use super::*;

lane_handler! {
    /// The i64 binary operator whose selector is `OPERATOR`; a trap leaves the lane, where
    /// the general loop raises it.
    fn lane_i64_operator<OPERATOR: u32, RAW: bool, ACC: u8>(cx, r, ip, i, view) {
        let operator = const { I64BinaryOperator::from_tag(OPERATOR).expect("an i64 operator") };
        let (Some(left), Some(right)) = (
            wide_operand::<RAW>(read::<ACC, ACC_B>(r, i.b, view)),
            wide_operand::<RAW>(read::<ACC, ACC_C>(r, i.c, view)),
        ) else {
            return exit(ip);
        };
        let Ok(value) = operator.evaluate(left as i64, right as i64) else {
            return exit(ip);
        };
        let Some(value) = wide_result::<RAW>(view, value) else {
            return exit(ip);
        };
        produce!(cx, r, ip, i.a, value, view)
    }
}

lane_handler! {
    /// `lane_i64_operator` of a 16-bit constant right operand (`WasmI64BinaryImmediate`): one
    /// register read and its check fewer, and no constant load before it.
    fn lane_i64_operator_immediate<OPERATOR: u32, RAW: bool, ACC: u8>(cx, r, ip, i, view) {
        let operator = const { I64BinaryOperator::from_tag(OPERATOR).expect("an i64 operator") };
        let Some(left) = wide_operand::<RAW>(read::<ACC, ACC_B>(r, i.b, view)) else {
            return exit(ip);
        };
        let Ok(value) = operator.evaluate(left as i64, i64::from(i.c as i16)) else {
            return exit(ip);
        };
        let Some(value) = wide_result::<RAW>(view, value) else {
            return exit(ip);
        };
        produce!(cx, r, ip, i.a, value, view)
    }
}

struct I64Operators;

macro_rules! i64_operator_handlers {
    ($handler:ident, $raw:literal; $($selector:literal)+) => {
        [$([
            $handler::<$selector, $raw, ACC_NONE> as Handler,
            $handler::<$selector, $raw, ACC_A> as Handler,
            $handler::<$selector, $raw, ACC_B> as Handler,
            $handler::<$selector, $raw, ACC_C> as Handler,
            $handler::<$selector, $raw, ACC_PAIR> as Handler,
        ],)+]
    };
}

impl I64Operators {
    /// By selector and accumulator field; the list names every selector of the family.
    const HANDLERS: [[Handler; ACC_POSITIONS]; I64BinaryOperator::ALL.len()] = i64_operator_handlers!(lane_i64_operator, false;
        0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26);
    /// The same of a constant right operand.
    const IMMEDIATE: [[Handler; ACC_POSITIONS]; I64BinaryOperator::ALL.len()] = i64_operator_handlers!(lane_i64_operator_immediate, false;
        0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26);
    /// The raw forms of both (`WasmOperation::is_raw`): no tag checks.
    const RAW: [[Handler; ACC_POSITIONS]; I64BinaryOperator::ALL.len()] = i64_operator_handlers!(lane_i64_operator, true;
        0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26);
    const RAW_IMMEDIATE: [[Handler; ACC_POSITIONS]; I64BinaryOperator::ALL.len()] = i64_operator_handlers!(lane_i64_operator_immediate, true;
        0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26);
}

/// The handler of the i64 binary record `op` with operator `selector` and accumulator field `acc`.
pub(super) fn i64_operator_handler(op: Op, selector: u32, acc: u8) -> Option<Handler> {
    let raw = WasmOperation::is_raw(op);
    let table = match Step::of(op, selector) {
        Some(Step::Numeric(WasmOperation::Binary { width: Width::I64, form: Form::Register, .. })) => {
            if raw { &I64Operators::RAW } else { &I64Operators::HANDLERS }
        }
        Some(Step::Numeric(WasmOperation::Binary { width: Width::I64, form: Form::Short, .. })) => {
            if raw { &I64Operators::RAW_IMMEDIATE } else { &I64Operators::IMMEDIATE }
        }
        _ => return None,
    };
    let operator = I64BinaryOperator::from_tag(selector)?;
    Some(table[operator as usize][usize::from(acc)])
}
