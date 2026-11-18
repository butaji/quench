//! A typed program's declared environment: what its TypeScript declares beyond its
//! run-time code (`compile::typescript::admit`). The compiler writes it once; a program
//! keeps it with its code, so the fact store and every tier read the same declarations.
//! A declaration is a seed, not a proof: TypeScript is unsound.

/// What a declaration of the declared environment is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum DeclaredKind {
    Interface = 0,
    TypeAlias = 1,
    /// An overload signature or a `declare function`.
    FunctionSignature = 2,
    Ambient = 3,
    /// A namespace with no run-time content.
    Namespace = 4,
    /// A type-only import or export.
    ModuleType = 5,
    /// An abstract or signature-only class member, a `declare` field or an index signature.
    ClassMember = 6,
    /// A run-time enum, whose folded members are `DeclaredTypes::enum_constants`.
    Enum = 7,
}

impl DeclaredKind {
    const ALL: [Self; 8] = [Self::Interface, Self::TypeAlias, Self::FunctionSignature, Self::Ambient,
        Self::Namespace, Self::ModuleType, Self::ClassMember, Self::Enum];

    pub(crate) fn from_binary_tag(tag: u8) -> Option<Self> {
        Self::ALL.get(usize::from(tag)).copied()
    }
}

/// A declared name of the typed program, by source range.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Declared {
    pub(crate) kind: DeclaredKind,
    pub(crate) name: Option<Box<str>>,
    pub(crate) start: u32,
    pub(crate) end: u32,
}

/// An enum member's folded value: a constant every read of it may use.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum EnumConstant {
    Number(f64),
    String(Box<str>),
}

/// A declared class field: the language types its TypeScript type admits (a bit per
/// `ValueType`, `compile::typescript::DECLARED_ANY` where it names no closed set).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DeclaredField {
    /// The class's source span start: `class.span.start` of the class declaring the field.
    /// The class's explicit constructor `Function` source span starts at the same offset,
    /// and so does an implicit constructor's (both come from `class.span` in `compile::class`).
    pub(crate) class_start: u32,
    /// The field's name: a non-computed identifier key, or a constructor parameter property's
    /// parameter name. Private `#name` keys are not recorded.
    pub(crate) name: Box<str>,
    /// Language bits (`1 << ValueType as u16`) the declared type admits for the field's value.
    pub(crate) languages: u16,
    /// `static` class member (a static field is not an instance field).
    pub(crate) is_static: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DeclaredTypes {
    pub(crate) declared: Vec<Declared>,
    /// `(enum, member, value)` for each member that folds, in declaration order.
    pub(crate) enum_constants: Vec<(Box<str>, Box<str>, EnumConstant)>,
    /// The declared class fields (instance and static), in declaration order.
    pub(crate) fields: Vec<DeclaredField>,
}

impl DeclaredTypes {
    pub(crate) fn declare(&mut self, kind: DeclaredKind, name: Option<&str>, span: oxc_span::Span) {
        self.declared.push(Declared { kind, name: name.map(Box::from), start: span.start, end: span.end });
    }

    pub(crate) fn declare_field(&mut self, class_start: u32, name: &str, languages: u16, is_static: bool) {
        self.fields.push(DeclaredField { class_start, name: Box::from(name), languages, is_static });
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.declared.is_empty() && self.enum_constants.is_empty() && self.fields.is_empty()
    }

    /// The folded value of `enumeration.member`, when the program declares one.
    #[cfg_attr(not(test), expect(dead_code, reason = "the fact store's enum folding reads it next"))]
    pub(crate) fn enum_constant(&self, enumeration: &str, member: &str) -> Option<&EnumConstant> {
        self.enum_constants.iter().rev()
            .find(|(name, key, _)| &**name == enumeration && &**key == member)
            .map(|(_, _, value)| value)
    }

    /// The declared language types of instance field `name` of the class starting at
    /// `class_start`, when the program declares one (the last declaration wins).
    pub(crate) fn declared_field(&self, class_start: u32, name: &str) -> Option<u16> {
        self.fields.iter().rev()
            .find(|field| !field.is_static && field.class_start == class_start && &*field.name == name)
            .map(|field| field.languages)
    }
}
