//! Wasm operator lowering into the shared residual vocabulary. Binary decoding
//! and module validation belong to quench-wasm, not this execution core.

use crate::bytecode::{
    AtomTable, Constant, DispatchClass, Function, ImmediateLayout, Instr, Op, ProgramKind,
    Register, ResidualProgram, WideInstruction,
};
use crate::{Diagnostic, Engine};
use wasmparser::{BinaryReaderError, Operator};

mod control;
mod scalar;
pub(crate) use scalar::ScalarBits;
pub use scalar::{WasmFunctionBody, WasmSignature, WasmType, WasmValue};
pub(crate) mod integer;
use control::{Control, Reachability};
use integer::{
    I32BinaryOperator, I32UnaryOperator, I64BinaryOperator, I64UnaryOperator,
    IntegerConversionOperator,
};

/// A WebAssembly trap, distinct from a JavaScript throw or invalid residual.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmTrap {
    Unreachable,
    CallStackExhausted,
    IntegerDivideByZero,
    IntegerOverflow,
}

impl std::fmt::Display for WasmTrap {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        output.write_str(match self {
            Self::Unreachable => "unreachable",
            Self::CallStackExhausted => "call stack exhausted",
            Self::IntegerDivideByZero => "integer divide by zero",
            Self::IntegerOverflow => "integer overflow",
        })
    }
}

const ZERO_LOCAL_CONSTANT: u32 = 0;
const VOID_RESULT_CONSTANT: u32 = 1;

/// A shared residual program with one typed module function entry.
pub struct WasmFunction {
    pub(crate) program: ResidualProgram,
    pub(crate) signature: WasmSignature,
    pub(crate) entry: u32,
}

/// Compatibility name for the i32-only lowering and execution boundaries.
pub type WasmI32Function = WasmFunction;

impl WasmFunction {
    pub fn residual(&self) -> &ResidualProgram {
        &self.program
    }
    pub fn signature(&self) -> &WasmSignature {
        &self.signature
    }
}

