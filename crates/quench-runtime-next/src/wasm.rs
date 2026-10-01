//! Wasm operator lowering into the shared residual vocabulary. Binary decoding
//! and module validation belong to quench-wasm, not this execution core.

use crate::bytecode::{
    AtomTable, Constant, DispatchClass, Function, Instr, Op, ProgramKind, Register,
    ResidualProgram, WideInstruction,
};
use crate::{Diagnostic, Engine};
use wasmparser::{BinaryReaderError, Operator};

mod control;
pub(crate) mod i32;
use control::{Control, Reachability};
use i32::{I32BinaryOperator, I32UnaryOperator};

/// A WebAssembly trap, distinct from a JavaScript throw or invalid residual.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmTrap {
    Unreachable,
    IntegerDivideByZero,
    IntegerOverflow,
}

impl std::fmt::Display for WasmTrap {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        output.write_str(match self {
            Self::Unreachable => "unreachable",
            Self::IntegerDivideByZero => "integer divide by zero",
            Self::IntegerOverflow => "integer overflow",
        })
    }
}

const ZERO_LOCAL_CONSTANT: u32 = 0;
const VOID_RESULT_CONSTANT: u32 = 1;

/// A lowered, standalone i32 function. This initial shared execution slice
/// supports locals, i32 numeric operators and structured control flow; module state and
/// cross-function calls require the subsequent module lowering work.
pub struct WasmI32Function {
    pub(crate) program: ResidualProgram,
    pub(crate) has_result: bool,
}

impl WasmI32Function {
    pub fn residual(&self) -> &ResidualProgram {
        &self.program
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
        let error = |message: &str| Diagnostic::unsupported(name, message);
        let local_count = params
            .checked_add(locals)
            .ok_or_else(|| error("too many Wasm locals"))?;
        let mut lowering = Lowering {
            name,
            locals: local_count,
            code: Vec::new(),
            wide: Vec::new(),
            constants: vec![Constant::Number(0.0), Constant::Undefined],
            depth: 0,
            registers: 1,
            controls: vec![Control::function(has_result)],
            path: Reachability::Live,
        };
        // Shared JS frames initialize non-parameter locals to undefined. Wasm
        // initialization is therefore explicit residual code, not another frame.
        lowering.emit(Op::LoadConst, 0, 0, 0, ZERO_LOCAL_CONSTANT)?;
        for slot in params..local_count {
            lowering.emit(Op::StoreLocal, 0, 0, 0, u32::from(slot))?;
        }
        for operator in operators {
            let operator = operator.map_err(|e| Diagnostic::unsupported(name, e.to_string()))?;
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
        let mut functions = vec![function];
        let register_roots = crate::compile::liveness::derive(&mut functions, &[], &[], &[]);
        let program = ResidualProgram {
            specialized: false,
            kind: ProgramKind::Wasm,
            module_requests: vec![],
            module_imports: vec![],
            module_link_plan: None,
            source_name: name.into(),
            atoms: AtomTable::default(),
            constants: lowering.constants,
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
        Ok(WasmI32Function {
            program,
            has_result,
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
        for (operator, opcode) in [
            (Operator::I32Add, Op::WasmI32Binary),
            (Operator::I32Eqz, Op::WasmI32Unary),
        ] {
            let mut operators = vec![Operator::LocalGet { local_index: 0 }];
            if opcode == Op::WasmI32Binary {
                operators.push(Operator::LocalGet { local_index: 0 });
            }
            operators.extend([operator, Operator::End]);
            let mut function = Engine::lower_wasm_i32_function(
                "selector",
                1,
                0,
                true,
                operators.into_iter().map(Ok),
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
                .execute_wasm_i32(&function, &[1])
                .unwrap_err();
            assert_eq!(error.wasm_trap(), None);
        }
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
        let constant = u32::try_from(self.constants.len())
            .map_err(|_| Diagnostic::unsupported(self.name, "too many Wasm constants"))?;
        self.constants.push(Constant::Number(f64::from(value)));
        self.emit(Op::LoadConst, result, 0, 0, constant)
    }

    fn operator(&mut self, operator: Operator<'_>) -> Result<(), Diagnostic> {
        if self.control_operator(&operator)? {
            return Ok(());
        }
        if let Some(operator) = I32BinaryOperator::from_wasm(&operator) {
            if self.path == Reachability::Dead {
                return Ok(());
            }
            let right = self.pop()?;
            let left = self.pop()?;
            let result = self.push()?;
            return self.emit(Op::WasmI32Binary, result, left, right, operator as u32);
        }
        if let Some(operator) = I32UnaryOperator::from_wasm(&operator) {
            if self.path == Reachability::Dead {
                return Ok(());
            }
            let value = self.pop()?;
            let result = self.push()?;
            return self.emit(Op::WasmI32Unary, result, value, 0, operator as u32);
        }
        match operator {
            Operator::I32Const { value } => {
                if self.path == Reachability::Dead {
                    return Ok(());
                }
                let result = self.push()?;
                self.load_i32(result, value)
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
