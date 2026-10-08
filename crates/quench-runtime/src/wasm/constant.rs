//! One initializer expression, evaluated against static or instance global facts.

use super::integer::{I32BinaryOperator, I64BinaryOperator};
use super::{WasmGlobal, WasmGlobalInitializer, WasmType, WasmValue};
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

enum InitializerType {
    Value(WasmType),
    Function(u32),
}

impl InitializerType {
    fn matches(&self, expected: WasmType, signatures: Option<&super::WasmSignatures>) -> bool {
        let actual = match (self, signatures) {
            (Self::Function(function), Some(pool)) => {
                let Some(ty) = pool.function_reference_type(*function as usize) else {
                    return false;
                };
                // Embeddings have an implicit final function type, without a
                // module declaration index. Declared functions use the ordinary
                // reference subtype operation, including static import exactness.
                if !ty.requires_declarations() {
                    if let WasmType::Reference {
                        kind: super::WasmReferenceKind::DeclaredFunction { index, exact },
                        ..
                    } = expected
                    {
                        return pool.function_type_index(*function as usize).is_some_and(
                            |actual| {
                                pool.matches_declaration(actual, &pool.declarations, index, exact)
                            },
                        );
                    }
                }
                ty
            }
            (Self::Function(_), None)
                if matches!(
                    expected,
                    WasmType::Reference {
                        kind: super::WasmReferenceKind::DeclaredFunction { .. },
                        ..
                    }
                ) =>
            {
                return true;
            }
            (Self::Function(_), None) => WasmType::FUNC,
            (Self::Value(ty), _) => *ty,
        };
        match signatures {
            Some(pool) => pool
                .declarations
                .value_subtype(actual, &pool.declarations, expected),
            None if actual.requires_declarations() || expected.requires_declarations() => {
                match (actual.reference_type(), expected.reference_type()) {
                    (Some(source), Some(target)) => !source.is_nullable() || target.is_nullable(),
                    _ => false,
                }
            }
            None => actual.is_subtype_of(expected),
        }
    }
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
    pub(crate) fn produces_non_null_function(&self) -> bool {
        matches!(&self.representation,
            ConstantExpression::Dependent { operators, .. }
                if matches!(operators.as_ref(), [ConstantOperator::Function(_)]))
    }

    pub fn value_type(&self) -> WasmType {
        self.expected
    }

    pub(crate) fn validate(&self, name: &str, globals: &[WasmGlobal]) -> Result<(), Diagnostic> {
        self.validate_in(name, globals, None)
    }

