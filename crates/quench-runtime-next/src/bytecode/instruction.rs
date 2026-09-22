use super::{Effect, Op, RETURN_REGISTER, Register, SET_THIS_REGISTER};

#[derive(Clone, Copy, Debug)]
pub(crate) struct WideInstruction {
    op: Op,
    a: u16,
    b: u16,
    c: u16,
    imm: u32,
}

impl WideInstruction {
    pub(crate) const fn new(op: Op, a: u16, b: u16, c: u16, imm: u32) -> Self {
        Self { op, a, b, c, imm }
    }

    pub(crate) const fn op(self) -> Op {
        self.op
    }

    pub(crate) const fn a(self) -> u16 {
        self.a
    }

    pub(crate) const fn b(self) -> u16 {
        self.b
    }

    pub(crate) const fn c(self) -> u16 {
        self.c
    }

    pub(crate) const fn imm(self) -> u32 {
        self.imm
    }

    pub(crate) fn set_op(&mut self, op: Op) {
        self.op = op;
    }

    pub(crate) fn set_imm(&mut self, imm: u32) {
        self.imm = imm;
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Instr(u64);

impl std::fmt::Debug for Instr {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        output
            .debug_struct("Instr")
            .field("op", &self.op())
            .field("a", &self.a())
            .field("b", &self.b())
            .field("c", &self.c())
            .field("imm", &self.imm())
            .finish()
    }
}

const _: () = assert!(std::mem::size_of::<Instr>() == 8);
const _: () = assert!(Op::COUNT <= 1 << Instr::OP_BITS);

impl Instr {
    const OP_BITS: u32 = 6;
    const FIELD_BITS: u32 = 14;
    const FIELD_MASK: u64 = (1 << Self::FIELD_BITS) - 1;
    const A_SHIFT: u32 = Self::OP_BITS;
    const B_SHIFT: u32 = Self::A_SHIFT + Self::FIELD_BITS;
    const C_SHIFT: u32 = Self::B_SHIFT + Self::FIELD_BITS;
    const IMM_SHIFT: u32 = Self::C_SHIFT + Self::FIELD_BITS;

    pub(crate) fn new(op: Op, a: Register, b: Register, c: Register, imm: u32) -> Self {
        Self::try_new(op, a, b, c, imm).expect("instruction exceeds packed domain")
    }

    pub(crate) fn try_new(op: Op, a: Register, b: Register, c: Register, imm: u32) -> Option<Self> {
        let a = Self::pack_field(op, 0, a)?;
        let b = Self::pack_field(op, 1, b)?;
        let c = Self::pack_field(op, 2, c)?;
        let imm = Self::pack_immediate(op, imm)?;
        Some(Self(
            op as u64
                | (u64::from(a) << Self::A_SHIFT)
                | (u64::from(b) << Self::B_SHIFT)
                | (u64::from(c) << Self::C_SHIFT)
                | (u64::from(imm) << Self::IMM_SHIFT),
        ))
    }

    pub(crate) fn wide(index: usize) -> Option<Self> {
        let index = u64::try_from(index).ok()?;
        let max = (1_u64 << (Self::FIELD_BITS * 3 + 16)) - 1;
        (index <= max).then_some(Self(
            Op::Wide as u64
                | ((index & Self::FIELD_MASK) << Self::A_SHIFT)
                | (((index >> Self::FIELD_BITS) & Self::FIELD_MASK) << Self::B_SHIFT)
                | (((index >> (Self::FIELD_BITS * 2)) & Self::FIELD_MASK) << Self::C_SHIFT)
                | ((index >> (Self::FIELD_BITS * 3)) << Self::IMM_SHIFT),
        ))
    }

    pub(crate) const fn is_wide(self) -> bool {
        matches!(self.op(), Op::Wide)
    }

    pub(crate) const fn wide_index(self) -> usize {
        let index = ((self.0 >> Self::A_SHIFT) & Self::FIELD_MASK)
            | (((self.0 >> Self::B_SHIFT) & Self::FIELD_MASK) << Self::FIELD_BITS)
            | (((self.0 >> Self::C_SHIFT) & Self::FIELD_MASK) << (Self::FIELD_BITS * 2))
            | ((self.0 >> Self::IMM_SHIFT) << (Self::FIELD_BITS * 3));
        index as usize
    }

    pub(crate) const fn as_wide(self) -> WideInstruction {
        WideInstruction::new(self.op(), self.a(), self.b(), self.c(), self.imm())
    }

    pub(crate) const fn op(self) -> Op {
        // SAFETY: construction and residual decoding reject IDs outside the
        // contiguous macro-generated opcode range.
        unsafe { std::mem::transmute::<u16, Op>((self.0 & ((1 << Self::OP_BITS) - 1)) as u16) }
    }

    pub(crate) const fn a(self) -> u16 {
        if self.is_wide() {
            return ((self.0 >> Self::A_SHIFT) & Self::FIELD_MASK) as u16;
        }
        Self::unpack_field(self.op(), 0, (self.0 >> Self::A_SHIFT) as u16)
    }

    pub(crate) const fn b(self) -> u16 {
        if self.is_wide() {
            return ((self.0 >> Self::B_SHIFT) & Self::FIELD_MASK) as u16;
        }
        Self::unpack_field(self.op(), 1, (self.0 >> Self::B_SHIFT) as u16)
    }

    pub(crate) const fn c(self) -> u16 {
        if self.is_wide() {
            return ((self.0 >> Self::C_SHIFT) & Self::FIELD_MASK) as u16;
        }
        Self::unpack_field(self.op(), 2, (self.0 >> Self::C_SHIFT) as u16)
    }

