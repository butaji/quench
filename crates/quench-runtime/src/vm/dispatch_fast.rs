//! The fast lane runs straight-line instructions whose whole effect is a
//! register write, a Wasm linear-memory access or a jump. Fields decode at
//! fixed positions and no frame state is published. The lane stops before any
//! instruction that would collect, trap, throw, call or touch a shared memory,
//! and the general path executes that instruction with its full semantics, so
//! lane execution is always an exact prefix of general execution.

use super::*;
use crate::bytecode::{Instr, WideInstruction};
use crate::wasm::integer::{I32BinaryOperator, I32UnaryOperator};
use crate::wasm::memory::{MemoryLoad, MemoryStorage, MemoryStore};

/// Operand access shared by narrow and wide encodings of lane opcodes.
pub(super) trait LaneFields: Copy {
    fn a(self) -> u16;
    fn b(self) -> u16;
    fn c(self) -> u16;
    fn imm(self) -> u32;
    fn pair(self) -> (u16, u16);
}

/// A narrow instruction of a lane opcode, decoded without layout checks.
#[derive(Clone, Copy)]
struct Narrow(Instr);

impl LaneFields for Narrow {
    #[inline(always)]
    fn a(self) -> u16 {
        self.0.plain_a()
    }
    #[inline(always)]
    fn b(self) -> u16 {
        self.0.plain_b()
    }
    #[inline(always)]
    fn c(self) -> u16 {
        self.0.plain_c()
    }
    #[inline(always)]
    fn imm(self) -> u32 {
        self.0.plain_imm()
    }
    #[inline(always)]
    fn pair(self) -> (u16, u16) {
        self.0.register_pair()
    }
}

impl LaneFields for WideInstruction {
    #[inline(always)]
    fn a(self) -> u16 {
        WideInstruction::a(self)
    }
    #[inline(always)]
    fn b(self) -> u16 {
        WideInstruction::b(self)
    }
    #[inline(always)]
    fn c(self) -> u16 {
        WideInstruction::c(self)
    }
    #[inline(always)]
    fn imm(self) -> u32 {
        WideInstruction::imm(self)
    }
    #[inline(always)]
    fn pair(self) -> (u16, u16) {
        self.register_pair()
    }
}

impl<H: Host> Vm<H> {
    /// Run lane instructions from `pc` until one needs the general path.
    #[inline(always)]
    pub(super) fn run_fast_lane(
        &mut self,
        program: ProgramId,
        frame: usize,
        code: *const Instr,
        wide: *const WideInstruction,
        pc: &mut usize,
    ) {
        loop {
            // SAFETY: residual validation establishes every reachable PC and
            // wide index; the caller passes the active function's code.
            let packed = unsafe { *code.add(*pc) };
            let next = if packed.is_wide() {
                let instruction = unsafe { *wide.add(packed.wide_index()) };
                self.lane_step(program, frame, instruction.op(), instruction, *pc)
            } else {
                self.lane_step(program, frame, packed.op(), Narrow(packed), *pc)
            };
            match next {
                Some(next) => *pc = next,
                None => return,
            }
        }
    }

