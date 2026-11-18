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

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DeclaredTypes {
    pub(crate) declared: Vec<Declared>,
    /// `(enum, member, value)` for each member that folds, in declaration order.
    pub(crate) enum_constants: Vec<(Box<str>, Box<str>, EnumConstant)>,
}

impl DeclaredTypes {
    pub(crate) fn declare(&mut self, kind: DeclaredKind, name: Option<&str>, span: oxc_span::Span) {
        self.declared.push(Declared { kind, name: name.map(Box::from), start: span.start, end: span.end });
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.declared.is_empty() && self.enum_constants.is_empty()
    }

    /// The folded value of `enumeration.member`, when the program declares one.
    #[cfg_attr(not(test), expect(dead_code, reason = "the fact store's enum folding reads it next"))]
    pub(crate) fn enum_constant(&self, enumeration: &str, member: &str) -> Option<&EnumConstant> {
        self.enum_constants.iter().rev()
            .find(|(name, key, _)| &**name == enumeration && &**key == member)
            .map(|(_, _, value)| value)
    }
}
