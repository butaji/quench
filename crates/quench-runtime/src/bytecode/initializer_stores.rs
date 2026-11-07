//! The stores of a constructor body that only initializes `this` (`Function::initializer_stores`).
use super::{Constant, Op, OperandKind, Register, WideInstruction};
use smallvec::SmallVec;

/// A store of an initializing body (`Function::initializer_stores`): the field `atom`, at
/// cache site `site`, takes `value`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct InitializerStore {
    pub(crate) atom: u32,
    pub(crate) site: u16,
    pub(crate) value: InitializerValue,
}

/// What an initializer store gives its field.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum InitializerValue {
    /// Parameter `parameter` as passed, or the Number `fallback` where it is falsy.
    Parameter { parameter: u16, fallback: Option<f64> },
    /// A primitive constant, whatever is passed: `this.id = 0`, `this.parent = void 0`.
    Constant(InitializerConstant),
    /// Numeric binary `operator` of parameter `parameter`, when passed a Number, and the
    /// Number `constant`: `this.y = x + 1`. Any other argument leaves the stores to the body.
    Numeric { parameter: u16, operator: u8, constant: f64 },
}

impl InitializerStore {
    /// The parameter the store reads, if any.
    pub(crate) fn parameter(&self) -> Option<u16> {
        match self.value {
            InitializerValue::Parameter { parameter, .. } | InitializerValue::Numeric { parameter, .. } => Some(parameter),
            InitializerValue::Constant(_) => None,
        }
    }
}

/// The constants an initializer store writes: those whose value needs no heap cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum InitializerConstant {
    Number(f64),
    Boolean(bool),
    Null,
    Undefined,
    /// A string literal: the program's constant at this index, the value its `LoadConst`
    /// yields (`this.type = ""`).
    String(u32),
}

impl InitializerConstant {
    pub(super) fn of(constants: &[Constant], index: usize) -> Option<Self> {
        Some(match constants.get(index)? {
            Constant::Number(number) => Self::Number(*number),
            Constant::Boolean(boolean) => Self::Boolean(*boolean),
            Constant::Null => Self::Null,
            Constant::Undefined => Self::Undefined,
            Constant::String(_) | Constant::StringUnits(_) => Self::String(u32::try_from(index).ok()?),
            _ => return None,
        })
    }

    /// The value the constant stores, a string literal's from the running program's
    /// materialized constants (`program_constant`).
    pub(crate) fn value(self, program_constant: impl FnOnce(usize) -> Option<crate::value::Value>) -> Option<crate::value::Value> {
        Some(match self {
            Self::Number(number) => crate::value::Value::number(number),
            Self::Boolean(boolean) => crate::value::Value::boolean(boolean),
            Self::Null => crate::value::Value::NULL,
            Self::Undefined => crate::value::Value::UNDEFINED,
            Self::String(index) => return program_constant(index as usize),
        })
    }
}

/// The temporaries an initializing body wrote, latest last: a primitive constant's
/// `LoadConst`, or a numeric `Binary` of a parameter nothing defaults and a Number constant.
#[derive(Default)]
pub(super) struct Temporaries(SmallVec<[(Register, InitializerValue); 4]>);

impl Temporaries {
    /// What temporary `register` holds, if one of these wrote it last.
    pub(super) fn value(&self, register: Register) -> Option<InitializerValue> {
        self.0.iter().rev().find(|(written, _)| *written == register).map(|(_, value)| *value)
    }

    /// Records `instruction` when it writes a temporary these model, which writes nothing
    /// else; `false` for any other instruction. `fallbacks` are the parameters' defaults.
    pub(super) fn write(&mut self, instruction: &WideInstruction, params: u16, fallbacks: &[Option<f64>], constants: &[Constant]) -> bool {
        if !matches!(instruction.op(), Op::LoadConst | Op::Binary) || instruction.result_flags() != 0 || instruction.result_register() < params {
            return false;
        }
        let value = match instruction.op() {
            Op::LoadConst => InitializerConstant::of(constants, instruction.constant_index()).map(InitializerValue::Constant),
            Op::Binary => Self::numeric(instruction, params, fallbacks, constants),
            _ => None,
        };
        value.map(|value| self.0.push((instruction.result_register(), value))).is_some()
    }

    /// `p op k` of a `Binary`: a numeric operator of a parameter without default and a
    /// Number constant.
    fn numeric(binary: &WideInstruction, params: u16, fallbacks: &[Option<f64>], constants: &[Constant]) -> Option<InitializerValue> {
        let parameter = binary.operand_b().register_index().filter(|p| *p < params && fallbacks[usize::from(*p)].is_none())?;
        let operand = binary.operand_c();
        let Some(Constant::Number(constant)) = constants.get(usize::from(operand.payload())).filter(|_| operand.kind() == Some(OperandKind::Constant)) else {
            return None;
        };
        let operator = binary.binary_operator();
        use oxc_syntax::operator::BinaryOperator;
        (BinaryOperator::Addition as u32..=BinaryOperator::BitwiseAnd as u32).contains(&operator)
            .then_some(InitializerValue::Numeric { parameter, operator: operator as u8, constant: *constant })
    }
}