    /// The next PC after completing one instruction, or `None` to leave it
    /// to the general path untouched.
    #[inline(always)]
    fn lane_step<I: LaneFields>(
        &mut self,
        program: ProgramId,
        f: usize,
        op: Op,
        i: I,
        pc: usize,
    ) -> Option<usize> {
        let next = pc + 1;
        match op {
            Op::Move => self.write(f, i.a(), self.read(f, i.b())),
            Op::LoadConst => {
                let value = self.programs.constant(program, i.imm() as usize)?;
                self.write(f, i.a(), value);
            }
            Op::LoadLocalPlain => {
                // SAFETY: validated bytecode bounds the slot by Function.locals,
                // and frame setup sizes locals to that count.
                let value = unsafe { self.read_validated_local(f, i.imm() as usize) };
                self.write(f, i.a(), value);
            }
            Op::Jump => return self.lane_jump(i.imm(), pc, true),
            Op::JumpFalse => {
                let value = self.read(f, i.a());
                let truthy = match value.as_int() {
                    Some(integer) => integer != 0,
                    None if value == Value::TRUE => true,
                    None if value == Value::FALSE => false,
                    None => return None,
                };
                if !truthy {
                    return self.lane_jump(i.imm(), pc, true);
                }
            }
            Op::WasmJumpI32Zero => {
                if self.read(f, i.a()).wasm_bits32() == 0 {
                    return self.lane_jump(i.imm(), pc, false);
                }
            }
            Op::WasmJumpI32NonZero => {
                if self.read(f, i.a()).wasm_bits32() != 0 {
                    return self.lane_jump(i.imm(), pc, false);
                }
            }
            Op::WasmJumpI32Equal => {
                return self.lane_compare_jump(f, i, pc, I32BinaryOperator::Equal);
            }
            Op::WasmJumpI32NotEqual => {
                return self.lane_compare_jump(f, i, pc, I32BinaryOperator::NotEqual);
            }
            Op::WasmJumpI32LessSigned => {
                return self.lane_compare_jump(f, i, pc, I32BinaryOperator::LessSigned);
            }
            Op::WasmJumpI32LessUnsigned => {
                return self.lane_compare_jump(f, i, pc, I32BinaryOperator::LessUnsigned);
            }
            Op::WasmJumpI32GreaterSigned => {
                return self.lane_compare_jump(f, i, pc, I32BinaryOperator::GreaterSigned);
            }
            Op::WasmJumpI32GreaterUnsigned => {
                return self.lane_compare_jump(f, i, pc, I32BinaryOperator::GreaterUnsigned);
            }
            Op::WasmJumpI32LessEqualSigned => {
                return self.lane_compare_jump(f, i, pc, I32BinaryOperator::LessEqualSigned);
            }
            Op::WasmJumpI32LessEqualUnsigned => {
                return self.lane_compare_jump(f, i, pc, I32BinaryOperator::LessEqualUnsigned);
            }
            Op::WasmJumpI32GreaterEqualSigned => {
                return self.lane_compare_jump(f, i, pc, I32BinaryOperator::GreaterEqualSigned);
            }
            Op::WasmJumpI32GreaterEqualUnsigned => {
                return self.lane_compare_jump(f, i, pc, I32BinaryOperator::GreaterEqualUnsigned);
            }
            Op::WasmSelect => {
                let (condition, _) = i.pair();
                let source = if self.read(f, condition).wasm_bits32() != 0 {
                    i.b()
                } else {
                    i.c()
                };
                self.write(f, i.a(), self.read(f, source));
            }
            Op::WasmFillRegisters => {
                let value = self.programs.constant(program, i.imm() as usize)?;
                for offset in 0..i.c() {
                    self.write(f, i.b() + offset, value);
                }
            }
            Op::WasmI32Unary => {
                let operator = I32UnaryOperator::from_tag(i.imm())?;
                let crate::WasmValue::I32(value) =
                    operator.apply(self.read(f, i.b()).wasm_bits32() as i32)
                else {
                    return None;
                };
                self.write(f, i.a(), Value::integer(value));
            }
            Op::WasmI32Add => self.lane_i32(f, i, I32BinaryOperator::Add, false)?,
            Op::WasmI32Subtract => self.lane_i32(f, i, I32BinaryOperator::Subtract, false)?,
            Op::WasmI32Multiply => self.lane_i32(f, i, I32BinaryOperator::Multiply, false)?,
            Op::WasmI32DivideSigned => {
                self.lane_i32(f, i, I32BinaryOperator::DivideSigned, false)?
            }
            Op::WasmI32DivideUnsigned => {
                self.lane_i32(f, i, I32BinaryOperator::DivideUnsigned, false)?
            }
            Op::WasmI32RemainderSigned => {
                self.lane_i32(f, i, I32BinaryOperator::RemainderSigned, false)?
            }
            Op::WasmI32RemainderUnsigned => {
                self.lane_i32(f, i, I32BinaryOperator::RemainderUnsigned, false)?
            }
            Op::WasmI32And => self.lane_i32(f, i, I32BinaryOperator::And, false)?,
            Op::WasmI32Or => self.lane_i32(f, i, I32BinaryOperator::Or, false)?,
            Op::WasmI32Xor => self.lane_i32(f, i, I32BinaryOperator::Xor, false)?,
            Op::WasmI32ShiftLeft => self.lane_i32(f, i, I32BinaryOperator::ShiftLeft, false)?,
            Op::WasmI32ShiftRightSigned => {
                self.lane_i32(f, i, I32BinaryOperator::ShiftRightSigned, false)?
            }
            Op::WasmI32ShiftRightUnsigned => {
                self.lane_i32(f, i, I32BinaryOperator::ShiftRightUnsigned, false)?
            }
            Op::WasmI32RotateLeft => self.lane_i32(f, i, I32BinaryOperator::RotateLeft, false)?,
            Op::WasmI32RotateRight => self.lane_i32(f, i, I32BinaryOperator::RotateRight, false)?,
            Op::WasmI32Equal => self.lane_i32(f, i, I32BinaryOperator::Equal, false)?,
            Op::WasmI32NotEqual => self.lane_i32(f, i, I32BinaryOperator::NotEqual, false)?,
            Op::WasmI32LessSigned => self.lane_i32(f, i, I32BinaryOperator::LessSigned, false)?,
            Op::WasmI32LessUnsigned => {
                self.lane_i32(f, i, I32BinaryOperator::LessUnsigned, false)?
            }
            Op::WasmI32GreaterSigned => {
                self.lane_i32(f, i, I32BinaryOperator::GreaterSigned, false)?
            }
            Op::WasmI32GreaterUnsigned => {
                self.lane_i32(f, i, I32BinaryOperator::GreaterUnsigned, false)?
            }
            Op::WasmI32LessEqualSigned => {
                self.lane_i32(f, i, I32BinaryOperator::LessEqualSigned, false)?
            }
            Op::WasmI32LessEqualUnsigned => {
                self.lane_i32(f, i, I32BinaryOperator::LessEqualUnsigned, false)?
            }
            Op::WasmI32GreaterEqualSigned => {
                self.lane_i32(f, i, I32BinaryOperator::GreaterEqualSigned, false)?
            }
            Op::WasmI32GreaterEqualUnsigned => {
                self.lane_i32(f, i, I32BinaryOperator::GreaterEqualUnsigned, false)?
            }
            Op::WasmI32AddImmediate => self.lane_i32(f, i, I32BinaryOperator::Add, true)?,
            Op::WasmI32SubtractImmediate => {
                self.lane_i32(f, i, I32BinaryOperator::Subtract, true)?
            }
            Op::WasmI32MultiplyImmediate => {
                self.lane_i32(f, i, I32BinaryOperator::Multiply, true)?
            }
            Op::WasmI32DivideSignedImmediate => {
                self.lane_i32(f, i, I32BinaryOperator::DivideSigned, true)?
            }
            Op::WasmI32DivideUnsignedImmediate => {
                self.lane_i32(f, i, I32BinaryOperator::DivideUnsigned, true)?
            }
            Op::WasmI32RemainderSignedImmediate => {
                self.lane_i32(f, i, I32BinaryOperator::RemainderSigned, true)?
            }
            Op::WasmI32RemainderUnsignedImmediate => {
                self.lane_i32(f, i, I32BinaryOperator::RemainderUnsigned, true)?
            }
            Op::WasmI32AndImmediate => self.lane_i32(f, i, I32BinaryOperator::And, true)?,
            Op::WasmI32OrImmediate => self.lane_i32(f, i, I32BinaryOperator::Or, true)?,
            Op::WasmI32XorImmediate => self.lane_i32(f, i, I32BinaryOperator::Xor, true)?,
            Op::WasmI32ShiftLeftImmediate => {
                self.lane_i32(f, i, I32BinaryOperator::ShiftLeft, true)?
            }
            Op::WasmI32ShiftRightSignedImmediate => {
                self.lane_i32(f, i, I32BinaryOperator::ShiftRightSigned, true)?
            }
            Op::WasmI32ShiftRightUnsignedImmediate => {
                self.lane_i32(f, i, I32BinaryOperator::ShiftRightUnsigned, true)?
            }
            Op::WasmI32RotateLeftImmediate => {
                self.lane_i32(f, i, I32BinaryOperator::RotateLeft, true)?
            }
            Op::WasmI32RotateRightImmediate => {
                self.lane_i32(f, i, I32BinaryOperator::RotateRight, true)?
            }
            Op::WasmI32EqualImmediate => self.lane_i32(f, i, I32BinaryOperator::Equal, true)?,
            Op::WasmI32NotEqualImmediate => {
                self.lane_i32(f, i, I32BinaryOperator::NotEqual, true)?
            }
            Op::WasmI32LessSignedImmediate => {
                self.lane_i32(f, i, I32BinaryOperator::LessSigned, true)?
            }
            Op::WasmI32LessUnsignedImmediate => {
                self.lane_i32(f, i, I32BinaryOperator::LessUnsigned, true)?
            }
            Op::WasmI32GreaterSignedImmediate => {
                self.lane_i32(f, i, I32BinaryOperator::GreaterSigned, true)?
            }
            Op::WasmI32GreaterUnsignedImmediate => {
                self.lane_i32(f, i, I32BinaryOperator::GreaterUnsigned, true)?
            }
            Op::WasmI32LessEqualSignedImmediate => {
                self.lane_i32(f, i, I32BinaryOperator::LessEqualSigned, true)?
            }
            Op::WasmI32LessEqualUnsignedImmediate => {
                self.lane_i32(f, i, I32BinaryOperator::LessEqualUnsigned, true)?
            }
            Op::WasmI32GreaterEqualSignedImmediate => {
                self.lane_i32(f, i, I32BinaryOperator::GreaterEqualSigned, true)?
            }
            Op::WasmI32GreaterEqualUnsignedImmediate => {
                self.lane_i32(f, i, I32BinaryOperator::GreaterEqualUnsigned, true)?
            }
            Op::WasmI32Load => self.lane_load(f, i, MemoryLoad::I32Load)?,
            Op::WasmI64Load => self.lane_load(f, i, MemoryLoad::I64Load)?,
            Op::WasmF32Load => self.lane_load(f, i, MemoryLoad::F32Load)?,
            Op::WasmF64Load => self.lane_load(f, i, MemoryLoad::F64Load)?,
            Op::WasmI32Load8S => self.lane_load(f, i, MemoryLoad::I32Load8S)?,
            Op::WasmI32Load8U => self.lane_load(f, i, MemoryLoad::I32Load8U)?,
            Op::WasmI32Load16S => self.lane_load(f, i, MemoryLoad::I32Load16S)?,
            Op::WasmI32Load16U => self.lane_load(f, i, MemoryLoad::I32Load16U)?,
            Op::WasmI64Load8S => self.lane_load(f, i, MemoryLoad::I64Load8S)?,
            Op::WasmI64Load8U => self.lane_load(f, i, MemoryLoad::I64Load8U)?,
            Op::WasmI64Load16S => self.lane_load(f, i, MemoryLoad::I64Load16S)?,
            Op::WasmI64Load16U => self.lane_load(f, i, MemoryLoad::I64Load16U)?,
            Op::WasmI64Load32S => self.lane_load(f, i, MemoryLoad::I64Load32S)?,
            Op::WasmI64Load32U => self.lane_load(f, i, MemoryLoad::I64Load32U)?,
            Op::WasmI32Store => self.lane_store(f, i, MemoryStore::I32Store)?,
            Op::WasmI64Store => self.lane_store(f, i, MemoryStore::I64Store)?,
            Op::WasmF32Store => self.lane_store(f, i, MemoryStore::F32Store)?,
            Op::WasmF64Store => self.lane_store(f, i, MemoryStore::F64Store)?,
            Op::WasmI32Store8 => self.lane_store(f, i, MemoryStore::I32Store8)?,
            Op::WasmI32Store16 => self.lane_store(f, i, MemoryStore::I32Store16)?,
            Op::WasmI64Store8 => self.lane_store(f, i, MemoryStore::I64Store8)?,
            Op::WasmI64Store16 => self.lane_store(f, i, MemoryStore::I64Store16)?,
            Op::WasmI64Store32 => self.lane_store(f, i, MemoryStore::I64Store32)?,
            _ => return None,
        }
        Some(next)
    }

