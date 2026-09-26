use super::{
    Effect, FieldLayout, ImmediateLayout, InstructionField, Op, REGISTER_MASK, RETURN_REGISTER,
    Register, ResultLayout, SET_THIS_REGISTER,
};

const PACKED_PAIR_LOW_BITS: u32 = 8;
const PACKED_PAIR_HIGH_BITS: u32 = 7;
const PACKED_PAIR_SOURCE_SHIFT: u32 = u16::BITS;
const PACKED_PAIR_SOURCE_MASK: u32 = u16::MAX as u32;
const PACKED_PAIR_LOW_MASK: u32 = (1 << PACKED_PAIR_LOW_BITS) - 1;
const PACKED_PAIR_HIGH_MASK: u32 = (1 << PACKED_PAIR_HIGH_BITS) - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RegisterWindow {
    pub(crate) base: Register,
    pub(crate) count: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConstructArguments {
    Registers(RegisterWindow),
    Array(Register),
}

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

macro_rules! layout_accessors {
    ($instruction:ty) => {
        impl $instruction {
            pub(crate) fn result_register(self) -> Register {
                self.a() & REGISTER_MASK
            }

            pub(crate) fn returns_from_frame(self) -> bool {
                self.op().result_layout().allows_return() && self.a() & RETURN_REGISTER != 0
            }

            pub(crate) fn writes_current_this(self) -> bool {
                self.op().result_layout().allows_this_write() && self.a() & SET_THIS_REGISTER != 0
            }

            #[allow(dead_code)]
            pub(crate) fn writes_numeric_local(self) -> bool {
                self.op().result_layout().allows_numeric_local()
                    && self.a() & super::NUMERIC_LOCAL_TARGET != 0
            }

            #[allow(dead_code)]
            pub(crate) fn result_flags_valid(self) -> bool {
                let layout: ResultLayout = self.op().result_layout();
                self.a() & !REGISTER_MASK & !layout.allowed_flags() == 0
            }

            pub(crate) fn call_window(self) -> RegisterWindow {
                debug_assert!(matches!(
                    self.op().immediate_layout(),
                    ImmediateLayout::CallWindow | ImmediateLayout::CallWindowWithEvalFlags
                ));
                let immediate = self.imm();
                RegisterWindow {
                    base: ImmediateLayout::call_window_base(immediate),
                    count: ImmediateLayout::argument_count(immediate),
                }
            }

            #[allow(dead_code)]
            pub(crate) fn known_function_index(self) -> u16 {
                debug_assert_eq!(
                    self.op().field_layout(InstructionField::B),
                    FieldLayout::FunctionIndex
                );
                self.b()
            }

            #[allow(dead_code)]
            pub(crate) fn capture_depth(self) -> u16 {
                debug_assert_eq!(
                    self.op().immediate_layout(),
                    ImmediateLayout::CaptureDepthAndSlot
                );
                ImmediateLayout::capture_depth(self.imm())
            }

            #[allow(dead_code)]
            pub(crate) fn capture_slot(self) -> u16 {
                debug_assert_eq!(
                    self.op().immediate_layout(),
                    ImmediateLayout::CaptureDepthAndSlot
                );
                ImmediateLayout::capture_slot(self.imm())
            }

            pub(crate) fn register_pair(self) -> (Register, Register) {
                debug_assert_eq!(self.op().immediate_layout(), ImmediateLayout::RegisterPair);
                ImmediateLayout::register_pair(self.imm())
            }

            pub(crate) fn construct_arguments(self) -> ConstructArguments {
                debug_assert_eq!(
                    self.op().immediate_layout(),
                    ImmediateLayout::ConstructCountAndFlags
                );
                debug_assert_eq!(
                    self.op().field_layout(InstructionField::C),
                    FieldLayout::ConstructArguments
                );
                let base = self.c();
                if ImmediateLayout::construct_array_arguments(self.imm()) {
                    ConstructArguments::Array(base)
                } else {
                    ConstructArguments::Registers(RegisterWindow {
                        base,
                        count: ImmediateLayout::argument_count(self.imm()),
                    })
                }
            }

            #[allow(dead_code)]
            pub(crate) fn is_super_construct(self) -> bool {
                debug_assert_eq!(
                    self.op().immediate_layout(),
                    ImmediateLayout::ConstructCountAndFlags
                );
                ImmediateLayout::super_construct(self.imm())
            }
        }
    };
}

layout_accessors!(WideInstruction);

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
    // Keep the packed instruction at 64 bits while allowing the opcode set to
    // grow. The explicit Wide form carries full-width operands when this
    // slightly smaller narrow immediate is insufficient.
    const OP_BITS: u32 = 7;
    const FIELD_BITS: u32 = 14;
    const FIELD_MASK: u64 = (1 << Self::FIELD_BITS) - 1;
    const FIELD_TAG_BITS: u32 = 2;
    const FIELD_PAYLOAD_BITS: u32 = Self::FIELD_BITS - Self::FIELD_TAG_BITS;
    const FIELD_TAG_SHIFT: u32 = Self::FIELD_BITS - Self::FIELD_TAG_BITS;
    const FIELD_RESERVED_TAG_MASK: u16 = ((1 << Self::FIELD_TAG_BITS) - 1) << Self::FIELD_TAG_SHIFT;
    const FIELD_PAYLOAD_MASK: u16 = (1 << Self::FIELD_PAYLOAD_BITS) - 1;
    const FIELD_SOURCE_HIGH_SHIFT: u32 = Self::FIELD_BITS;
    const FIELD_PACKED_HIGH_SHIFT: u32 = Self::FIELD_PAYLOAD_BITS;
    const FIELD_SENTINEL: u16 = Self::FIELD_MASK as u16 - 1;
    const FIELD_SENTINEL_NEXT: u16 = Self::FIELD_SENTINEL + 1;
    const FIELD_SENTINEL_START: u16 = u16::MAX - 1;
    const A_SHIFT: u32 = Self::OP_BITS;
    const B_SHIFT: u32 = Self::A_SHIFT + Self::FIELD_BITS;
    const C_SHIFT: u32 = Self::B_SHIFT + Self::FIELD_BITS;
    const IMM_SHIFT: u32 = Self::C_SHIFT + Self::FIELD_BITS;
    const NARROW_IMMEDIATE_BITS: u32 = u64::BITS - Self::IMM_SHIFT;
    const NARROW_IMMEDIATE_MASK: u32 = (1 << Self::NARROW_IMMEDIATE_BITS) - 1;

    #[cfg(feature = "profile-aggregate")]
    pub(crate) const fn field_fits(op: Op, position: usize, value: u16) -> bool {
        Self::pack_field(op, position, value).is_some()
    }

    #[cfg(feature = "profile-aggregate")]
    pub(crate) const fn immediate_fits(op: Op, value: u32) -> bool {
        Self::pack_immediate(op, value).is_some()
    }

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
        let max = (1_u64 << (Self::FIELD_BITS * 3 + (64 - Self::IMM_SHIFT))) - 1;
        (index <= max).then_some(Self(
            Op::Wide as u64
                | ((index & Self::FIELD_MASK) << Self::A_SHIFT)
                | (((index >> Self::FIELD_BITS) & Self::FIELD_MASK) << Self::B_SHIFT)
                | (((index >> (Self::FIELD_BITS * 2)) & Self::FIELD_MASK) << Self::C_SHIFT)
                | ((index >> (Self::FIELD_BITS * 3)) << Self::IMM_SHIFT),
        ))
    }

    pub(crate) fn wide_from_fields(a: u16, b: u16, c: u16, imm: u32) -> Option<Self> {
        if imm > Self::NARROW_IMMEDIATE_MASK {
            return None;
        }
        let index = u64::from(a)
            | (u64::from(b) << Self::FIELD_BITS)
            | (u64::from(c) << (Self::FIELD_BITS * 2))
            | (u64::from(imm) << (Self::FIELD_BITS * 3));
        Self::wide(usize::try_from(index).ok()?)
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
        if self.is_wide() {
            assert_eq!(value, Op::Wide, "wide instruction reference is immutable");
            return;
        }
        *self = Self::new(value, self.a(), self.b(), self.c(), self.imm());
    }

    pub(crate) fn set_a(&mut self, value: u16) {
        *self = Self::new(self.op(), value, self.b(), self.c(), self.imm());
    }

    pub(crate) fn set_returns_from_frame(&mut self) {
        debug_assert!(self.op().result_layout().allows_return());
        self.set_a(self.a() | RETURN_REGISTER);
    }

    pub(crate) fn set_this_result(&mut self) {
        debug_assert!(self.op().result_layout().allows_this_write());
        self.set_a(self.a() | SET_THIS_REGISTER);
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

    const fn pack_field(op: Op, position: usize, value: u16) -> Option<u16> {
        if matches!(op, Op::GetField) && position == 1 && value >= Self::FIELD_SENTINEL_START {
            return Some(if value == u16::MAX {
                Self::FIELD_SENTINEL_NEXT
            } else {
                Self::FIELD_SENTINEL
            });
        }
        if value & Self::FIELD_RESERVED_TAG_MASK != 0 {
            return None;
        }
        Some(
            (value & Self::FIELD_PAYLOAD_MASK)
                | ((value >> Self::FIELD_SOURCE_HIGH_SHIFT) << Self::FIELD_PACKED_HIGH_SHIFT),
        )
    }

    const fn unpack_field(op: Op, position: usize, packed: u16) -> u16 {
        let packed = packed & Self::FIELD_MASK as u16;
        if matches!(op, Op::GetField) && position == 1 && packed >= Self::FIELD_SENTINEL {
            return if packed == Self::FIELD_SENTINEL {
                Self::FIELD_SENTINEL_START
            } else {
                u16::MAX
            };
        }
        (packed & Self::FIELD_PAYLOAD_MASK)
            | ((packed >> Self::FIELD_PACKED_HIGH_SHIFT) << Self::FIELD_SOURCE_HIGH_SHIFT)
    }

    const fn pack_immediate(op: Op, value: u32) -> Option<u16> {
        if op.immediate_layout().uses_packed_pair() {
            let high = value >> PACKED_PAIR_SOURCE_SHIFT;
            let low = value & PACKED_PAIR_SOURCE_MASK;
            if high > PACKED_PAIR_HIGH_MASK || low > PACKED_PAIR_LOW_MASK {
                return None;
            }
            Some(((high as u16) << PACKED_PAIR_LOW_BITS) | low as u16)
        } else if value <= Self::NARROW_IMMEDIATE_MASK {
            Some(value as u16)
        } else {
            None
        }
    }

    const fn unpack_immediate(op: Op, packed: u16) -> u32 {
        if op.immediate_layout().uses_packed_pair() {
            (((packed >> PACKED_PAIR_LOW_BITS) as u32) << PACKED_PAIR_SOURCE_SHIFT)
                | ((packed as u32) & PACKED_PAIR_LOW_MASK)
        } else {
            packed as u32
        }
    }

    pub(crate) fn effect(self) -> Effect {
        let mut effect = self.op().effect();
        if self.writes_current_this() {
            effect = effect.union(Effect::WRITES_HEAP);
        }
        if self.returns_from_frame() {
            effect = effect.union(Effect::CONTROL);
        }
        effect
    }
}

layout_accessors!(Instr);

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
        assert!(Instr::try_new(Op::Binary, 0, 0, Instr::FIELD_RESERVED_TAG_MASK, 0).is_none());
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