    pub(crate) const fn imm(self) -> u32 {
        if self.is_wide() {
            return (self.0 >> Self::IMM_SHIFT) as u32;
        }
        Self::unpack_immediate(self.op(), (self.0 >> Self::IMM_SHIFT) as u16)
    }

    pub(crate) fn set_op(&mut self, value: Op) {
        *self = Self::new(value, self.a(), self.b(), self.c(), self.imm());
    }

    pub(crate) fn set_a(&mut self, value: u16) {
        *self = Self::new(self.op(), value, self.b(), self.c(), self.imm());
    }

    pub(crate) fn set_b(&mut self, value: u16) {
        *self = Self::new(self.op(), self.a(), value, self.c(), self.imm());
    }

    pub(crate) fn set_c(&mut self, value: u16) {
        *self = Self::new(self.op(), self.a(), self.b(), value, self.imm());
    }

    pub(crate) fn set_imm(&mut self, value: u32) {
        *self = Self::new(self.op(), self.a(), self.b(), self.c(), value);
    }

    const fn compound(op: Op) -> bool {
        matches!(
            op,
            Op::LoadCapture | Op::StoreCapture | Op::Call | Op::CallKnown
        )
    }

    const fn pack_field(op: Op, position: usize, value: u16) -> Option<u16> {
        if matches!(op, Op::GetField) && position == 1 && value >= u16::MAX - 1 {
            return Some(0x3ffe + (value == u16::MAX) as u16);
        }
        if value & 0x3000 != 0 {
            return None;
        }
        Some((value & 0x0fff) | ((value >> 14) << 12))
    }

    const fn unpack_field(op: Op, position: usize, packed: u16) -> u16 {
        let packed = packed & Self::FIELD_MASK as u16;
        if matches!(op, Op::GetField) && position == 1 && packed >= 0x3ffe {
            return u16::MAX - (packed == 0x3ffe) as u16;
        }
        (packed & 0x0fff) | ((packed >> 12) << 14)
    }

    const fn pack_immediate(op: Op, value: u32) -> Option<u16> {
        if Self::compound(op) {
            let high = value >> 16;
            let low = value & u16::MAX as u32;
            if high > u8::MAX as u32 || low > u8::MAX as u32 {
                return None;
            }
            Some(((high as u16) << 8) | low as u16)
        } else if value <= u16::MAX as u32 {
            Some(value as u16)
        } else {
            None
        }
    }

    const fn unpack_immediate(op: Op, packed: u16) -> u32 {
        if Self::compound(op) {
            (((packed >> 8) as u32) << 16) | ((packed & 0xff) as u32)
        } else {
            packed as u32
        }
    }

    pub(crate) const fn effect(self) -> Effect {
        let mut effect = self.op().effect();
        if matches!(self.op(), Op::GetField) && self.a() & SET_THIS_REGISTER != 0 {
            effect = effect.union(Effect::WRITES_HEAP);
        }
        if matches!(
            self.op(),
            Op::Binary
                | Op::NumericAdd
                | Op::NumericMultiply
                | Op::GetIterator
                | Op::GetField
                | Op::Call
                | Op::CallKnown
                | Op::CallMethod
                | Op::CallThisMethod
                | Op::Construct
                | Op::MakeObject2
                | Op::SuperConstArrayObject2
        ) && self.a() & RETURN_REGISTER != 0
        {
            effect = effect.union(Effect::CONTROL);
        }
        effect
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_word_preserves_tagged_fields() {
        let values = [0, 0x0fff, 0x4000, 0x8000, 0xcfff];
        for value in values {
            let instruction = Instr::new(Op::Binary, value, value, value, 65535);
            assert_eq!(
                (instruction.a(), instruction.b(), instruction.c()),
                (value, value, value)
            );
            assert_eq!(instruction.imm(), 65535);
        }
        assert_eq!(std::mem::size_of::<Instr>(), 8);
    }

    #[test]
    fn packed_word_preserves_declared_special_domains() {
        for base in [u16::MAX - 1, u16::MAX] {
            assert_eq!(Instr::new(Op::GetField, 0, base, 0, 0).b(), base);
        }
        let compound = (255 << 16) | 255;
        assert_eq!(Instr::new(Op::Call, 0, 0, 0, compound).imm(), compound);
    }

    #[test]
    fn packed_word_rejects_every_overflow_axis() {
        assert!(Instr::try_new(Op::Binary, 0x1000, 0, 0, 0).is_none());
        assert!(Instr::try_new(Op::Binary, 0, 0x2000, 0, 0).is_none());
        assert!(Instr::try_new(Op::Binary, 0, 0, 0x3000, 0).is_none());
        assert!(Instr::try_new(Op::Binary, 0, 0, 0, 65536).is_none());
        assert!(Instr::try_new(Op::Call, 0, 0, 0, 256 << 16).is_none());
        assert!(Instr::try_new(Op::Call, 0, 0, 0, 256).is_none());
    }

    #[test]
    fn wide_marker_round_trips_the_full_side_table_index() {
        let index = (1_usize << 56) | (0x1234 << 28) | (0x2345 << 14) | 0x3456;
        let instruction = Instr::wide(index).unwrap();
        assert!(instruction.is_wide());
        assert_eq!(instruction.wide_index(), index);
        assert_eq!(
            (instruction.a(), instruction.b(), instruction.c()),
            (0x3456, 0x2345, 0x1234)
        );
    }
}
