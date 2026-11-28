//! Wasm opcodes of the shared vocabulary. The general step delegates the
//! whole Wasm family here, so JavaScript dispatch keeps its own compact match
//! while both run in the same loop over the same frames. The family is
//! dispatched on its catalog step (`Step::of`): one arm per step kind, each
//! group's arms its enum's variants. The GC group's sub-kinds (`gc.rs`) and the
//! atomics, which have no step group, still match opcodes.

use super::*;
use crate::staged::ConcreteWasm;
use crate::staged::machine::MachineStage as _;
use crate::staged::wasm::{Width, integer_binary};
use crate::staged::wasm_atomic::Atomic;
use crate::staged::wasm_call::Call;
use crate::staged::wasm_exception::Exception;
use crate::staged::wasm_flow::Flow;
use crate::staged::wasm_float::{convert, float_binary};
use crate::staged::wasm_instance::Instance;
use crate::staged::wasm_memory_bytes::write_memory;
use crate::staged::wasm_machine::ConcreteMemory;
use crate::staged::wasm_memory::{Memory, fill as memory_fill_rule, pages as memory_pages_rule, scalar_address};
use crate::staged::wasm_operation::{Operand, WasmOperation};
use crate::staged::wasm_simd::{Simd, WasmSimd as _};
use crate::staged::wasm_simd_concrete::{ConcreteSimd, execute};
use crate::staged::wasm_step::{Family, Step};
use crate::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};
use super::wasm_table::copy_length_type;

mod gc;
pub use gc::{wasm_gc_allocates, wasm_gc_field_op, wasm_gc_reads_instance};

impl Vm {
    /// One Wasm instruction, dispatched on its catalog step (`Step::of`).
    #[inline(never)]
    pub(super) fn step_wasm(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        i: WideInstruction,
        pc: &mut usize,
    ) -> Result<StepResult, JsError> {
        match Step::of(i.op(), i.imm()) {
            Some(Step::Numeric(operation)) => self.wasm_numeric_step(p, f, i, pc, operation)?,
            Some(Step::Instance(Instance::Fill)) => self.wasm_fill_registers(f, i)?,
            Some(Step::Instance(Instance::Move | Instance::Constant)) => {
                unreachable!("the general loop makes moves and constants")
            }
            Some(Step::Instance(step)) => self.wasm_instance_step(f, i, step)?,
            Some(Step::Memory(step)) => self.wasm_memory_step(f, i, step)?,
            Some(Step::Flow(step)) => return self.wasm_flow_step(f, i, pc, step),
            Some(Step::Table(_)) => self.wasm_table_step(f, i)?,
            Some(Step::Simd(step)) => self.wasm_simd_step(p, f, i, step)?,
            Some(Step::Gc(_) | Step::Call(Call::IndirectTarget | Call::RefFunc)) => self.wasm_gc_arm(f, i)?,
            Some(Step::Call(Call::Known | Call::Reference)) => unreachable!("the general loop makes calls"),
            Some(Step::Exception(step)) => self.wasm_exception_step(f, i, step)?,
            Some(Step::Atomic(Atomic::Fence)) => crate::wasm::atomic::fence(),
            Some(Step::Atomic(Atomic::Access)) => self.wasm_atomic_access(f, i)?,
            None => self.wasm_unlisted_step(f, i)?,
        }
        Ok(StepResult::Continue)
    }

    /// `Instance::Fill`: every register of the window takes the constant the instruction names.
    fn wasm_fill_registers(&mut self, f: usize, i: WideInstruction) -> Result<(), JsError> {
        let value = self
            .programs
            .constant(self.frames[f].program, i.constant_index())
            .ok_or_else(|| JsError::validation("constant index is outside program".into()))?;
        // The registers the staged fill names (`Instance::Fill`).
        for register in Instance::Fill.constant_registers(i).into_iter().flatten() {
            self.write(f, register, value);
        }
        Ok(())
    }

