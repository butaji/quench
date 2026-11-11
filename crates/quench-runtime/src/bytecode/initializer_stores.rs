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
    /// A fresh object of the constructor a capture and a property chain name, constructed from
    /// primitive constants: `this.color = new NS.Color(0, 0, 0)`.
    Construct(NestedConstruct),
}

/// The most properties a nested construction's constructor is read through.
pub(crate) const NESTED_CHAIN: usize = 2;
/// The most constant arguments a nested construction passes.
pub(crate) const NESTED_ARGUMENTS: usize = 4;

/// `new <capture>.<chain>(<constants>)`: the constructor read from the captured binding
/// `capture` (a `LoadCapture` immediate) through `chain[..depth]`, called with
/// `arguments[..count]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NestedConstruct {
    pub(crate) capture: u32,
    pub(crate) chain: [u32; NESTED_CHAIN],
    pub(crate) depth: u8,
    pub(crate) arguments: [InitializerConstant; NESTED_ARGUMENTS],
    pub(crate) count: u8,
}

impl InitializerStore {
    /// The parameter the store reads, if any.
    pub(crate) fn parameter(&self) -> Option<u16> {
        match self.value {
            InitializerValue::Parameter { parameter, .. } | InitializerValue::Numeric { parameter, .. } => Some(parameter),
            InitializerValue::Constant(_) | InitializerValue::Construct(_) => None,
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

/// The stores a nested construction adds to those of its body: the instructions from pc `at`
/// that read the constructor (`LoadCapture` and a chain of `GetField`), load the constant
/// arguments into the window, construct, and store the result into `this`; with how many
/// instructions they take. The registers they write are the body's temporaries, above its
/// parameters, which nothing else reads.
pub(super) fn nested_store(instructions: &[WideInstruction], at: usize, params: u16, constants: &[Constant]) -> Option<(InitializerStore, usize)> {
    let load = instructions.get(at).filter(|load| load.op() == Op::LoadCapture && load.result_register() >= params && load.result_flags() == 0)?;
    let mut register = load.result_register();
    let (mut chain, mut depth, mut pc) = ([0; NESTED_CHAIN], 0, at + 1);
    while let Some(read) = instructions.get(pc).filter(|read| read.op() == Op::GetField && read.register_b() == register
        && read.result_register() >= params && read.result_flags() == 0 && depth < NESTED_CHAIN) {
        chain[depth] = read.imm();
        register = read.result_register();
        depth += 1;
        pc += 1;
    }
    (depth > 0).then_some(())?;
    let mut arguments = [InitializerConstant::Undefined; NESTED_ARGUMENTS];
    let mut loaded: SmallVec<[Register; NESTED_ARGUMENTS]> = SmallVec::new();
    while let Some(constant) = instructions.get(pc).filter(|constant| constant.op() == Op::LoadConst && constant.result_register() >= params && constant.result_flags() == 0) {
        *arguments.get_mut(loaded.len())? = InitializerConstant::of(constants, constant.constant_index())?;
        loaded.push(constant.result_register());
        pc += 1;
    }
    let construct = instructions.get(pc).filter(|construct| construct.op() == Op::Construct && construct.register_b() == register
        && construct.result_flags() == 0 && !construct.returns_from_frame())?;
    let super::ConstructArguments::Registers(window) = construct.construct_arguments() else { return None };
    // The window is the constants just loaded, in order.
    (usize::from(window.count) == loaded.len()
        && loaded.iter().enumerate().all(|(index, loaded)| *loaded == window.base + index as Register))
        .then_some(())?;
    let set = instructions.get(pc + 1).filter(|set| set.stores_to_this() && set.register_a() == construct.result_register())?;
    let construct = NestedConstruct { capture: load.imm(), chain, depth: depth as u8, arguments, count: loaded.len() as u8 };
    Some((InitializerStore { atom: set.imm(), site: set.c(), value: InitializerValue::Construct(construct) }, pc + 2 - at))
}