    fn validate_in(
        &self,
        name: &str,
        globals: &[WasmGlobal],
        signatures: Option<&super::WasmSignatures>,
    ) -> Result<(), Diagnostic> {
        let expected = self.expected;
        let error = |message: &str| Diagnostic::unsupported(name, message);
        let ConstantExpression::Dependent { operators } = &self.representation else {
            return Ok(());
        };
        // Aggregate storage types require the completed declaration graph.
        // Full module validation runs before any instance effects.
        if signatures.is_none()
            && operators.iter().any(|op| {
                matches!(
                    op,
                    ConstantOperator::Struct { .. } | ConstantOperator::Array { .. }
                )
            })
        {
            return Ok(());
        }
        let mut stack = Vec::new();
        for operator in operators.iter() {
            let ty = match operator {
                ConstantOperator::Value(value) => value.ty(),
                ConstantOperator::Null(heap) => {
                    let reference = wasmparser::RefType::new(true, *heap)
                        .ok_or_else(|| error("invalid Wasm null type"))?;
                    let ty = wasmparser::ValType::Ref(reference);
                    match signatures {
                        Some(pool) => pool.declarations.callable_value_type(ty),
                        None => WasmType::from_wasm(ty)
                            .or_else(|| expected.requires_declarations().then_some(expected)),
                    }
                    .ok_or_else(|| error("unsupported Wasm null type"))?
                }
                ConstantOperator::Function(index) => {
                    if signatures
                        .is_some_and(|pool| pool.function_type_index(*index as usize).is_none())
                    {
                        return Err(error("Wasm initializer function index out of bounds"));
                    }
                    stack.push(InitializerType::Function(*index));
                    continue;
                }
                ConstantOperator::Array { index, mode } => {
                    let pool = signatures
                        .ok_or_else(|| error("Wasm array initializer requires declarations"))?;
                    let field = pool
                        .declarations
                        .array_field(*index)
                        .ok_or_else(|| error("invalid Wasm array initializer declaration"))?;
                    let ty = match field.element_type {
                        wasmparser::StorageType::I8 | wasmparser::StorageType::I16 => WasmType::I32,
                        wasmparser::StorageType::Val(ty) => pool
                            .declarations
                            .callable_value_type(ty)
                            .ok_or_else(|| error("unsupported Wasm array initializer element"))?,
                    };
                    let operands = match mode {
                        ArrayConstructor::Repeated | ArrayConstructor::Default => {
                            if !stack
                                .pop()
                                .is_some_and(|actual| actual.matches(WasmType::I32, signatures))
                            {
                                return Err(error("Wasm array initializer length type mismatch"));
                            }
                            if *mode == ArrayConstructor::Default {
                                if ty.default_value().is_none() {
                                    return Err(error(
                                        "non-defaultable Wasm array initializer element",
                                    ));
                                }
                                0
                            } else {
                                1
                            }
                        }
                        ArrayConstructor::Fixed(count) => *count as usize,
                    };
                    if operands > stack.len() {
                        return Err(error("Wasm array initializer operand underflow"));
                    }
                    for _ in 0..operands {
                        if !stack
                            .pop()
                            .is_some_and(|actual| actual.matches(ty, signatures))
                        {
                            return Err(error("Wasm array initializer element type mismatch"));
                        }
                    }
                    WasmType::Reference {
                        kind: super::WasmReferenceKind::DeclaredGc {
                            index: *index,
                            exact: true,
                        },
                        nullable: false,
                    }
                }
                ConstantOperator::Struct { index, mode } => {
                    let pool = signatures
                        .ok_or_else(|| error("Wasm struct initializer requires declarations"))?;
                    let fields = pool
                        .declarations
                        .struct_fields(*index)
                        .ok_or_else(|| error("invalid Wasm struct initializer declaration"))?;
                    let descriptor = pool.declarations.descriptor_type(*index);
                    if mode.described() != descriptor.is_some() {
                        return Err(error("Wasm struct constructor descriptor mismatch"));
                    }
                    if let Some(ty) = descriptor {
                        let actual = stack.pop().ok_or_else(|| {
                            error("Wasm descriptor initializer operand underflow")
                        })?;
                        if !actual.matches(ty, signatures) {
                            return Err(error("Wasm descriptor initializer type mismatch"));
                        }
                    }
                    for field in fields.iter().rev() {
                        let ty = match field.element_type {
                            wasmparser::StorageType::I8 | wasmparser::StorageType::I16 => {
                                WasmType::I32
                            }
                            wasmparser::StorageType::Val(ty) => {
                                pool.declarations.callable_value_type(ty).ok_or_else(|| {
                                    error("unsupported Wasm struct initializer field")
                                })?
                            }
                        };
                        if mode.defaulted() {
                            if ty.default_value().is_none() {
                                return Err(error("non-defaultable Wasm struct initializer field"));
                            }
                        } else {
                            let actual = stack.pop().ok_or_else(|| {
                                error("Wasm struct initializer operand underflow")
                            })?;
                            if !actual.matches(ty, signatures) {
                                return Err(error("Wasm struct initializer field type mismatch"));
                            }
                        }
                    }
                    WasmType::Reference {
                        kind: super::WasmReferenceKind::DeclaredGc {
                            index: *index,
                            exact: true,
                        },
                        nullable: false,
                    }
                }
                ConstantOperator::Global(index) => {
                    globals
                        .get(*index as usize)
                        .filter(|global| !global.mutable)
                        .ok_or_else(|| error("invalid Wasm constant global dependency"))?;
                    WasmGlobal::value_type(globals, *index as usize)
                        .ok_or_else(|| error("invalid Wasm constant global type"))?
                }
                ConstantOperator::ExternalConversion(conversion) => {
                    let input = stack
                        .pop()
                        .ok_or_else(|| error("Wasm conversion initializer operand underflow"))?;
                    if !input.matches(conversion.input_type(), signatures) {
                        return Err(error("Wasm conversion initializer operand type mismatch"));
                    }
                    let nullable = matches!(
                        input,
                        InitializerType::Value(WasmType::Reference { nullable: true, .. })
                    );
                    conversion.output_type(nullable)
                }
                ConstantOperator::I31 => {
                    if !matches!(stack.pop(), Some(InitializerType::Value(WasmType::I32))) {
                        return Err(error("Wasm i31 initializer operand type mismatch"));
                    }
                    super::i31::REFERENCE_TYPE
                }
                ConstantOperator::I32(_) | ConstantOperator::I64(_) => {
                    let ty = if matches!(operator, ConstantOperator::I32(_)) {
                        WasmType::I32
                    } else {
                        WasmType::I64
                    };
                    if !stack
                        .pop()
                        .is_some_and(|actual| actual.matches(ty, signatures))
                        || !stack
                            .pop()
                            .is_some_and(|actual| actual.matches(ty, signatures))
                    {
                        return Err(error("Wasm constant operand type mismatch"));
                    }
                    ty
                }
            };
            stack.push(InitializerType::Value(ty));
        }
        let valid = stack.len() == 1 && stack[0].matches(expected, signatures);
        if !valid {
            return Err(error("invalid Wasm constant expression result"));
        }
        Ok(())
    }