    /// A taken jump stays in the lane unless it is a collection safepoint
    /// with a collection due: every `Jump`, and backward Wasm conditionals.
    #[inline(always)]
    fn lane_jump(&self, target: u32, pc: usize, safepoint: bool) -> Option<usize> {
        let target = target as usize;
        if (safepoint || target <= pc) && self.heap.should_collect() {
            return None;
        }
        Some(target)
    }

    #[inline(always)]
    fn lane_compare_jump<I: LaneFields>(
        &self,
        f: usize,
        i: I,
        pc: usize,
        comparison: I32BinaryOperator,
    ) -> Option<usize> {
        let left = self.read(f, i.a()).wasm_bits32() as i32;
        let right = self.read(f, i.b()).wasm_bits32() as i32;
        if comparison.evaluate(left, right).ok()? != 0 {
            self.lane_jump(i.imm(), pc, false)
        } else {
            Some(pc + 1)
        }
    }

    /// An i32 operator whose result is defined; a trap leaves the lane.
    #[inline(always)]
    fn lane_i32<I: LaneFields>(
        &mut self,
        f: usize,
        i: I,
        operator: I32BinaryOperator,
        immediate: bool,
    ) -> Option<()> {
        let left = self.read(f, i.b()).wasm_bits32() as i32;
        let right = if immediate {
            i.imm() as i32
        } else {
            self.read(f, i.c()).wasm_bits32() as i32
        };
        let value = operator.evaluate(left, right).ok()?;
        self.write(f, i.a(), Value::integer(value));
        Some(())
    }