    /// A linear-memory step (`Memory`): the access's width and sign, the size and grow, and the
    /// bulk fill and copy.
    fn wasm_memory_step(&mut self, f: usize, i: WideInstruction, step: Memory) -> Result<(), JsError> {
        match step {
            Memory::Load(access) => self.wasm_direct_load(f, i, access),
            Memory::Store(access) => self.wasm_direct_store(f, i, access),
            Memory::Size | Memory::Grow => self.wasm_memory_pages_step(f, i, step),
            Memory::Fill | Memory::Copy => self.wasm_memory_bulk_step(f, i, step),
        }
    }

    /// `memory.size` and `memory.grow`: the size in pages, before a growth where the memory grows.
    fn wasm_memory_pages_step(&mut self, f: usize, i: WideInstruction, step: Memory) -> Result<(), JsError> {
        let memory = self.read(f, i.register_b());
        let index_type = self.wasm_memory_index_type(memory)?;
        let pages = match step {
            Memory::Grow => {
                let delta = self.wasm_index_operand(self.read(f, i.register_c()), index_type)?;
                self.wasm_memory_grow(memory, delta)?
            }
            _ => {
                let page_bits = self.wasm_memory_type(memory)?.page_size_log2();
                Some(self.wasm_memory_read(memory, |stage| memory_pages_rule(stage, page_bits))?)
            }
        };
        let value = self.encode_wasm_index_size(index_type, pages);
        self.write(f, i.result_register(), value);
        Ok(())
    }

    /// `memory.fill` and `memory.copy`: the length's type follows the memories' index types, and
    /// the range checks run before any byte moves.
    fn wasm_memory_bulk_step(&mut self, f: usize, i: WideInstruction, step: Memory) -> Result<(), JsError> {
        let destination = self.read(f, i.register_a());
        let source = self.read(f, i.register_b());
        let (output, input) = i.register_pair();
        let index_type = self.wasm_memory_index_type(destination)?;
        let output = self.wasm_index_operand(self.read(f, output), index_type)?;
        match step {
            Memory::Fill => {
                let length = self.wasm_index_operand(self.read(f, i.register_c()), index_type)?;
                let byte = u64::from(Self::wasm_u32_operand(source)?);
                self.wasm_memory_bulk(destination, |stage| memory_fill_rule(stage, output, byte, length))
            }
            _ => {
                // A copy's source is a memory too: its index type is derived once, for both the
                // length and the input offset.
                let source_type = self.wasm_memory_index_type(source)?;
                let length_type = copy_length_type(index_type, source_type);
                let length = self.wasm_index_operand(self.read(f, i.register_c()), length_type)?;
                let input = self.wasm_index_operand(self.read(f, input), source_type)?;
                self.wasm_copy_bytes(destination, Some(source), output, input, length)
            }
        }
    }

    /// A tagged memory load: the memory in `b` at the 64-bit address `c`, read by the operator
    /// `imm` names (a vector load, or a lane's scalar one).
    fn wasm_tagged_memory_load(&mut self, f: usize, i: WideInstruction) -> Result<(), JsError> {
        let memory = self.read(f, i.register_b());
        let address = self.wasm_i64_operand(self.read(f, i.register_c()))? as u64;
        let operator = crate::wasm::memory::MemoryLoad::from_tag(i.imm()).expect("validated memory load");
        let value = operator
            .read(&self.wasm_memory_bytes(memory)?, address)
            .map_err(JsError::wasm_trap_error)?;
        let value = self.encode_wasm_value(value);
        self.write(f, i.result_register(), value);
        Ok(())
    }

    /// A tagged memory store: the value in `a` to the memory in `b` at the 64-bit address `c`,
    /// written by the operator `imm` names (a vector store, or a lane's scalar one).
    fn wasm_tagged_memory_store(&mut self, f: usize, i: WideInstruction) -> Result<(), JsError> {
        let memory = self.read(f, i.register_b());
        let address = self.wasm_i64_operand(self.read(f, i.register_c()))? as u64;
        let operator = crate::wasm::memory::MemoryStore::from_tag(i.imm()).expect("validated memory store");
        let value = self.decode_wasm_value(self.read(f, i.register_a()), operator.value_type())?;
        let mut bytes = self.wasm_memory_mut(memory)?;
        write_memory(&mut bytes, operator, address, value).map_err(JsError::wasm_trap_error)
    }