    pub(crate) fn validate_functions(
        &self,
        name: &str,
        signatures: &super::WasmSignatures,
        globals: &[WasmGlobal],
    ) -> Result<(), Diagnostic> {
        self.validate_in(name, globals, Some(signatures))
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
                    let reference = wasmparser::RefType::new(true, heap)
                        .ok_or_else(|| error("invalid Wasm null type"))?;
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
                                .ok_or_else(|| error("Wasm array initializer operand underflow"))?;
                            WasmArrayInitializer::Fixed(&stack[base..])
                        }
                        ArrayConstructor::Default | ArrayConstructor::Repeated => {
                            let Some(WasmValue::I32(count)) = stack.pop() else {
                                return Err(error("Wasm array initializer length type mismatch"));
                            };
                            let initialization = if mode == ArrayConstructor::Default {
                                WasmArrayInitializer::Default(count as u32)
                            } else {
                                let value = stack.pop().ok_or_else(|| {
                                    error("Wasm array initializer operand underflow")
                                })?;
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
                        Some(stack.pop().ok_or_else(|| {
                            error("Wasm descriptor initializer operand underflow")
                        })?)
                    } else {
                        None
                    };
                    let base = stack
                        .len()
                        .checked_sub(if mode.defaulted() { 0 } else { fields.len() })
                        .ok_or_else(|| error("Wasm struct initializer operand underflow"))?;
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
                    let value = stack
                        .pop()
                        .ok_or_else(|| error("Wasm conversion initializer operand underflow"))?;
                    dependency(WasmInitializerEffect::ExternalConversion { conversion, value })?
                }
                ConstantOperator::I31 => {
                    let value = stack
                        .pop()
                        .ok_or_else(|| error("Wasm constant operand underflow"))?;
                    super::i31::I31Operator::New
                        .apply(value)
                        .map_err(|_| error("invalid Wasm i31 initializer"))?
                }
                ConstantOperator::I32(_) | ConstantOperator::I64(_) => {
                    let right = stack
                        .pop()
                        .ok_or_else(|| error("Wasm constant operand underflow"))?;
                    let left = stack
                        .pop()
                        .ok_or_else(|| error("Wasm constant operand underflow"))?;
                    let value = match (operator, left, right) {
                        (
                            ConstantOperator::I32(op),
                            WasmValue::I32(left),
                            WasmValue::I32(right),
                        ) => op.apply(left, right),
                        (
                            ConstantOperator::I64(op),
                            WasmValue::I64(left),
                            WasmValue::I64(right),
                        ) => op.apply(left, right),
                        _ => return Err(error("Wasm constant operand type mismatch")),
                    };
                    value.map_err(|trap| Diagnostic::unsupported(name, trap.to_string()))?
                }
            };
            stack.push(value);
        }
        let value = stack
            .pop()
            .ok_or_else(|| error("empty Wasm constant expression"))?;
        if !stack.is_empty() || !value.fits_type(*expected) {
            return Err(error("invalid Wasm constant expression result"));
        }
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
                    expression.validate(name, globals)?;
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

    /// Static evaluation uses the same admitted expression and scalar operations.
    /// Unresolved imports remain absent facts, never placeholder values.
    pub fn evaluate_wasm_constant_expression<'a>(
        name: &str,
        expected: WasmType,
        globals: &[WasmGlobal],
        operators: impl IntoIterator<Item = Result<Operator<'a>, BinaryReaderError>>,
    ) -> Result<WasmValue, Diagnostic> {
        let expression = Self::lower_wasm_constant_expression(name, expected, globals, operators)?;
        if expected.requires_declarations()
            || globals.iter().enumerate().any(|(index, _)| {
                WasmGlobal::value_type(globals, index).is_some_and(WasmType::requires_declarations)
            })
        {
            return Err(Diagnostic::unsupported(
                name,
                "Wasm constant evaluation requires a declaration owner",
            ));
        }
        let mut values: Vec<Option<WasmValue>> = Vec::with_capacity(globals.len());
        for (index, global) in globals.iter().enumerate() {
            let value = match &global.initial {
                WasmGlobalInitializer::Import { .. } => None,
                WasmGlobalInitializer::Expression(expression) => {
                    expression.validate(name, &globals[..index])?;
                    // Type-correct admitted arithmetic cannot trap; absence means an unresolved global or function dependency.
                    expression
                        .evaluate(name, None, |dependency| {
                            let WasmInitializerEffect::Global(index) = dependency else {
                                return Err(Diagnostic::unsupported(
                                    name,
                                    "unresolved Wasm constant function dependency",
                                ));
                            };
                            values
                                .get(index as usize)
                                .copied()
                                .flatten()
                                .ok_or_else(|| {
                                    Diagnostic::unsupported(
                                        name,
                                        "unresolved Wasm constant global dependency",
                                    )
                                })
                        })
                        .ok()
                }
            };
            values.push(value);
        }
        expression.evaluate(name, None, |dependency| {
            let WasmInitializerEffect::Global(index) = dependency else {
                return Err(Diagnostic::unsupported(
                    name,
                    "unresolved Wasm constant function dependency",
                ));
            };
            values
                .get(index as usize)
                .copied()
                .flatten()
                .ok_or_else(|| {
                    Diagnostic::unsupported(name, "unresolved Wasm constant global dependency")
                })
        })
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
        assert!(function.produces_non_null_function());
        let concrete = WasmType::Reference {
            kind: super::super::WasmReferenceKind::DeclaredFunction {
                index: 0,
                exact: false,
            },
            nullable: true,
        };
        let error = Engine::evaluate_wasm_constant_expression(
            "ownerless-concrete",
            concrete,
            &[],
            [
                Operator::RefNull {
                    hty: wasmparser::HeapType::Concrete(wasmparser::UnpackedIndex::Module(0)),
                },
                Operator::End,
            ]
            .into_iter()
            .map(Ok),
        )
        .unwrap_err();
        assert!(error.to_string().contains("requires a declaration owner"));

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

        let signatures = super::super::WasmSignatures::with_imports(
            "function-facts",
            &[],
            [],
            &crate::WasmTypes::default(),
        )
        .unwrap();
        assert!(
            allocated
                .validate_functions("missing-struct-declaration", &signatures, &[])
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
            assert!(
                array
                    .validate_functions("missing-array-declaration", &signatures, &[])
                    .is_err()
            );
        }
        assert!(
            function
                .validate_functions("function-facts", &signatures, &[])
                .is_err()
        );
        assert!(
            Engine::evaluate_wasm_constant_expression(
                "function-facts",
                WasmType::FUNCREF,
                &[],
                [Operator::RefFunc { function_index: 0 }, Operator::End]
                    .into_iter()
                    .map(Ok),
            )
            .is_err()
        );
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
        assert!(
            Engine::evaluate_wasm_constant_expression(
                "initializer-facts",
                WasmType::I32,
                &globals,
                operators()
            )
            .is_err()
        );
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
