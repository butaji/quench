//! i64 binary operators the lane runs with one handler per operator: the operator is the
//! handler's constant, so the selector decode and the operator's switch fold away.
use super::*;

lane_handler! {
    /// The i64 binary operator whose selector is `OPERATOR`; a trap leaves the lane, where
    /// the general loop raises it.
    fn lane_i64_operator<OPERATOR: u32, ACC: u8>(cx, r, ip, i, view) {
        let operator = const { I64BinaryOperator::from_tag(OPERATOR).expect("an i64 operator") };
        let (Some(left), Some(right)) = (
            read::<ACC, ACC_B>(r, i.b, view).bits64(),
            read::<ACC, ACC_C>(r, i.c, view).bits64(),
        ) else {
            return exit(ip);
        };
        let Ok(value) = operator.evaluate(left as i64, right as i64) else {
            return exit(ip);
        };
        let Some(value) = lane_encode(view, value) else {
            return exit(ip);
        };
        produce!(cx, r, ip, i.a, value, view)
    }
}

struct I64Operators<H>(std::marker::PhantomData<H>);

macro_rules! i64_operator_handlers {
    ($host:ty; $($selector:literal)+) => {
        [$([
            lane_i64_operator::<$host, $selector, ACC_NONE> as Handler,
            lane_i64_operator::<$host, $selector, ACC_A> as Handler,
            lane_i64_operator::<$host, $selector, ACC_B> as Handler,
            lane_i64_operator::<$host, $selector, ACC_C> as Handler,
            lane_i64_operator::<$host, $selector, ACC_PAIR> as Handler,
        ],)+]
    };
}

impl<H: Host> I64Operators<H> {
    /// By selector and accumulator field; the list names every selector of the family.
    const HANDLERS: [[Handler; ACC_POSITIONS]; I64BinaryOperator::COUNT] = i64_operator_handlers!(H;
        0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26);
}

/// The handler of the i64 binary record `op` with operator `selector` and accumulator field `acc`.
pub(super) fn i64_operator_handler<H: Host>(op: Op, selector: u32, acc: u8) -> Option<Handler> {
    if op.class() != Class::WasmI64Binary {
        return None;
    }
    let operator = I64BinaryOperator::from_tag(selector)?;
    Some(I64Operators::<H>::HANDLERS[operator as usize][usize::from(acc)])
}
