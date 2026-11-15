//! The proved int32 records of a region (`region_state`): the counted loop's update and test,
//! the int32 sum, and the truncation island `(a ± i * k) | 0`. Each is the lane's own record
//! with its entry check done once: the window holds the int tag in every register the region
//! keeps as an int32, so an operand reads its payload unchecked and a result writes the
//! payload alone. Their semantics are the staged rows' (`staged::numeric`) over the proved
//! window (`LaneFrame<ACC_PROVED>`), not a second copy.

use super::*;
use super::field_region::{lane_get_integer_field, lane_set_integer_field};
use super::lane_frame::{LaneFrame, ACC_PROVED};
use crate::bytecode::Operand;
use crate::staged::numeric::scaled_sum;
use crate::staged::{Concrete, Domain, NumberOperation, NumericStage, Operation, Overflow, Step};
use crate::typed_ir::counted_loop::{UnitKind, COUNTED};
use crate::vm::operations::NumberArithmetic;

const _: () = assert!(cfg!(target_endian = "little"), "a window word's payload is its first four bytes");

impl Registers {
    /// The int32 payload of `register`, which the region proved holds the int tag.
    #[inline(always)]
    pub(super) fn payload(self, register: u16) -> u32 {
        // SAFETY: as `get`; the word's low half is its first four bytes.
        unsafe { self.0.add(usize::from(register)).cast::<u32>().read() }
    }

    /// Writes the int32 payload of `register`, leaving the tag half the region proved.
    #[inline(always)]
    pub(super) fn set_payload(self, register: u16, payload: u32) {
        // SAFETY: as `get`.
        unsafe { self.0.add(usize::from(register)).cast::<u32>().write(payload) }
    }
}

/// What a proved record carries on: the run's view without an accumulator, which no record of a
/// region reads or produces.
#[inline(always)]
pub(super) fn carried(view: Carried) -> Carried {
    Carried { acc: Slot::unset(), ..view }
}

/// The staged int32 `operation` on the proved window; its word.
#[inline(always)]
fn proved_step(r: Registers, view: Carried, operation: NumberOperation) -> u64 {
    let mut frame = LaneFrame::<ACC_PROVED>::new(r, view);
    match crate::staged::step(&mut Concrete::new(std::ptr::null()), &mut frame, Operation::Number(operation)) {
        Step::Value(word) => word,
        _ => unreachable!("a proved int32 operation neither leaves nor transfers control"),
    }
}

script_handler! {
    /// A counted loop's update, back edge and test (`lane_inc_dec_loop`) of a proved region: the
    /// step cannot overflow where the test held, the test compares two int32 words, and the
    /// body follows or the loop exits. Only the safepoint leaves, as the lane's back edge does:
    /// the lane resumes at the header, which proves the entry facts again.
    pub(super) fn lane_int_loop<DECREMENT: bool>(cx, r, ip, i, view) {
        let step = NumberOperation::IncDec { destination: i.a, source: i.a, decrement: DECREMENT, overflow: Overflow::Wrap };
        let counter = proved_step(r, view, step);
        if script_below_floor(cx) {
            sibling!(lane_int_safepoint::<H>, r, ip, Carried::script(cx))
        }
        let (_, relation, _) = COUNTED[usize::from(DECREMENT)];
        let holds = Concrete::new(std::ptr::null()).relation(relation, Domain::Int32, counter, u64::from(r.payload(i.c)));
        let jump_ip = following(ip);
        if holds {
            // SAFETY: the back edge records the distance to the loop's header, whose test record is
            // followed by the body.
            let header = unsafe { jump_ip.byte_offset((*jump_ip).jump()) };
            next!(cx, r, following(header), carried(view))
        }
        next!(cx, r, following(jump_ip), carried(view))
    }
}

script_handler! {
    /// `lane_int_loop`'s back edge past the stack floor: the lane's own safepoint jump
    /// (`lane_jump`), out of line, so the loop's record pushes no frame.
    #[inline(never)]
    fn lane_int_safepoint<>(cx, r, ip, i, view) {
        resume!(cx, lane_jump::<H>(r, following(ip), true, Carried::script(cx), None))
    }
}

script_handler! {
    /// The sum or difference of two int32 registers whose result the next record truncates
    /// (`UnitKind::Sum`) in a proved region: the wrapping sum, written to the truncation's register.
    fn lane_int32_sum<SUBTRACT: bool>(cx, r, ip, i, view) {
        // SAFETY: the plan gives a sum its truncation record.
        let truncation = unsafe { &*following(ip) };
        let operation = if SUBTRACT { NumberArithmetic::Subtract } else { NumberArithmetic::Add };
        proved_step(r, view, NumberOperation::Arithmetic {
            destination: truncation.a, left: Operand::register(i.b), right: Operand::register(i.c),
            operation, domain: Domain::Int32, overflow: Overflow::Wrap,
        });
        next!(cx, r, following(following(ip)), carried(view))
    }
}

