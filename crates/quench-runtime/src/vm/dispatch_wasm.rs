//! Wasm opcodes of the shared vocabulary. The general step delegates the
//! whole Wasm family here, so JavaScript dispatch keeps its own compact match
//! while both run in the same loop over the same frames.

use super::*;
use crate::bytecode::Class;
use crate::staged::ConcreteWasm;
use crate::staged::machine::MachineStage as _;
use crate::staged::wasm::{Width, integer_binary};
use crate::staged::wasm_float::{convert, float_binary};
use crate::staged::wasm_operation::{Operand, WasmOperation};
use crate::staged::wasm_simd::WasmSimd as _;
use crate::staged::wasm_simd_concrete::{ConcreteSimd, execute};
use crate::staged::wasm_instance::Instance;
use crate::staged::wasm_machine::ConcreteMemory;
use crate::staged::wasm_memory::fill as memory_fill_rule;
use crate::staged::wasm_step::Step;
use crate::staged::wasm_table_ops::TableOp;
use crate::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};

mod gc;
pub(super) use gc::{wasm_gc_allocates, wasm_gc_field_op, wasm_gc_reads_instance, wasm_gc_value_op, wasm_gc_value_ops};

impl<H: Host> Vm<H> {
    #[inline(never)]
    pub(super) fn step_wasm(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        i: WideInstruction,
        pc: &mut usize,
    ) -> Result<StepResult, JsError> {
        if let Some(Step::Numeric(operation)) = Step::of(i.op(), i.imm()) {
            self.wasm_numeric_step(p, f, i, pc, operation)?;
            return Ok(StepResult::Continue);
        }
        match i.op() {
            Op::WasmInstanceBinding | Op::WasmGlobalGet | Op::WasmGlobalSet => {
                self.wasm_instance_step(f, i)?;
            }
            op if wasm_gc_value_op(op) => {
                if let Some(value) = self.wasm_gc_general_step(self.frames[f].program, self.frames[f].env, i, |vm, register| vm.read(f, register))? {
                    self.write(f, i.result_register(), value);
                }
            }
            op if TableOp::of(op).is_some() => self.wasm_table_step(f, i)?,
            Op::WasmMemoryCopy | Op::WasmMemoryFill => {
                let destination = self.read(f, i.register_a());
                let source = self.read(f, i.register_b());
                let (output, input) = i.register_pair();
                let output = self.wasm_memory_index(destination, self.read(f, output))?;
                let index_type = self.wasm_memory_index_type(destination)?;
                let length_type = match i.op() {
                    Op::WasmMemoryCopy => {
                        match (index_type, self.wasm_memory_index_type(source)?) {
                            (crate::WasmType::I64, crate::WasmType::I64) => crate::WasmType::I64,
                            _ => crate::WasmType::I32,
                        }
                    }
                    _ => index_type,
                };
                let length = self.wasm_index_operand(self.read(f, i.register_c()), length_type)?;
                if i.op() == Op::WasmMemoryFill {
                    let byte = u64::from(Self::wasm_u32_operand(source)?);
                    self.wasm_memory_bulk(destination, |stage| {
                        memory_fill_rule(stage, output, byte, length)
                    })?;
                } else {
                    let input = self.wasm_memory_index(source, self.read(f, input))?;
                    self.wasm_copy_bytes(destination, Some(source), output, input, length)?;
                }
            }
            Op::WasmMemoryAddress => {
                let memory = self.read(f, i.register_c());
                let address = self.wasm_memory_index(memory, self.read(f, i.register_b()))?;
                let Constant::WasmBits64(offset) = p.constants[i.constant_index()] else {
                    return Err(JsError::validation(
                        "invalid Wasm memory offset constant".into(),
                    ));
                };
                let effective = address
                    .checked_add(offset)
                    .ok_or_else(|| JsError::wasm_trap_error(crate::WasmTrap::OutOfBoundsMemory))?;
                let value = self.encode_wasm_value(crate::WasmValue::I64(effective as i64));
                self.write(f, i.result_register(), value);
            }
            Op::WasmAtomicFence => crate::wasm::atomic::fence(),
            Op::WasmAtomicAccess => {
                use crate::wasm::atomic::{AtomicInput, AtomicOperator};
                let operator =
                    AtomicOperator::from_tag(i.imm()).expect("validated atomic operator");
                let window = i.register_window();
                let input = |role: AtomicInput| self.read(f, window.base + role as u16);
                let memory = input(AtomicInput::Memory);
                let address = self.wasm_i64_operand(input(AtomicInput::Address))? as u64;
                let value = if window.count > AtomicInput::Value as u16 {
                    Some(self.decode_wasm_value(input(AtomicInput::Value), operator.value_type())?)
                } else {
                    None
                };
                let replacement = if window.count > AtomicInput::Auxiliary as u16 {
                    Some(self.decode_wasm_value(
                        input(AtomicInput::Auxiliary),
                        operator.auxiliary_type(),
                    )?)
                } else {
                    None
                };
                let Some(Cell::WasmMemory { bytes, .. }) = self.heap.get(memory) else {
                    return Err(JsError::validation("invalid atomic memory binding".into()));
                };
                let result = operator
                    .apply(bytes, address, value, replacement)
                    .map_err(JsError::wasm_trap_error)?;
                let result = result
                    .map(|value| self.encode_wasm_value(value))
                    .unwrap_or(Value::UNDEFINED);
                self.write(f, i.result_register(), result);
            }
            Op::WasmI32Load
            | Op::WasmI64Load
            | Op::WasmF32Load
            | Op::WasmF64Load
            | Op::WasmI32Load8S
            | Op::WasmI32Load8U
            | Op::WasmI32Load16S
            | Op::WasmI32Load16U
            | Op::WasmI64Load8S
            | Op::WasmI64Load8U
            | Op::WasmI64Load16S
            | Op::WasmI64Load16U
            | Op::WasmI64Load32S
            | Op::WasmI64Load32U => self.wasm_direct_load(f, i)?,
            Op::WasmI32Store
            | Op::WasmI64Store
            | Op::WasmF32Store
            | Op::WasmF64Store
            | Op::WasmI32Store8
            | Op::WasmI32Store16
            | Op::WasmI64Store8
            | Op::WasmI64Store16
            | Op::WasmI64Store32 => self.wasm_direct_store(f, i)?,
            Op::WasmMemoryLoad | Op::WasmMemoryStore => {
                let memory = self.read(f, i.register_b());
                let address = self.wasm_i64_operand(self.read(f, i.register_c()))? as u64;
                if i.op() == Op::WasmMemoryLoad {
                    let operator = crate::wasm::memory::MemoryLoad::from_tag(i.imm())
                        .expect("validated memory load");
                    let value = operator
                        .read(&self.wasm_memory_bytes(memory)?, address)
                        .map_err(JsError::wasm_trap_error)?;
                    let value = self.encode_wasm_value(value);
                    self.write(f, i.result_register(), value);
                } else {
                    let operator = crate::wasm::memory::MemoryStore::from_tag(i.imm())
                        .expect("validated memory store");
                    self.wasm_memory_store(
                        memory,
                        address,
                        operator,
                        self.read(f, i.register_a()),
                    )?;
                }
            }
            Op::WasmMemorySize | Op::WasmMemoryGrow => {
                let memory = self.read(f, i.register_b());
                let pages = if i.op() == Op::WasmMemoryGrow {
                    let delta = self.wasm_memory_index(memory, self.read(f, i.register_c()))?;
                    self.wasm_memory_grow(memory, delta)?
                } else {
                    Some(self.wasm_memory_pages(memory)?)
                };
                let index_type = self.wasm_memory_index_type(memory)?;
                let value = self.encode_wasm_index_size(index_type, pages);
                self.write(f, i.result_register(), value);
            }
            Op::WasmSimd => {
                let (kernel, _) =
                    crate::wasm::simd::SimdKernel::of(i.imm()).expect("validated SIMD operator");
                let left = self.decode_wasm_value(self.read(f, i.register_b()), kernel.left)?;
                let right = kernel
                    .right
                    .map(|ty| self.decode_wasm_value(self.read(f, i.register_c()), ty))
                    .transpose()?;
                let value = self.encode_wasm_value(execute(i.imm(), left, right));
                self.write(f, i.result_register(), value);
            }
            Op::WasmSimdShuffle => {
                let crate::WasmValue::V128(left) =
                    self.decode_wasm_value(self.read(f, i.register_b()), crate::WasmType::V128)?
                else {
                    unreachable!()
                };
                let crate::WasmValue::V128(right) =
                    self.decode_wasm_value(self.read(f, i.register_c()), crate::WasmType::V128)?
                else {
                    unreachable!()
                };
                let Constant::WasmV128(indices) = p.constants[i.constant_index()] else {
                    unreachable!("validated shuffle indices")
                };
                let bits = ConcreteSimd.vector_shuffle(indices, left, right);
                let value = self.encode_wasm_value(crate::WasmValue::V128(bits));
                self.write(f, i.result_register(), value);
            }
            Op::WasmUnreachable => {
                return Err(JsError::wasm_trap_error(crate::WasmTrap::Unreachable));
            }
            Op::WasmFillRegisters => {
                let value = self
                    .programs
                    .constant(self.frames[f].program, i.constant_index())
                    .ok_or_else(|| {
                        JsError::validation("constant index is outside program".into())
                    })?;
                // The registers the staged fill names (`Instance::Fill`).
                for register in Instance::Fill.constant_registers(i).into_iter().flatten() {
                    self.write(f, register, value);
                }
            }
            Op::WasmSelect => {
                // `MachineStage::select`, the primitive the Wasm flow stage selects with.
                let (condition, _) = i.register_pair();
                let flag = u64::from(self.read(f, condition).wasm_bits32());
                let mut stage = ConcreteWasm;
                let chosen = stage.select(
                    flag,
                    self.read(f, i.register_b()).to_bits(),
                    self.read(f, i.register_c()).to_bits(),
                );
                self.write(f, i.result_register(), Value::from_bits(chosen));
            }
            Op::WasmBranchTable => *pc += self.branch_table_case(f, i),
            Op::WasmReturn64 => {
                let word = self.read(f, i.register_a()).to_bits();
                return Ok(StepResult::Return(self.encode_bits64(word)));
            }
            Op::WasmExceptionNew => {
                use crate::wasm::tag::ExceptionInput;
                let window = i.register_window();
                if window.count < ExceptionInput::MIN_COUNT {
                    return Err(JsError::validation("missing Wasm exception tag".into()));
                }
                let tag = self.read(f, window.base + ExceptionInput::Tag as u16);
                let payload = (ExceptionInput::Payload as u16..window.count)
                    .map(|offset| self.read(f, window.base + offset))
                    .collect();
                let value = self.wasm_exception_new(tag, payload)?;
                self.write(f, i.result_register(), value);
            }
            Op::WasmExceptionMatch => {
                let matches = self.wasm_exception_matches(
                    self.read(f, i.register_b()),
                    self.read(f, i.register_c()),
                )?;
                self.write(f, i.result_register(), Value::integer(i32::from(matches)));
            }
            Op::WasmExceptionPayload => {
                let value = self.wasm_exception_payload(self.read(f, i.register_b()), i.imm())?;
                self.write(f, i.result_register(), value);
            }
            Op::WasmThrowRef => return Err(self.wasm_throw_ref(self.read(f, i.register_a()))),
            op => unreachable!("{op:?} is not a Wasm opcode"),
        }
        Ok(StepResult::Continue)
    }
}