impl Engine {
    /// Lower a decoded i32 function after the frontend validates its module.
    /// `locals` counts non-parameter i32 locals. No binary parser or second
    /// instruction representation is introduced at this boundary.
    pub fn lower_wasm_i32_function<'a>(
        name: &str,
        params: u16,
        locals: u16,
        has_result: bool,
        operators: impl IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>>,
    ) -> Result<WasmI32Function, Diagnostic> {
        Self::lower_wasm_i32_module(name, 0, [(params, locals, has_result, operators)])
    }

    /// Lower validated i32 function bodies into one shared residual program.
    /// Function indices and signatures retain the frontend's module order.
    pub fn lower_wasm_i32_module<'a, I>(
        name: &str,
        entry: u32,
        bodies: impl IntoIterator<Item = (u16, u16, bool, I)>,
    ) -> Result<WasmI32Function, Diagnostic>
    where
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>>,
    {
        Self::lower_wasm_module(
            name,
            entry,
            bodies
                .into_iter()
                .map(|(params, locals, has_result, operators)| WasmFunctionBody {
                    signature: WasmSignature {
                        params: vec![WasmType::I32; usize::from(params)],
                        result: has_result.then_some(WasmType::I32),
                    },
                    locals: vec![WasmType::I32; usize::from(locals)],
                    operators,
                }),
        )
    }

    /// Lower the frontend's validated scalar signatures and operator streams.
    pub fn lower_wasm_module<'a, I>(
        name: &str,
        entry: u32,
        bodies: impl IntoIterator<Item = WasmFunctionBody<I>>,
    ) -> Result<WasmFunction, Diagnostic>
    where
        I: IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>>,
    {
        let (signatures, bodies): (Vec<_>, Vec<_>) = bodies
            .into_iter()
            .map(|body| (body.signature, (body.locals, body.operators)))
            .unzip();
        if entry as usize >= signatures.len() {
            return Err(Diagnostic::unsupported(
                name,
                "Wasm entry function out of bounds",
            ));
        }
        let mut constants = vec![Constant::Number(0.0), Constant::Undefined];
        let mut functions = Vec::with_capacity(bodies.len());
        for (signature, (locals, operators)) in signatures.iter().zip(bodies) {
            let params = u16::try_from(signature.params.len())
                .map_err(|_| Diagnostic::unsupported(name, "too many Wasm parameters"))?;
            let has_result = signature.result.is_some();
            let local_types: Vec<_> = signature.params.iter().copied().chain(locals).collect();
            let error = |message: &str| Diagnostic::unsupported(name, message);
            let local_count =
                u16::try_from(local_types.len()).map_err(|_| error("too many Wasm locals"))?;
            let mut lowering = Lowering {
                name,
                locals: local_count,
                code: Vec::new(),
                wide: Vec::new(),
                constants,
                signatures: &signatures,
                depth: 0,
                registers: 1,
                controls: vec![Control::function(has_result)],
                path: Reachability::Live,
            };
            // Shared JS frames initialize non-parameter locals to undefined. Wasm
            // initialization is therefore explicit residual code, not another frame.
            for (slot, ty) in local_types.iter().enumerate().skip(usize::from(params)) {
                lowering.load_zero(0, *ty)?;
                lowering.emit(Op::StoreLocal, 0, 0, 0, slot as u32)?;
            }
            for operator in operators {
                let operator =
                    operator.map_err(|e| Diagnostic::unsupported(name, e.to_string()))?;
                if lowering.controls.is_empty() {
                    return Err(error("operators after Wasm function end"));
                }
                lowering.operator(operator)?;
            }
            if !lowering.controls.is_empty() {
                return Err(error("missing Wasm function end"));
            }
            let function = Function {
                parent: None,
                name: None,
                source_text: None,
                params,
                length: params,
                parameter_end_pc: 0,
                parameter_atoms: vec![],
                rest: false,
                is_async: false,
                is_generator: false,
                is_class_constructor: false,
                derived_constructor: false,
                super_home_atom: None,
                constructible: false,
                class_field_initializer: false,
                parameter_eval_arguments_error: false,
                arguments_slot: None,
                strict: true,
                locals: local_count,
                local_atoms: vec![],
                environment_atoms: vec![],
                lexical_atoms: vec![],
                global_lexical_atoms: vec![],
                global_var_atoms: vec![],
                global_function_atoms: vec![],
                global_annex_b_var_atoms: vec![],
                global_immutable_atoms: vec![],
                eval_sites: vec![],
                code: lowering.code,
                wide: lowering.wide,
                registers: lowering.registers,
                dispatch: DispatchClass::General,
                handlers: vec![],
                register_root_offset: crate::bytecode::NO_REGISTER_ROOT_MAP,
            };
            functions.push(function);
            constants = lowering.constants;
        }
        let register_roots = crate::compile::liveness::derive(&mut functions, &[], &[], &[]);
        let program = ResidualProgram {
            specialized: false,
            kind: ProgramKind::Wasm,
            module_requests: vec![],
            module_imports: vec![],
            module_link_plan: None,
            source_name: name.into(),
            atoms: AtomTable::default(),
            constants,
            functions,
            cache_sites: 0,
            method_sites: vec![],
            method_arguments: vec![],
            field_sites: vec![],
            object_sites: vec![],
            superinstructions: vec![],
            register_roots,
        };
        program
            .validate()
            .map_err(|e| Diagnostic::unsupported(name, e))?;
        Ok(WasmFunction {
            program,
            signature: signatures.into_iter().nth(entry as usize).unwrap(),
            entry,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowering_rejects_invalid_control_structure_and_stack_domains() {
        for operators in [
            vec![Operator::Else, Operator::End],
            vec![
                Operator::Br {
                    relative_depth: u32::MAX,
                },
                Operator::End,
            ],
            vec![
                Operator::Block {
                    blockty: wasmparser::BlockType::Empty,
                },
                Operator::End,
            ],
            vec![
                Operator::I32Const { value: 1 },
                Operator::Block {
                    blockty: wasmparser::BlockType::Empty,
                },
                Operator::Drop,
                Operator::End,
                Operator::End,
            ],
            vec![
                Operator::I32Const { value: 1 },
                Operator::If {
                    blockty: wasmparser::BlockType::Type(wasmparser::ValType::I32),
                },
                Operator::I32Const { value: 2 },
                Operator::End,
                Operator::End,
            ],
        ] {
            assert!(
                Engine::lower_wasm_i32_function(
                    "invalid control",
                    0,
                    0,
                    false,
                    operators.into_iter().map(Ok)
                )
                .is_err()
            );
        }
    }

    #[test]
    fn lowering_boundary_rejects_invalid_operand_and_local_domains() {
        for operators in [
            vec![Operator::I32Add, Operator::End],
            vec![Operator::Call { function_index: 1 }, Operator::End],
            vec![Operator::Call { function_index: 0 }, Operator::End],
            vec![Operator::LocalGet { local_index: 1 }, Operator::End],
            vec![Operator::LocalSet { local_index: 0 }, Operator::End],
            vec![Operator::I32Const { value: 1 }],
            vec![Operator::End],
            vec![
                Operator::I32Const { value: 1 },
                Operator::End,
                Operator::Nop,
            ],
        ] {
            assert!(
                Engine::lower_wasm_i32_function(
                    "invalid",
                    1,
                    0,
                    true,
                    operators.into_iter().map(Ok)
                )
                .is_err()
            );
        }
        assert!(
            Engine::lower_wasm_i32_function("locals", u16::MAX, 1, false, [Ok(Operator::End)])
                .is_err()
        );
    }

    #[test]
    fn lowering_rejects_stack_depth_outside_result_register_layout() {
        let operators = std::iter::repeat_with(|| Ok(Operator::I32Const { value: 1 }))
            .take(usize::from(crate::bytecode::REGISTER_MASK) + 2);
        assert!(Engine::lower_wasm_i32_function("stack", 0, 0, true, operators).is_err());
    }

    #[test]
    fn residual_validation_rejects_unknown_wasm_numeric_selectors() {
        for (operator, opcode, ty, result_type) in [
            (
                Operator::I32Add,
                Op::WasmI32Binary,
                WasmType::I32,
                WasmType::I32,
            ),
            (
                Operator::I32Eqz,
                Op::WasmI32Unary,
                WasmType::I32,
                WasmType::I32,
            ),
            (
                Operator::I64Add,
                Op::WasmI64Binary,
                WasmType::I64,
                WasmType::I64,
            ),
            (
                Operator::I64Clz,
                Op::WasmI64Unary,
                WasmType::I64,
                WasmType::I64,
            ),
            (
                Operator::I32WrapI64,
                Op::WasmIntegerConvert,
                WasmType::I64,
                WasmType::I32,
            ),
        ] {
            let mut operators = vec![Operator::LocalGet { local_index: 0 }];
            if opcode
                .field_layout(crate::bytecode::InstructionField::C)
                .is_register_field()
            {
                operators.push(Operator::LocalGet { local_index: 0 });
            }
            operators.extend([operator, Operator::End]);
            let mut function = Engine::lower_wasm_module(
                "selector",
                0,
                [WasmFunctionBody {
                    signature: WasmSignature {
                        params: vec![ty],
                        result: Some(result_type),
                    },
                    locals: vec![],
                    operators: operators.into_iter().map(Ok),
                }],
            )
            .unwrap();
            let residual = &mut function.program.functions[0];
            let pc = residual.code.iter().position(|i| i.op() == opcode).unwrap();
            let index = residual.wide.len();
            let instruction = residual.code[pc];
            residual.wide.push(WideInstruction::new(
                opcode,
                instruction.a(),
                instruction.b(),
                instruction.c(),
                u32::MAX,
            ));
            residual.code[pc] = Instr::wide(index).unwrap();
            assert!(function.program.validate().is_err());
            let error = crate::Runtime::new(crate::SystemHost)
                .execute_wasm(&function, &[ty.zero()])
                .unwrap_err();
            assert_eq!(error.wasm_trap(), None);
        }
    }

    #[test]
    fn serialized_i64_numeric_rows_execute_after_decoding() {
        let mut function = Engine::lower_wasm_module(
            "i64 round trip",
            0,
            [WasmFunctionBody {
                signature: WasmSignature {
                    params: vec![WasmType::I64; 2],
                    result: Some(WasmType::I64),
                },
                locals: vec![],
                operators: [
                    Operator::LocalGet { local_index: 0 },
                    Operator::LocalGet { local_index: 1 },
                    Operator::I64DivU,
                    Operator::I64Clz,
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            }],
        )
        .unwrap();
        let path =
            std::env::temp_dir().join(format!("quench-shared-i64-{}.qbc", std::process::id()));
        function.program.write_binary(&path).unwrap();
        let decoded = ResidualProgram::read_binary(&path);
        std::fs::remove_file(path).unwrap();
        function.program = decoded.unwrap();
        let mut runtime = crate::Runtime::new(crate::SystemHost);
        assert_eq!(
            runtime
                .execute_wasm(&function, &[WasmValue::I64(-1), WasmValue::I64(2)])
                .unwrap(),
            Some(WasmValue::I64(1))
        );
        assert_eq!(
            runtime
                .execute_wasm(&function, &[WasmValue::I64(1), WasmValue::I64(0)])
                .unwrap_err()
                .wasm_trap(),
            Some(WasmTrap::IntegerDivideByZero)
        );
    }

    #[test]
    fn serialized_wasm_bits64_preserve_tag_collision_payloads() {
        let bits = 0x7ffc_1234_5678_9abc_u64;
        let mut function = Engine::lower_wasm_module(
            "scalar round trip",
            0,
            [WasmFunctionBody {
                signature: WasmSignature {
                    params: vec![],
                    result: Some(WasmType::I64),
                },
                locals: vec![],
                operators: [Operator::I64Const { value: bits as i64 }, Operator::End]
                    .into_iter()
                    .map(Ok),
            }],
        )
        .unwrap();
        let path =
            std::env::temp_dir().join(format!("quench-shared-scalars-{}.qbc", std::process::id()));
        function.program.write_binary(&path).unwrap();
        let decoded = ResidualProgram::read_binary(&path);
        std::fs::remove_file(path).unwrap();
        function.program = decoded.unwrap();
        assert_eq!(
            crate::Runtime::new(crate::SystemHost)
                .execute_wasm(&function, &[])
                .unwrap(),
            Some(WasmValue::I64(bits as i64))
        );
    }

    #[test]
    fn serialized_wasm_numeric_rows_execute_after_decoding() {
        let operators = [
            Operator::LocalGet { local_index: 0 },
            Operator::LocalGet { local_index: 1 },
            Operator::I32DivU,
            Operator::I32Clz,
            Operator::End,
        ];
        let mut function = Engine::lower_wasm_i32_function(
            "round-trip",
            2,
            0,
            true,
            operators.into_iter().map(Ok),
        )
        .unwrap();
        let path =
            std::env::temp_dir().join(format!("quench-shared-i32-{}.qbc", std::process::id()));
        function.program.write_binary(&path).unwrap();
        let decoded = ResidualProgram::read_binary(&path);
        std::fs::remove_file(path).unwrap();
        function.program = decoded.unwrap();
        let mut runtime = crate::Runtime::new(crate::SystemHost);
        assert_eq!(
            runtime.execute_wasm_i32(&function, &[-1, 2]).unwrap(),
            Some(1)
        );
        assert_eq!(
            runtime
                .execute_wasm_i32(&function, &[1, 0])
                .unwrap_err()
                .wasm_trap(),
            Some(WasmTrap::IntegerDivideByZero)
        );
    }
}

struct Lowering<'a> {
    signatures: &'a [WasmSignature],
    name: &'a str,
    locals: u16,
    code: Vec<Instr>,
    wide: Vec<WideInstruction>,
    constants: Vec<Constant>,
    depth: Register,
    registers: u16,
    controls: Vec<Control>,
    path: Reachability,
}

impl Lowering<'_> {
    fn emit(
        &mut self,
        op: Op,
        a: Register,
        b: Register,
        c: Register,
        imm: u32,
    ) -> Result<(), Diagnostic> {
        let instruction = match Instr::try_new(op, a, b, c, imm) {
            Some(instruction) => instruction,
            None => {
                let marker = Instr::wide(self.wide.len())
                    .ok_or_else(|| Diagnostic::unsupported(self.name, "Wasm residual too large"))?;
                self.wide.push(WideInstruction::new(op, a, b, c, imm));
                marker
            }
        };
        self.code.push(instruction);
        Ok(())
    }

    fn push(&mut self) -> Result<Register, Diagnostic> {
        let register = self.depth;
        if register > crate::bytecode::REGISTER_MASK {
            return Err(Diagnostic::unsupported(
                self.name,
                "Wasm operand stack exceeds register layout",
            ));
        }
        self.depth = self
            .depth
            .checked_add(1)
            .ok_or_else(|| Diagnostic::unsupported(self.name, "Wasm operand stack too large"))?;
        self.registers = self.registers.max(self.depth);
        Ok(register)
    }

    fn pop(&mut self) -> Result<Register, Diagnostic> {
        if self.depth <= self.control_base() {
            return Err(Diagnostic::unsupported(
                self.name,
                "Wasm operand stack underflow",
            ));
        }
        self.depth = self
            .depth
            .checked_sub(1)
            .ok_or_else(|| Diagnostic::unsupported(self.name, "Wasm operand stack underflow"))?;
        Ok(self.depth)
    }

    fn load_i32(&mut self, result: Register, value: i32) -> Result<(), Diagnostic> {
        self.load_scalar(result, WasmValue::I32(value))
    }

    fn load_zero(&mut self, result: Register, ty: WasmType) -> Result<(), Diagnostic> {
        if matches!(ty, WasmType::I32 | WasmType::F32) {
            return self.emit(Op::LoadConst, result, 0, 0, ZERO_LOCAL_CONSTANT);
        }
        if let Some(index) = self
            .constants
            .iter()
            .position(|constant| matches!(constant, Constant::WasmBits64(0)))
        {
            return self.emit(Op::LoadConst, result, 0, 0, index as u32);
        }
        self.load_scalar(result, ty.zero())
    }

    fn load_scalar(&mut self, result: Register, value: WasmValue) -> Result<(), Diagnostic> {
        let constant = u32::try_from(self.constants.len())
            .map_err(|_| Diagnostic::unsupported(self.name, "too many Wasm constants"))?;
        self.constants.push(value.constant());
        self.emit(Op::LoadConst, result, 0, 0, constant)
    }

    fn operator(&mut self, operator: Operator<'_>) -> Result<(), Diagnostic> {
        if self.control_operator(&operator)? {
            return Ok(());
        }
        let numeric = I32BinaryOperator::from_wasm(&operator)
            .map(|op| (Op::WasmI32Binary, op as u32))
            .or_else(|| {
                I32UnaryOperator::from_wasm(&operator).map(|op| (Op::WasmI32Unary, op as u32))
            })
            .or_else(|| {
                I64BinaryOperator::from_wasm(&operator).map(|op| (Op::WasmI64Binary, op as u32))
            })
            .or_else(|| {
                I64UnaryOperator::from_wasm(&operator).map(|op| (Op::WasmI64Unary, op as u32))
            });
        let numeric = numeric.or_else(|| {
            IntegerConversionOperator::from_wasm(&operator)
                .map(|op| (Op::WasmIntegerConvert, op as u32))
        });
        if let Some((op, selector)) = numeric {
            if self.path == Reachability::Dead {
                return Ok(());
            }
            let right = if op
                .field_layout(crate::bytecode::InstructionField::C)
                .is_register_field()
            {
                self.pop()?
            } else {
                0
            };
            let left = self.pop()?;
            let result = self.push()?;
            return self.emit(op, result, left, right, selector);
        }
        match operator {
            Operator::I32Const { .. }
            | Operator::I64Const { .. }
            | Operator::F32Const { .. }
            | Operator::F64Const { .. } => {
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let result = self.push()?;
                let value = match operator {
                    Operator::I32Const { value } => WasmValue::I32(value),
                    Operator::I64Const { value } => WasmValue::I64(value),
                    Operator::F32Const { value } => WasmValue::F32(value.bits()),
                    Operator::F64Const { value } => WasmValue::F64(value.bits()),
                    _ => unreachable!(),
                };
                self.load_scalar(result, value)
            }
            Operator::LocalGet { local_index }
            | Operator::LocalSet { local_index }
            | Operator::LocalTee { local_index } => {
                if local_index >= u32::from(self.locals) {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm local out of bounds",
                    ));
                }
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let (op, register) = match operator {
                    Operator::LocalGet { .. } => (Op::LoadLocal, self.push()?),
                    Operator::LocalSet { .. } => (Op::StoreLocal, self.pop()?),
                    Operator::LocalTee { .. } => {
                        let register = self.pop()?;
                        self.push()?;
                        (Op::StoreLocal, register)
                    }
                    _ => unreachable!(),
                };
                self.emit(op, register, 0, 0, local_index)
            }
            Operator::Call { function_index } => {
                let signature = self
                    .signatures
                    .get(function_index as usize)
                    .ok_or_else(|| {
                        Diagnostic::unsupported(self.name, "Wasm call target out of bounds")
                    })?;
                let params = u16::try_from(signature.params.len()).map_err(|_| {
                    Diagnostic::unsupported(self.name, "too many Wasm call parameters")
                })?;
                let has_result = signature.result.is_some();
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let target = u16::try_from(function_index).map_err(|_| {
                    Diagnostic::unsupported(
                        self.name,
                        "Wasm call target exceeds function index layout",
                    )
                })?;
                let base = self
                    .depth
                    .checked_sub(params)
                    .filter(|base| *base >= self.control_base())
                    .ok_or_else(|| {
                        Diagnostic::unsupported(self.name, "Wasm call argument stack underflow")
                    })?;
                let immediate = ImmediateLayout::call_immediate(base, params, false, false);
                if ImmediateLayout::call_window_base(immediate) != base {
                    return Err(Diagnostic::unsupported(
                        self.name,
                        "Wasm call exceeds argument window layout",
                    ));
                }
                self.depth = base;
                // CallKnown always writes a shared Value. Void calls reserve a
                // scratch result slot without exposing it as a Wasm operand.
                let result = self.push()?;
                if !has_result {
                    self.depth = base;
                }
                self.emit(Op::CallKnown, result, target, 0, immediate)
            }
            Operator::Drop if self.path == Reachability::Live => self.pop().map(drop),
            Operator::Drop => Ok(()),
            Operator::Nop => Ok(()),
            operator => Err(Diagnostic::unsupported(
                self.name,
                format!("unsupported shared Wasm operator: {operator:?}"),
            )),
        }
    }
}