script_handler! {
    /// `(addend ± factor * k) | 0` (`staged::numeric::scaled_sum`) in a proved region, from the
    /// record of the product `factor * k` (its `b` and constant), the sum after it (whose
    /// operand other than the product is the addend) and the truncation whose register takes
    /// the result: the product and the sum are dead, so neither is written.
    fn lane_int32_scaled_sum<SUBTRACT: bool, SCALED_FIRST: bool>(cx, r, ip, i, view) {
        // SAFETY: the plan gives an island its sum and truncation records.
        let (sum, truncation) = unsafe { (&*following(ip), &*following(following(ip))) };
        let addend = if SCALED_FIRST { sum.c } else { sum.b };
        let operation = if SUBTRACT { NumberArithmetic::Subtract } else { NumberArithmetic::Add };
        let value = scaled_sum(operation, SCALED_FIRST, r.payload(addend) as i32, r.payload(i.b) as i32, i.imm() as i32);
        r.set_payload(truncation.a, value as u32);
        next!(cx, r, following(following(following(ip))), carried(view))
    }
}

/// The proved record of a unit of `kind`.
pub(super) fn proved_handler<H: Host>(kind: UnitKind) -> Handler {
    match kind {
        UnitKind::Sum { subtract: false } => lane_int32_sum::<H, false>,
        UnitKind::Sum { subtract: true } => lane_int32_sum::<H, true>,
        UnitKind::ScaledSum { subtract: false, scaled_first: false } => lane_int32_scaled_sum::<H, false, false>,
        UnitKind::ScaledSum { subtract: false, scaled_first: true } => lane_int32_scaled_sum::<H, false, true>,
        UnitKind::ScaledSum { subtract: true, scaled_first: false } => lane_int32_scaled_sum::<H, true, false>,
        UnitKind::ScaledSum { subtract: true, scaled_first: true } => lane_int32_scaled_sum::<H, true, true>,
        UnitKind::Get => lane_get_integer_field::<H>,
        UnitKind::Set => lane_set_integer_field::<H>,
    }
}

/// The proved update record of a counted loop that steps `decrement`.
pub(super) fn proved_update<H: Host>(decrement: bool) -> Handler {
    if decrement { lane_int_loop::<H, true> } else { lane_int_loop::<H, false> }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The view a test runs its records with: no memory, no accumulator.
    fn view() -> Carried {
        Carried { memory: MemoryView { bytes: std::ptr::null_mut(), reach: Slot::unset() }, integers: crate::value::IntegerEncoding::TAG, acc: Slot::unset() }
    }

    /// The register file after the staged `operation` ran on `values` in the proved frame.
    fn proved(values: [Value; 3], operation: NumberOperation) -> [Value; 3] {
        let mut window = values;
        let mut frame = LaneFrame::<ACC_PROVED>::new(Registers(window.as_mut_ptr()), view());
        let step = crate::staged::step(&mut Concrete::new(std::ptr::null()), &mut frame, Operation::Number(operation));
        assert!(matches!(step, Step::Value(_)));
        window
    }

    const EDGES: [i32; 9] = [0, 1, -1, 7, i32::MAX, i32::MIN, i32::MAX - 1, i32::MIN + 1, 1 << 30];

    /// The Number result of `left op right` truncated to an int32 (`ToInt32`): what `(a op b) | 0` is.
    fn truncated(operation: NumberArithmetic, left: i32, right: i32) -> i32 {
        let exact = if operation == NumberArithmetic::Add { f64::from(left) + f64::from(right) } else { f64::from(left) - f64::from(right) };
        exact as i64 as i32
    }

    #[test]
    fn the_proved_frame_defines_the_truncated_number_result_in_the_tagged_window() {
        for operation in [NumberArithmetic::Add, NumberArithmetic::Subtract] {
            for left in EDGES { for right in EDGES {
                let values = [Value::integer(left), Value::integer(right), Value::integer(0)];
                let after = proved(values, NumberOperation::Arithmetic {
                    destination: 2, left: Operand::register(0), right: Operand::register(1),
                    operation, domain: Domain::Int32, overflow: Overflow::Wrap,
                });
                let expected = [values[0], values[1], Value::integer(truncated(operation, left, right))];
                assert_eq!(after, expected, "{operation:?} {left} {right}");
            }}
        }
    }

    #[test]
    fn the_proved_frame_steps_a_counter_in_place_with_wrapping() {
        for decrement in [false, true] {
            for value in EDGES {
                let values = [Value::integer(value), Value::integer(0), Value::integer(0)];
                let after = proved(values, NumberOperation::IncDec { destination: 0, source: 0, decrement, overflow: Overflow::Wrap });
                let expected = if decrement { value.wrapping_sub(1) } else { value.wrapping_add(1) };
                assert_eq!(after[0], Value::integer(expected), "{decrement} {value}");
                assert_eq!(after[1..], values[1..]);
            }
        }
    }

    #[test]
    fn a_payload_write_keeps_the_tag_half_the_entry_proved() {
        let mut window = [Value::integer(5), Value::integer(i32::MIN)];
        let registers = Registers(window.as_mut_ptr());
        registers.set_payload(0, u32::MAX);
        registers.set_payload(1, 0);
        assert_eq!(registers.payload(0), u32::MAX);
        assert_eq!(window, [Value::integer(-1), Value::integer(0)]);
    }
}