    /// The control steps (`Flow`): a select, a branch table, a trap and a return. A jump is
    /// the general loop's.
    fn wasm_flow_step(
        &mut self,
        f: usize,
        i: WideInstruction,
        pc: &mut usize,
        step: Flow,
    ) -> Result<StepResult, JsError> {
        match step {
            Flow::Select => {
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
            Flow::Table => *pc += self.branch_table_case(f, i),
            Flow::Unreachable => return Err(JsError::wasm_trap_error(crate::WasmTrap::Unreachable)),
            Flow::Return => {
                let word = self.read(f, i.register_a()).to_bits();
                return Ok(StepResult::Return(self.encode_bits64(word)));
            }
            Flow::Jump => unreachable!("the general loop takes jumps"),
        }
        Ok(StepResult::Continue)
    }

    /// The vector steps (`Simd`): an operator on lanes, a shuffle, the effective address of a
    /// vector access, and its load and store.
    fn wasm_simd_step(&mut self, p: &ResidualProgram, f: usize, i: WideInstruction, step: Simd) -> Result<(), JsError> {
        match step {
            Simd::Operator { selector } => {
                let (kernel, _) = crate::wasm::simd::SimdKernel::of(selector).expect("validated SIMD operator");
                let left = self.decode_wasm_value(self.read(f, i.register_b()), kernel.left)?;
                let right = kernel
                    .right
                    .map(|ty| self.decode_wasm_value(self.read(f, i.register_c()), ty))
                    .transpose()?;
                let value = self.encode_wasm_value(execute(kernel, selector, left, right));
                self.write(f, i.result_register(), value);
            }
            Simd::Shuffle => {
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
            Simd::Address => self.wasm_address_step(p, f, i)?,
            Simd::Load => self.wasm_tagged_memory_load(f, i)?,
            Simd::Store => self.wasm_tagged_memory_store(f, i)?,
        }
        Ok(())
    }

    /// The effective address of an access: the 32-bit address in `b` plus the constant's static
    /// offset, as 64 bits; a sum that wraps traps. The memory is in `c`.
    fn wasm_address_step(&mut self, p: &ResidualProgram, f: usize, i: WideInstruction) -> Result<(), JsError> {
        let memory = self.read(f, i.register_c());
        let index = self.wasm_memory_index(memory, self.read(f, i.register_b()))?;
        let Constant::WasmBits64(offset) = p.constants[i.constant_index()] else {
            return Err(JsError::validation("invalid Wasm memory offset constant".into()));
        };
        let effective = scalar_address(&mut ConcreteWasm, index, offset).map_err(JsError::wasm_trap_error)?;
        let value = self.encode_wasm_value(crate::WasmValue::I64(effective as i64));
        self.write(f, i.result_register(), value);
        Ok(())
    }

    /// The exception steps (`Exception`): building one, a catch's tag test and payload read, and
    /// `throw_ref`.
    fn wasm_exception_step(&mut self, f: usize, i: WideInstruction, step: Exception) -> Result<(), JsError> {
        match step {
            Exception::New => {
                let value = self.wasm_exception_from_window(i, |vm, register| vm.read(f, register))?;
                self.write(f, i.result_register(), value);
            }
            Exception::Match => {
                let (exception, expected) = (self.read(f, i.register_b()), self.read(f, i.register_c()));
                let matches = self.wasm_exception_matches(exception, expected)?;
                self.write(f, i.result_register(), Value::integer(i32::from(matches)));
            }
            Exception::Payload => {
                let exception = self.read(f, i.register_b());
                let value = self.wasm_exception_payload(exception, i.imm())?;
                self.write(f, i.result_register(), value);
            }
            Exception::ThrowRef => {
                let exception = self.read(f, i.register_a());
                return Err(self.wasm_throw_ref(exception));
            }
        }
        Ok(())
    }

    /// A GC value step, or a call target (`ref.func`, an indirect target), which writes its
    /// result register when it makes a value.
    fn wasm_gc_arm(&mut self, f: usize, i: WideInstruction) -> Result<(), JsError> {
        if let Some(value) = self.wasm_gc_general_step(
            self.frames[f].program,
            self.frames[f].env,
            i,
            |vm, register| vm.read(f, register),
        )? {
            self.write(f, i.result_register(), value);
        }
        Ok(())
    }

    /// The steps no group names: a scalar lane's memory access, whose tag selects the width (a
    /// vector access is `Step::Simd`).
    fn wasm_unlisted_step(&mut self, f: usize, i: WideInstruction) -> Result<(), JsError> {
        match Family::of(i.op()) {
            Some(Family::MemoryLoad) => self.wasm_tagged_memory_load(f, i),
            Some(Family::MemoryStore) => self.wasm_tagged_memory_store(f, i),
            _ => unreachable!("{:?} is not a Wasm opcode", i.op()),
        }
    }

    /// An atomic read-modify-write, compare-exchange or wait on the memory the instruction names.
    fn wasm_atomic_access(&mut self, f: usize, i: WideInstruction) -> Result<(), JsError> {
        let result = self.wasm_atomic_result(i, |vm, register| vm.read(f, register))?;
        self.write(f, i.result_register(), result);
        Ok(())
    }

    /// The value of the atomic access `i` names, its operands read with `read`: what the operator
    /// leaves, or `UNDEFINED` for an operator that leaves none.
    pub(super) fn wasm_atomic_result(
        &mut self,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value,
    ) -> Result<Value, JsError> {
        use crate::wasm::atomic::{AtomicInput, AtomicOperator};
        let operator = AtomicOperator::from_tag(i.imm()).expect("validated atomic operator");
        let window = i.register_window();
        let input = |role: AtomicInput| read(&*self, window.base + role as u16);
        let memory = input(AtomicInput::Memory);
        let address = self.wasm_i64_operand(input(AtomicInput::Address))? as u64;
        let value = if window.count > AtomicInput::Value as u16 {
            Some(self.decode_wasm_value(input(AtomicInput::Value), operator.value_type())?)
        } else {
            None
        };
        let replacement = if window.count > AtomicInput::Auxiliary as u16 {
            Some(self.decode_wasm_value(input(AtomicInput::Auxiliary), operator.auxiliary_type())?)
        } else {
            None
        };
        let Some(Cell::WasmMemory { bytes, .. }) = self.heap.get(memory) else {
            return Err(JsError::validation("invalid atomic memory binding".into()));
        };
        let result = operator
            .apply(bytes, address, value, replacement)
            .map_err(JsError::wasm_trap_error)?;
        Ok(result.map(|value| self.encode_wasm_value(value)).unwrap_or(Value::UNDEFINED))
    }

    /// The exception the window of `i` builds, its tag and payload read with `read`.
    pub(super) fn wasm_exception_from_window(
        &mut self,
        i: WideInstruction,
        read: impl Fn(&Self, u16) -> Value,
    ) -> Result<Value, JsError> {
        use crate::wasm::tag::ExceptionInput;
        let window = i.register_window();
        if window.count < ExceptionInput::MIN_COUNT {
            return Err(JsError::validation("missing Wasm exception tag".into()));
        }
        let tag = read(&*self, window.base + ExceptionInput::Tag as u16);
        let payload: Vec<Value> = (ExceptionInput::Payload as u16..window.count)
            .map(|offset| read(&*self, window.base + offset))
            .collect();
        self.wasm_exception_new(tag, payload)
    }
}

impl Vm {
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
                    let crate::WasmValue::I32(bits) = self.decode_wasm_value_slow(value, ty, None)? else {
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
    pub(super) fn wasm_memory_bulk(
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
        let raw = WasmOperation::is_raw(i.op());
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
