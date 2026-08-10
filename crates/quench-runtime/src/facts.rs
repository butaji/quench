//! Facts shared by frontend queries and residualization.

use oxc::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum Fact<T> {
    Proven(T),
    Guarded { value: T, guard: Guard },
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Guard {
    PlainObject,
    Number,
}

impl<T> Fact<T> {
    pub fn proven(value: T) -> Self {
        Self::Proven(value)
    }

    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Unknown)
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct ProgramDb {
    pub constants: Vec<ConstantFact>,
    pub scope_count: usize,
    pub symbol_count: usize,
    pub(crate) private_names: Vec<(Span, PrivateNameId)>,
    pub(crate) strict: bool,
    pub(crate) in_function: bool,
    pub(crate) tail_calls: bool,
    pub(crate) eval_var_barrier: Vec<String>,
    pub(crate) eval_deletable: Vec<(String, u16)>,
}

impl ProgramDb {
    pub fn insert_private_name(&mut self, span: Span, id: PrivateNameId) {
        if let Some((_, stored)) = self.private_names.iter_mut().find(|(key, _)| *key == span) {
            *stored = id;
        } else {
            self.private_names.push((span, id));
        }
    }

    pub fn private_name(&self, span: Span) -> Option<PrivateNameId> {
        self.private_names
            .iter()
            .find_map(|(key, id)| (*key == span).then_some(*id))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PrivateNameId(pub u32);

#[derive(Debug, Clone, PartialEq)]
pub struct ConstantFact {
    pub value: Constant,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    Number(f64),
    Boolean(bool),
    String(String),
    BigInt(String),
    Null,
    Undefined,
}
