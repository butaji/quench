//! One initializer expression, evaluated against static or instance global facts.

use super::integer::{I32BinaryOperator, I64BinaryOperator};
use super::{WasmGlobal, WasmGlobalInitializer, WasmType, WasmValue, WasmValueScalarExt};
use crate::{Diagnostic, Engine};
use std::rc::Rc;
use wasmparser::{BinaryReaderError, Operator};

#[derive(Clone, Debug, PartialEq, Eq)]
enum ConstantOperator {
    Value(WasmValue),
    Null(wasmparser::HeapType),
    Global(u32),
    Function(u32),
    I31,
    ExternalConversion(super::reference::ExternalConversion),
    Struct {
        index: u32,
        mode: super::gc::StructConstruction,
    },
    Array {
        index: u32,
        mode: ArrayConstructor,
    },
    I32(I32BinaryOperator),
    I64(I64BinaryOperator),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArrayConstructor {
    Repeated,
    Default,
    Fixed(u32),
}

pub(crate) enum WasmArrayInitializer<'a> {
    Repeated { count: u32, value: WasmValue },
    Default(u32),
    Fixed(&'a [WasmValue]),
}

pub(crate) enum WasmInitializerEffect<'a> {
    ExternalConversion {
        conversion: super::reference::ExternalConversion,
        value: WasmValue,
    },
    Global(u32),
    Function(u32),
    Array {
        index: u32,
        initialization: WasmArrayInitializer<'a>,
    },
    Struct {
        index: u32,
        fields: Option<&'a [WasmValue]>,
        descriptor: Option<WasmValue>,
    },
}

/// The declaration's result contract and admitted initializer operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WasmConstantExpression {
    expected: WasmType,
    representation: ConstantExpression,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ConstantExpression {
    Known(WasmValue),
    Dependent { operators: Rc<[ConstantOperator]> },
}

impl From<WasmValue> for WasmConstantExpression {
    fn from(value: WasmValue) -> Self {
        Self {
            expected: value.ty(),
            representation: ConstantExpression::Known(value),
        }
    }
}

impl From<u32> for WasmConstantExpression {
    fn from(value: u32) -> Self {
        WasmValue::I32(value as i32).into()
    }
}

impl WasmConstantExpression {
    pub fn value_type(&self) -> WasmType {
        self.expected
    }

