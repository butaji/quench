//! The interpreter's register frame: `staged` moves and constants over the live file.

use super::*;
use crate::bytecode::Register;
use crate::staged::{Concrete, Constant, Frame, Operation};

/// The interpreter's frame: the live register file and the accumulator view a producer
/// carries. `ACC` names the record's accumulator field (`read`).
struct LaneFrame<const ACC: u8> {
    registers: Registers,
    view: Carried,
}

impl<const ACC: u8> Frame<Concrete> for LaneFrame<ACC> {
    type Fact = ();

    #[inline(always)]
    fn fact(&self, _register: Register) -> Self::Fact {}

    #[inline(always)]
    fn read(&mut self, _stage: &mut Concrete, register: Register) -> u64 {
        read::<ACC, ACC_B>(self.registers, register, self.view).to_bits()
    }

    #[inline(always)]
    fn write(&mut self, _stage: &mut Concrete, register: Register, value: u64, _fact: Self::Fact) {
        self.registers.set(register, Value::from_bits(value));
    }

    #[inline(always)]
    fn constant(&mut self, _stage: &mut Concrete, constant: Constant) -> (u64, Self::Fact) {
        (constant.value.to_bits(), ())
    }
}

lane_handler! {
    pub(super) fn lane_move<ACC: u8>(cx, r, ip, i, view) {
        let mut stage = Concrete::new(std::ptr::null());
        let mut frame = LaneFrame::<ACC> { registers: r, view };
        let value = crate::staged::step(&mut stage, &mut frame, Operation::Move {
            destination: i.a, source: i.b });
        next!(cx, r, following(ip), Carried { acc: Slot::of(Value::from_bits(value)), ..view })
    }
}

lane_handler! {
    pub(super) fn lane_load_const<>(cx, r, ip, i, view) {
        let mut stage = Concrete::new(std::ptr::null());
        let mut frame = LaneFrame::<ACC_NONE> { registers: r, view };
        let value = crate::staged::step(&mut stage, &mut frame, Operation::LoadConst {
            destination: i.a, constant: Constant::untyped(i.constant()) });
        next!(cx, r, following(ip), Carried { acc: Slot::of(Value::from_bits(value)), ..view })
    }
}