impl<H: Host> Vm<H> {
    /// The word a register value of type `ty` gives a staged Wasm stage: a raw register holds
    /// it, a boxed slot decodes to it. An f64 operator that reads a NaN only as a NaN takes the
    /// slot's bits as they are (`float_bits64`).
    fn wasm_word(
        &self,
        value: Value,
        ty: crate::WasmType,
        raw: bool,
        nan_agnostic: bool,
    ) -> Result<u64, JsError> {
        if raw {
            return Ok(value.to_bits());
        }
        Ok(match ty {
            crate::WasmType::I32 => match value.as_int() {
                Some(bits) => u64::from(bits as u32),
                None => {
                    let crate::WasmValue::I32(bits) = self.decode_wasm_value(value, ty)? else {
                        unreachable!("decoded i32 operand")
                    };
                    u64::from(bits as u32)
                }
            },
            crate::WasmType::I64 => self.wasm_i64_operand(value)? as u64,
            crate::WasmType::F32 => u64::from(self.wasm_f32_operand(value)?.to_bits()),
            crate::WasmType::F64 if nan_agnostic => value.float_bits64(),
            crate::WasmType::F64 => self.wasm_f64_operand(value)?.to_bits(),
            _ => unreachable!("numeric Wasm operand type"),
        })
    }