    pub(crate) fn evaluate(
        &self,
        name: &str,
        declarations: Option<&super::WasmTypes>,
        mut dependency: impl FnMut(WasmInitializerEffect<'_>) -> Result<WasmValue, Diagnostic>,
    ) -> Result<WasmValue, Diagnostic> {
        let expected = &self.expected;
        let operators = match &self.representation {
            ConstantExpression::Known(value) => return Ok(*value),
            ConstantExpression::Dependent { operators } => operators,
        };
        let error = |message: &str| Diagnostic::unsupported(name, message);
        let mut stack = Vec::new();
        for operator in operators.iter() {
            let value = match *operator {
                ConstantOperator::Value(value) => value,
                ConstantOperator::Null(heap) => {
                    // The validator checks the null heap type.
                    let reference = wasmparser::RefType::new(true, heap)
                        .expect("validated: null heap type");
                    let ty = wasmparser::ValType::Ref(reference);
                    let ty = match declarations {
                        Some(owner) => owner.callable_value_type(ty),
                        None => WasmType::from_wasm(ty),
                    }
                    .ok_or_else(|| error("Wasm null initializer requires a declaration owner"))?;
                    ty.default_value()
                        .ok_or_else(|| error("invalid Wasm null result contract"))?
                }
                ConstantOperator::Array { index, mode } => {
                    let owner = declarations.ok_or_else(|| {
                        error("Wasm array initializer requires a declaration owner")
                    })?;
                    owner
                        .array_field(index)
                        .ok_or_else(|| error("invalid Wasm array initializer declaration"))?;
                    let base;
                    let initialization = match mode {
                        ArrayConstructor::Fixed(count) => {
                            base = stack
                                .len()
                                .checked_sub(count as usize)
                                .expect("validated: array initializer operands");
                            WasmArrayInitializer::Fixed(&stack[base..])
                        }
                        ArrayConstructor::Default | ArrayConstructor::Repeated => {
                            let Some(WasmValue::I32(count)) = stack.pop() else {
                                unreachable!("validated: array initializer length is i32")
                            };
                            let initialization = if mode == ArrayConstructor::Default {
                                WasmArrayInitializer::Default(count as u32)
                            } else {
                                let value = stack.pop().expect("validated: repeated array value");
                                WasmArrayInitializer::Repeated {
                                    count: count as u32,
                                    value,
                                }
                            };
                            base = stack.len();
                            initialization
                        }
                    };
                    let result = dependency(WasmInitializerEffect::Array {
                        index,
                        initialization,
                    })?;
                    stack.truncate(base);
                    result
                }
                ConstantOperator::Struct { index, mode } => {
                    let owner = declarations.ok_or_else(|| {
                        error("Wasm struct initializer requires a declaration owner")
                    })?;
                    let fields = owner
                        .struct_fields(index)
                        .ok_or_else(|| error("invalid Wasm struct initializer declaration"))?;
                    let descriptor = if mode.described() {
                        Some(stack.pop().expect("validated: descriptor initializer operand"))
                    } else {
                        None
                    };
                    let base = stack
                        .len()
                        .checked_sub(if mode.defaulted() { 0 } else { fields.len() })
                        .expect("validated: struct initializer operands");
                    let result = dependency(WasmInitializerEffect::Struct {
                        index,
                        fields: if mode.defaulted() {
                            None
                        } else {
                            Some(&stack[base..])
                        },
                        descriptor,
                    })?;
                    stack.truncate(base);
                    result
                }
                ConstantOperator::Global(index) => {
                    dependency(WasmInitializerEffect::Global(index))?
                }
                ConstantOperator::Function(index) => {
                    dependency(WasmInitializerEffect::Function(index))?
                }
                ConstantOperator::ExternalConversion(conversion) => {
                    let value = stack.pop().expect("validated: conversion initializer operand");
                    dependency(WasmInitializerEffect::ExternalConversion { conversion, value })?
                }
                ConstantOperator::I31 => {
                    let value = stack.pop().expect("validated: i31 operand");
                    super::i31::I31Operator::New
                        .apply(value)
                        .map_err(|_| error("invalid Wasm i31 initializer"))?
                }
                ConstantOperator::I32(_) | ConstantOperator::I64(_) => {
                    let right = stack.pop().expect("validated: binary constant operands");
                    let left = stack.pop().expect("validated: binary constant operands");
                    let value = match (operator, left, right) {
                        (
                            ConstantOperator::I32(op),
                            WasmValue::I32(left),
                            WasmValue::I32(right),
                        ) => op.evaluate(left, right).map(WasmValue::I32),
                        (
                            ConstantOperator::I64(op),
                            WasmValue::I64(left),
                            WasmValue::I64(right),
                        ) => op.evaluate(left, right),
                        _ => unreachable!("validated: constant operands match the operator"),
                    };
                    value.map_err(|trap| Diagnostic::unsupported(name, trap.to_string()))?
                }
            };
            stack.push(value);
        }
        // The validator checks that the expression leaves one value of the declared type.
        let value = stack.pop().expect("validated: constant expression result");
        debug_assert!(stack.is_empty() && value.fits_type(*expected));
        Ok(value)
    }
}

impl Engine {
    /// Admit supported operators once; immutable dependencies remain symbolic.
    pub fn lower_wasm_constant_expression<'a>(
        name: &str,
        expected: WasmType,
        globals: &[WasmGlobal],
        operators: impl IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>>,
    ) -> Result<WasmConstantExpression, Diagnostic> {
        let error = |message: &str| Diagnostic::unsupported(name, message);
        let mut admitted = Vec::new();
        let mut operators = operators.into_iter();
        while let Some(operator) = operators.next() {
            let operator =
                operator.map_err(|error| Diagnostic::unsupported(name, error.to_string()))?;
            let operator = match operator {
                Operator::RefI31 => ConstantOperator::I31,
                Operator::ArrayNew { array_type_index } => ConstantOperator::Array {
                    index: array_type_index,
                    mode: ArrayConstructor::Repeated,
                },
                Operator::ArrayNewDefault { array_type_index } => ConstantOperator::Array {
                    index: array_type_index,
                    mode: ArrayConstructor::Default,
                },
                Operator::ArrayNewFixed {
                    array_type_index,
                    array_size,
                } => ConstantOperator::Array {
                    index: array_type_index,
                    mode: ArrayConstructor::Fixed(array_size),
                },
                operator if super::gc::StructConstruction::from_operator(&operator).is_some() => {
                    let (index, mode) =
                        super::gc::StructConstruction::from_operator(&operator).unwrap();
                    ConstantOperator::Struct { index, mode }
                }
                Operator::AnyConvertExtern => ConstantOperator::ExternalConversion(
                    super::reference::ExternalConversion::Internalize,
                ),
                Operator::ExternConvertAny => ConstantOperator::ExternalConversion(
                    super::reference::ExternalConversion::Externalize,
                ),
                Operator::RefNull { hty } => ConstantOperator::Null(hty),
                Operator::I32Const { value } => ConstantOperator::Value(WasmValue::I32(value)),
                Operator::I64Const { value } => ConstantOperator::Value(WasmValue::I64(value)),
                Operator::V128Const { value } => {
                    ConstantOperator::Value(WasmValue::V128(u128::from_le_bytes(*value.bytes())))
                }
                Operator::F32Const { value } => {
                    ConstantOperator::Value(WasmValue::F32(value.bits()))
                }
                Operator::F64Const { value } => {
                    ConstantOperator::Value(WasmValue::F64(value.bits()))
                }
                Operator::RefFunc { function_index } => ConstantOperator::Function(function_index),
                Operator::GlobalGet { global_index } => {
                    let global = globals
                        .get(global_index as usize)
                        .filter(|global| !global.mutable)
                        .ok_or_else(|| error("invalid Wasm constant global dependency"))?;
                    match &global.initial {
                        WasmGlobalInitializer::Expression(WasmConstantExpression {
                            representation: ConstantExpression::Known(value),
                            expected: ty,
                        }) if !ty.requires_declarations() => ConstantOperator::Value(*value),
                        _ => ConstantOperator::Global(global_index),
                    }
                }
                Operator::End => {
                    if operators.next().is_some() {
                        return Err(error("operators after Wasm constant expression end"));
                    }
                    let dependent = admitted.iter().any(|operator| {
                        matches!(
                            operator,
                            ConstantOperator::Global(_)
                                | ConstantOperator::ExternalConversion(_)
                                | ConstantOperator::Array { .. }
                                | ConstantOperator::Struct { .. }
                                | ConstantOperator::Function(_)
                                | ConstantOperator::Null(_)
                        )
                    });
                    let expression = WasmConstantExpression {
                        expected,
                        representation: ConstantExpression::Dependent {
                            operators: admitted.into(),
                        },
                    };
                    return if dependent {
                        Ok(expression)
                    } else {
                        expression
                            .evaluate(name, None, |_| {
                                unreachable!("static initializer has no instance dependencies")
                            })
                            .map(|value| WasmConstantExpression {
                                expected,
                                representation: ConstantExpression::Known(value),
                            })
                    };
                }
                operator => {
                    if let Some(op) = I32BinaryOperator::from_wasm(&operator)
                        .filter(|op| op.allowed_in_constant_expression())
                    {
                        ConstantOperator::I32(op)
                    } else if let Some(op) = I64BinaryOperator::from_wasm(&operator)
                        .filter(|op| op.allowed_in_constant_expression())
                    {
                        ConstantOperator::I64(op)
                    } else {
                        return Err(error("unsupported Wasm constant expression operator"));
                    }
                }
            };
            admitted.push(operator);
        }
        Err(error("missing Wasm constant expression end"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initializer_facts_fold_once_and_unresolved_dependencies_remain_instance_owned() {
        let imported = WasmGlobal {
            mutable: false,
            initial: WasmGlobalInitializer::Import {
                ty: WasmType::I32,
                name: super::super::WasmImportName {
                    index: 0,
                    module: "owner".into(),
                    name: "offset".into(),
                },
            },
        };
        let operators = || {
            [
                Operator::GlobalGet { global_index: 0 },
                Operator::I32Const { value: 3 },
                Operator::I32Mul,
                Operator::End,
            ]
            .into_iter()
            .map(Ok)
        };
        let globals = [imported];
        let function = Engine::lower_wasm_constant_expression(
            "function-facts",
            WasmType::FUNCREF,
            &[],
            [Operator::RefFunc { function_index: 0 }, Operator::End]
                .into_iter()
                .map(Ok),
        )
        .unwrap();
        assert!(matches!(
            &function.representation,
            ConstantExpression::Dependent { operators, .. }
                if matches!(operators.as_ref(), [ConstantOperator::Function(_)])
        ));

        let allocated = Engine::lower_wasm_constant_expression(
            "instance-owned-struct",
            WasmType::Reference {
                kind: super::super::WasmReferenceKind::Internal(wasmparser::AbstractHeapType::Any),
                nullable: true,
            },
            &[],
            [
                Operator::StructNewDefault {
                    struct_type_index: 0,
                },
                Operator::End,
            ]
            .into_iter()
            .map(Ok),
        )
        .unwrap();
        assert!(matches!(
            allocated.representation,
            ConstantExpression::Dependent { .. }
        ));
        assert!(
            allocated
                .evaluate("ownerless-struct", None, |_| panic!(
                    "ownerless allocation must reject before effects"
                ))
                .is_err()
        );

        for constructor in [
            Operator::ArrayNewDefault {
                array_type_index: 0,
            },
            Operator::ArrayNewFixed {
                array_type_index: 0,
                array_size: 1,
            },
        ] {
            let array = Engine::lower_wasm_constant_expression(
                "array-facts",
                WasmType::Reference {
                    kind: super::super::WasmReferenceKind::Internal(
                        wasmparser::AbstractHeapType::Any,
                    ),
                    nullable: true,
                },
                &[],
                [Operator::I32Const { value: 1 }, constructor, Operator::End]
                    .into_iter()
                    .map(Ok),
            )
            .unwrap();
            assert!(matches!(
                array.representation,
                ConstantExpression::Dependent { .. }
            ));
            assert!(
                array
                    .evaluate("ownerless-array", None, |_| panic!(
                        "ownerless allocation must reject before effects"
                    ))
                    .is_err()
            );
        }
        // A function reference is an instance's fact: with no owner its dependency stays unresolved.
        assert!(function.evaluate("function-facts", None, |_| Err(Diagnostic::unsupported("function-facts", "no owner"))).is_err());
        let expression = Engine::lower_wasm_constant_expression(
            "initializer-facts",
            WasmType::I32,
            &globals,
            operators(),
        )
        .unwrap();
        assert!(matches!(
            expression.representation,
            ConstantExpression::Dependent { .. }
        ));
        assert!(expression.evaluate("initializer-facts", None, |_| Err(Diagnostic::unsupported("initializer-facts", "unresolved import"))).is_err());
        for value in [7, 11] {
            assert_eq!(
                expression
                    .evaluate("instance", None, |dependency| {
                        assert!(matches!(dependency, WasmInitializerEffect::Global(0)));
                        Ok(WasmValue::I32(value))
                    })
                    .unwrap(),
                WasmValue::I32(value * 3)
            );
        }
        let globals = [WasmGlobal {
            initial: WasmValue::I32(7).into(),
            mutable: false,
        }];
        let folded = Engine::lower_wasm_constant_expression(
            "initializer-facts",
            WasmType::I32,
            &globals,
            operators(),
        )
        .unwrap();
        assert_eq!(
            folded.representation,
            ConstantExpression::Known(WasmValue::I32(21))
        );
        assert_eq!(
            folded
                .evaluate("instance", None, |_| panic!(
                    "known fact needs no environment"
                ))
                .unwrap(),
            WasmValue::I32(21)
        );
        let invalid_globals = [WasmGlobal {
            mutable: true,
            ..globals[0].clone()
        }];
        assert!(
            Engine::lower_wasm_constant_expression(
                "initializer-facts",
                WasmType::I32,
                &invalid_globals,
                operators()
            )
            .is_err()
        );
        assert!(
            Engine::lower_wasm_constant_expression(
                "initializer-facts",
                WasmType::I32,
                &[],
                operators()
            )
            .is_err()
        );
        assert!(
            Engine::lower_wasm_constant_expression(
                "initializer-facts",
                WasmType::I64,
                &globals,
                operators()
            )
            .is_err()
        );
        assert!(
            Engine::lower_wasm_constant_expression(
                "initializer-facts",
                WasmType::I32,
                &globals,
                [
                    Operator::I32Const { value: 1 },
                    Operator::End,
                    Operator::End
                ]
                .into_iter()
                .map(Ok)
            )
            .is_err()
        );
    }
}
