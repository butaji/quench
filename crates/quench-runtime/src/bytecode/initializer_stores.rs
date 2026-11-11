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
    /// Whether the field is defined (a class field's `DefineField`, CreateDataProperty or
    /// PrivateFieldAdd), which consults no prototype, rather than assigned.
    pub(crate) define: bool,
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
    /// Forgets what `register` held: another instruction wrote it.
    pub(super) fn forget(&mut self, register: Register) {
        self.0.retain(|(written, _)| *written != register);
    }

    /// The value of `register`: a parameter (with its default), else a temporary's.
    pub(super) fn operand(&self, register: Register, params: Register, fallbacks: &[Option<f64>]) -> Option<InitializerValue> {
        match register < params {
            true => Some(InitializerValue::Parameter { parameter: register, fallback: fallbacks[usize::from(register)] }),
            false => self.value(register),
        }
    }

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

impl InitializerStore {
    /// The assignment `set` (a `SetThisField`, or a `SetField` of `this` in a temporary)
    /// makes of `value`.
    pub(super) fn assigned(set: &WideInstruction, value: InitializerValue) -> Self {
        Self { atom: set.imm(), site: set.c(), value, define: false }
    }
}

/// Adds `store` to `stores`; a store to a field an earlier store made writes its value in that
/// field's place: an own writable data property's assignment, which nothing observes between.
pub(super) fn push_store(stores: &mut Vec<InitializerStore>, store: InitializerStore) {
    match stores.iter_mut().find(|stored| stored.atom == store.atom) {
        Some(stored) => stored.value = store.value,
        None => stores.push(store),
    }
}

/// Whether `stores` make a construct plan: some, and either all defines or none, so a plan
/// never repeats a define the body's activation would make again (`lane_add_stores`).
pub(super) fn plannable(stores: &[InitializerStore]) -> bool {
    stores.first().is_some_and(|first| stores.iter().all(|store| store.define == first.define))
}

/// The class field initialization of a base class constructor: `this` in temporaries
/// (`LoadThis`), each field's define on it (`DefineField`, CreateDataProperty of a fresh
/// receiver's absent field or PrivateFieldAdd, which consult no prototype), a defined private
/// field's brand check (`CheckPrivate`) and the stores to defined fields through it.
#[derive(Default)]
pub(super) struct ClassFields {
    this: SmallVec<[Register; 2]>,
}

impl ClassFields {
    /// Records `instruction` when it is one of these or a temporary's write
    /// (`Temporaries::write`), which writes nothing else; `false` for any other instruction.
    pub(super) fn step(&mut self, instruction: &WideInstruction, params: Register, fallbacks: &[Option<f64>], constants: &[Constant],
        temporaries: &mut Temporaries, stores: &mut Vec<InitializerStore>) -> bool {
        if let Some(recorded) = self.field_step(instruction, params, fallbacks, temporaries, stores) {
            return recorded;
        }
        !self.this.contains(&instruction.result_register()) && temporaries.write(instruction, params, fallbacks, constants)
    }

    /// `step` of a `this` load, or of a define, brand check or store through `this`; `None`
    /// for any other instruction.
    fn field_step(&mut self, instruction: &WideInstruction, params: Register, fallbacks: &[Option<f64>],
        temporaries: &mut Temporaries, stores: &mut Vec<InitializerStore>) -> Option<bool> {
        let value = |register: Register| temporaries.operand(register, params, fallbacks);
        let this = |register: Register| self.this.contains(&register);
        let defined = |atom: u32| stores.iter().any(|stored| stored.define && stored.atom == atom);
        let store = match instruction.op() {
            Op::LoadThis if instruction.result_flags() == 0 && instruction.result_register() >= params => {
                temporaries.forget(instruction.result_register());
                self.this.push(instruction.result_register());
                return Some(true);
            }
            Op::CheckPrivate => return Some(this(instruction.register_a()) && defined(instruction.imm())),
            Op::DefineField if this(instruction.register_b()) && !stores.iter().any(|stored| stored.atom == instruction.imm()) => {
                value(instruction.register_a()).map(|value| InitializerStore { atom: instruction.imm(), site: instruction.c(), value, define: true })
            }
            Op::SetField | Op::SetFieldStrict if this(instruction.register_b()) && defined(instruction.imm()) => {
                value(instruction.register_a()).map(|value| InitializerStore::assigned(instruction, value))
            }
            _ => return None,
        };
        Some(store.map(|store| push_store(stores, store)).is_some())
    }
}