    /// The register value of a stage word of type `ty`: raw registers hold it as is, boxed
    /// slots encode it.
    fn encode_wasm_word(&mut self, raw: bool, ty: crate::WasmType, word: u64) -> Value {
        let value = crate::WasmValue::from_bits(ty, word);
        if raw {
            self.encode_raw_wasm_value(value)
        } else {
            self.encode_wasm_value(value)
        }
    }

    /// Runs a staged bulk rule over the bytes of the linear memory `memory`, on the interpreter's
    /// `ConcreteMemory` stage. The rule's trap leaves as the Wasm trap it names.
    fn wasm_memory_bulk(
        &self,
        memory: Value,
        rule: impl FnOnce(&mut ConcreteMemory<'_>) -> Result<(), crate::WasmTrap>,
    ) -> Result<(), JsError> {
        let Some(Cell::WasmMemory { bytes, .. }) = self.heap.get(memory) else {
            return Err(JsError::validation("invalid mutable Wasm memory binding".into()));
        };
        let mut guard = bytes.lock();
        rule(&mut ConcreteMemory { memory: &mut guard }).map_err(JsError::wasm_trap_error)
    }

    /// One Wasm numeric step (`WasmOperation`) on the interpreter's stage: the staged rules
    /// the Wasm tier lowers (`vm/compiled/wasm_stage.rs`), run over `ConcreteWasm`. Operands are
    /// read, traps leave, and the result is written or jumped on, in that order.
    fn wasm_numeric_step(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        i: WideInstruction,
        pc: &mut usize,
        operation: WasmOperation,
    ) -> Result<(), JsError> {
        let raw = matches!(
            i.op().class(),
            Class::WasmI64BinaryRaw
                | Class::WasmI64BinaryImmediateRaw
                | Class::WasmI64UnaryRaw
                | Class::WasmFloatBinary64Raw
                | Class::WasmFloatUnary64Raw
        );
        let nan_agnostic = match operation {
            WasmOperation::FloatBinary { operator, .. } => operator.nan_agnostic(),
            WasmOperation::FloatUnary { operator, .. } => operator.nan_agnostic(),
            _ => false,
        };
        let mut stage = ConcreteWasm;
        let (left_register, _) = operation.reads(i);
        let left = self.wasm_word(
            self.read(f, left_register),
            operation.operand_type(),
            raw,
            nan_agnostic,
        )?;
        let value = match operation {
            WasmOperation::FloatBinary { operator, float } => {
                let right = self.wasm_word(
                    self.read(f, i.register_c()),
                    float.ty(),
                    raw,
                    nan_agnostic,
                )?;
                float_binary(&mut stage, operator, float, left, right)
            }
            WasmOperation::FloatUnary { operator, float } => {
                stage.operate_float_unary(operator, float, left)
            }
            WasmOperation::Convert { operator } => {
                convert(&mut stage, operator, left).map_err(JsError::wasm_trap_error)?
            }
            WasmOperation::Binary {
                operator,
                width,
                form,
            } => {
                let right = match form.right(i) {
                    Operand::Constant(value) => stage.constant(width, value),
                    Operand::Register(register) => {
                        self.wasm_word(self.read(f, register), width.ty(), raw, false)?
                    }
                };
                integer_binary(&mut stage, operator, width, left, right)
                    .map_err(JsError::wasm_trap_error)?
            }
            WasmOperation::Unary { operator, width } => stage.operate_unary(operator, width, left),
            WasmOperation::BitField => {
                let shift = stage.constant(Width::I32, i64::from(i.c() as i16));
                let shifted = stage.operate(Binary::ShiftRightUnsigned, Width::I32, left, shift);
                let mask = stage.constant(Width::I32, i64::from(i.imm() as i32));
                stage.operate(Binary::And, Width::I32, shifted, mask)
            }
            WasmOperation::Test { nonzero: true } => left,
            WasmOperation::Test { nonzero: false } => {
                stage.operate_unary(Unary::EqualZero, Width::I32, left)
            }
            WasmOperation::AddBranch => {
                let addend = stage.constant(Width::I32, i64::from(i.c() as i16));
                stage.operate(Binary::Add, Width::I32, left, addend)
            }
        };
        if operation.writes() {
            let result = self.encode_wasm_word(raw, operation.result_type(), value);
            self.write(f, i.result_register(), result);
        }
        if operation.jumps() {
            self.wasm_jump(p, f, i, pc, value != 0);
        }
        Ok(())
    }
}