    /// The unshared backing and memory32 effective address of a direct access.
    #[inline(always)]
    fn lane_memory<I: LaneFields>(&self, f: usize, i: I) -> Option<(&MemoryStorage, u64)> {
        let Some(Cell::WasmMemory { bytes, .. }) = self.heap.get(self.read(f, i.b())) else {
            return None;
        };
        let storage: &MemoryStorage = bytes;
        let address = u64::from(self.read(f, i.c()).wasm_bits32()) + u64::from(i.imm());
        matches!(storage, MemoryStorage::Unshared(_)).then_some((storage, address))
    }

    #[inline(always)]
    fn lane_load<I: LaneFields>(&mut self, f: usize, i: I, load: MemoryLoad) -> Option<()> {
        let value = {
            let (storage, address) = self.lane_memory(f, i)?;
            let MemoryStorage::Unshared(state) = storage else {
                return None;
            };
            load.read(&state.try_borrow().ok()?.bytes, address).ok()?
        };
        let value = self.encode_wasm_value(value);
        self.write(f, i.a(), value);
        Some(())
    }

    #[inline(always)]
    fn lane_store<I: LaneFields>(&mut self, f: usize, i: I, store: MemoryStore) -> Option<()> {
        let value = self
            .decode_wasm_value(self.read(f, i.a()), store.value_type())
            .ok()?;
        let (storage, address) = self.lane_memory(f, i)?;
        let MemoryStorage::Unshared(state) = storage else {
            return None;
        };
        store
            .write(&mut state.try_borrow_mut().ok()?.bytes, address, value)
            .ok()
    }
}
