//! A typed program's declared environment: what its TypeScript declares beyond its
//! run-time code (`compile::typescript::admit`). The compiler writes it once; a program
//! keeps it with its code, so the fact store and every tier read the same declarations.
//! A declaration is a seed, not a proof: TypeScript is unsound.

use crate::typed_ir::ANY_LANGUAGES;
use super::Function;

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
    /// Language bits (`typed_ir::Languages`) the declared type admits for the field's value.
    pub(crate) languages: u16,
    /// Language bits the elements of an array-typed field's value admit (`DECLARED_ANY` where the
    /// type is not an array or its element type is open).
    pub(crate) elements: u16,
    /// `static` class member (a static field is not an instance field).
    pub(crate) is_static: bool,
}

/// The receiver layout a base class's construction reaches before any code can observe the
/// receiver (`compile::typescript::fields::layout`): the names of the leading run of the class's
/// instance fields that have no initializer, in order and without repeats. A base class's
/// [[Construct]] makes the receiver, then defines its instance fields first thing
/// (InitializeInstanceElements precedes the body and the parameter bindings), and a field with no
/// initializer defines `undefined` (CreateDataPropertyOrThrow on a fresh extensible ordinary
/// object, which cannot fail or run code). So until the first field outside this run, no code
/// runs and the receiver is unreachable but by the constructor: a receiver made already holding
/// these names, each `undefined`, in this order, is the receiver those defines make
/// (`Vm::preshaped_receiver_layout`). The run stops at the first instance field with an
/// initializer, a computed, private, string or numeric key, an `accessor`, a decorator, or the
/// name `__proto__`; a derived or decorated class has none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeclaredLayout {
    /// The class's source span start (as `DeclaredField::class_start`).
    pub(crate) class_start: u32,
    pub(crate) names: Box<[Box<str>]>,
}

/// A function's declared signature beyond its parameters (`Function::declared_parameters`).
/// Each set is a language set (`typed_ir::language_bit`), `DECLARED_ANY` where unknown.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DeclaredSignature {
    /// The function's source span start: the start its bytecode `FunctionHeader::source_span`
    /// begins at (a class constructor's is `class.span.start`, a method's the start after its
    /// `static` modifier).
    pub(crate) start: u32,
    /// The return type; an `async` function or generator returns an object.
    pub returns: u16,
    /// The element type of an annotated rest parameter.
    pub rest: u16,
    /// The `this` parameter's type.
    pub this: u16,
}

/// A declared local binding: its innermost enclosing function's start (`u32::MAX` at the top
/// level of a script or module) and its name.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DeclaredLocal {
    pub(crate) function: u32,
    pub(crate) name: Box<str>,
    /// Language bits the binding's annotation admits.
    pub(crate) languages: u16,
    /// Language bits the elements of an array-typed binding admit (`DECLARED_ANY` where the
    /// annotation is not an array or its element type is open).
    pub(crate) elements: u16,
}

/// A property a parameter's type literal declares (`function f(p: {x: number}) {}` declares
/// `x` of parameter 0 of `f`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeclaredMember {
    /// The function's source span start (as `DeclaredSignature::start`).
    pub(crate) function: u32,
    /// The parameter's index.
    pub(crate) parameter: u16,
    /// The property's name: a non-computed identifier key.
    pub(crate) name: Box<str>,
    /// Language bits the property's annotation admits.
    pub(crate) languages: u16,
}

/// A property a local binding's object annotation declares: `const p: Point = …` with `interface
/// Point { x: number }` declares `x` of local `p`. Keyed like `DeclaredLocal` (the function's start and
/// the local's name).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeclaredLocalMember {
    /// The innermost enclosing function's start (`u32::MAX` at the top level, as `DeclaredLocal::function`).
    pub(crate) function: u32,
    /// The local binding's name.
    pub(crate) local: Box<str>,
    /// The property's name: a non-computed identifier key.
    pub(crate) member: Box<str>,
    /// Language bits the property's annotation admits.
    pub(crate) languages: u16,
}

/// A parameter whose annotation names a class declared at the top level of the program: the
/// parameter is an instance of that class, whose declared fields (`DeclaredField`) describe it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DeclaredParameterClass {
    /// The function's source span start (as `DeclaredSignature::start`).
    pub function: u32,
    /// The parameter's index.
    pub parameter: u16,
    /// The class's source span start (as `DeclaredField::class_start`).
    pub class: u32,
}

/// A parameter whose annotation the program's own type names resolve to a different language set
/// than the name-free reading gives (`type N = number; function f(a: N) {}`). Its set narrows the
/// function's `declared_parameters` entry once the program is compiled (`DeclaredTypes::apply_parameters`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DeclaredParameter {
    /// The function's source span start (as `DeclaredSignature::start`).
    pub(crate) function: u32,
    /// The parameter's index.
    pub(crate) parameter: u16,
    /// Language bits the parameter's type admits, with the program's type names resolved.
    pub(crate) languages: u16,
}

/// A type-level name the program declares at its top level, resolved when the program loads, owned
/// so a body compiled later resolves its annotations against it (`TypeNames::resolved`). Entries are
/// sorted by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SavedTypeName {
    pub(crate) name: Box<str>,
    /// The language bits the name denotes; `DECLARED_ANY` where it names no closed set.
    pub(crate) languages: u16,
    /// An alias: with type arguments, its expansion is open.
    pub(crate) alias: bool,
    /// The class's source span start, for a class.
    pub(crate) class: Option<u32>,
    /// The property members an interface or object alias resolves to (`TypeNames::members_of`), so a
    /// body compiled later keeps the member facts the eager path gives.
    pub(crate) members: Option<Box<[(Box<str>, u16)]>>,
}

/// The `DeclaredLocal::function` of top-level code.
const TOP_LEVEL: u32 = u32::MAX;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DeclaredTypes {
    pub(crate) declared: Vec<Declared>,
    /// `(enum, member, value)` for each member that folds, in declaration order.
    pub(crate) enum_constants: Vec<(Box<str>, Box<str>, EnumConstant)>,
    /// The declared class fields (instance and static), in declaration order.
    pub(crate) fields: Vec<DeclaredField>,
    /// The declared signatures of functions, in source order.
    pub(crate) signatures: Vec<DeclaredSignature>,
    /// The declared local bindings, in source order.
    pub(crate) locals: Vec<DeclaredLocal>,
    /// The declared properties of parameters' type literals, in source order.
    pub(crate) members: Vec<DeclaredMember>,
    /// The declared properties of locals whose object annotation names them, in source order.
    pub(crate) local_members: Vec<DeclaredLocalMember>,
    /// The parameters whose annotation names a top-level class, in source order.
    pub parameter_classes: Vec<DeclaredParameterClass>,
    /// The parameters whose named-resolved language sets differ from their name-free ones, in source order.
    pub(crate) parameters: Vec<DeclaredParameter>,
    /// The receiver layouts of base classes (`DeclaredLayout`), in source order.
    pub(crate) layouts: Vec<DeclaredLayout>,
    /// The program's top-level type names (sorted), which a lazily parsed body's annotations resolve
    /// against; `None` where the program declares none saved (a JavaScript program, or a chunk's own).
    pub(crate) type_names: Option<std::rc::Rc<[SavedTypeName]>>,
}

impl DeclaredTypes {
    pub(crate) fn declare(&mut self, kind: DeclaredKind, name: Option<&str>, span: oxc_span::Span) {
        self.declared.push(Declared { kind, name: name.map(Box::from), start: span.start, end: span.end });
    }

    pub(crate) fn declare_field(&mut self, class_start: u32, name: &str, languages: u16, elements: u16, is_static: bool) {
        self.fields.push(DeclaredField { class_start, name: Box::from(name), languages, elements, is_static });
    }

    pub(crate) fn declare_signature(&mut self, signature: DeclaredSignature) {
        self.signatures.push(signature);
    }

    pub(crate) fn declare_local(&mut self, function: u32, name: &str, languages: u16, elements: u16) {
        self.locals.push(DeclaredLocal { function, name: Box::from(name), languages, elements });
    }

    pub(crate) fn declare_member(&mut self, function: u32, parameter: u16, name: &str, languages: u16) {
        self.members.push(DeclaredMember { function, parameter, name: Box::from(name), languages });
    }

    pub(crate) fn declare_local_member(&mut self, function: u32, local: &str, member: &str, languages: u16) {
        self.local_members.push(DeclaredLocalMember { function, local: Box::from(local), member: Box::from(member), languages });
    }

    pub(crate) fn declare_parameter_class(&mut self, function: u32, parameter: u16, class: u32) {
        self.parameter_classes.push(DeclaredParameterClass { function, parameter, class });
    }

    pub(crate) fn declare_parameter(&mut self, function: u32, parameter: u16, languages: u16) {
        self.parameters.push(DeclaredParameter { function, parameter, languages });
    }

    pub(crate) fn declare_layout(&mut self, class_start: u32, names: Vec<Box<str>>) {
        self.layouts.push(DeclaredLayout { class_start, names: names.into_boxed_slice() });
    }

    /// Moves `other`'s declarations after these, each kind in its own order.
    pub(crate) fn append(&mut self, other: DeclaredTypes) {
        self.declared.extend(other.declared);
        self.enum_constants.extend(other.enum_constants);
        self.fields.extend(other.fields);
        self.signatures.extend(other.signatures);
        self.locals.extend(other.locals);
        self.members.extend(other.members);
        self.parameter_classes.extend(other.parameter_classes);
        self.parameters.extend(other.parameters);
        self.layouts.extend(other.layouts);
    }

    /// The declarations keyed inside `start..end` (a function's source range, its nested functions and
    /// classes included), with their positions unchanged: what a first-call chunk of that function
    /// compiles against, as the chunk's positions are the program's. Enum constants are not positional
    /// and are left out.
    pub(crate) fn within(&self, start: u32, end: u32) -> DeclaredTypes {
        let inside = |position: u32| (start..end).contains(&position);
        DeclaredTypes {
            type_names: None,
            declared: self.declared.iter().filter(|entry| inside(entry.start)).cloned().collect(),
            enum_constants: Vec::new(),
            fields: self.fields.iter().filter(|field| inside(field.class_start)).cloned().collect(),
            signatures: self.signatures.iter().filter(|signature| inside(signature.start)).cloned().collect(),
            locals: self.locals.iter().filter(|local| inside(local.function)).cloned().collect(),
            local_members: self.local_members.iter().filter(|member| inside(member.function)).cloned().collect(),
            members: self.members.iter().filter(|member| inside(member.function)).cloned().collect(),
            parameter_classes: self.parameter_classes.iter().filter(|entry| inside(entry.function)).copied().collect(),
            parameters: self.parameters.iter().filter(|entry| inside(entry.function)).copied().collect(),
            layouts: self.layouts.iter().filter(|layout| inside(layout.class_start)).cloned().collect(),
        }
    }

    /// Whether any declaration is recorded (the saved names aside).
    pub(crate) fn has_declarations(&self) -> bool {
        !self.declared.is_empty() || !self.enum_constants.is_empty() || !self.fields.is_empty()
            || !self.signatures.is_empty() || !self.locals.is_empty() || !self.members.is_empty() || !self.local_members.is_empty()
            || !self.parameter_classes.is_empty() || !self.parameters.is_empty() || !self.layouts.is_empty()
    }

    pub(crate) fn is_empty(&self) -> bool {
        !self.has_declarations() && self.type_names.is_none()
    }

    /// Moves every position of these declarations by `delta`: a body parsed alone, whose positions
    /// are its text's, takes the program's (`ShiftSpans`). The saved names have none.
    pub(crate) fn shift(&mut self, delta: u32) {
        let at = |position: &mut u32| *position = position.wrapping_add(delta);
        let at_function = |function: &mut u32| if *function != TOP_LEVEL { *function = function.wrapping_add(delta) };
        for entry in &mut self.declared {
            at(&mut entry.start);
            at(&mut entry.end);
        }
        for signature in &mut self.signatures {
            at(&mut signature.start);
        }
        for field in &mut self.fields {
            at(&mut field.class_start);
        }
        for layout in &mut self.layouts {
            at(&mut layout.class_start);
        }
        for local in &mut self.locals {
            at_function(&mut local.function);
        }
        for member in &mut self.members {
            at(&mut member.function);
        }
        for entry in &mut self.parameter_classes {
            at(&mut entry.function);
            at(&mut entry.class);
        }
        for entry in &mut self.parameters {
            at(&mut entry.function);
        }
        for member in &mut self.local_members {
            at_function(&mut member.function);
        }
    }

    /// Narrows the declared parameter sets of the functions in `functions` with the named-resolved
    /// sets recorded here, matched by each function's source span start (`DeclaredParameter::function`).
    /// The recorded set is intersected with the function's current set; where they are disjoint the
    /// recorded set stands, as a shadowing type name's does.
    pub(crate) fn apply_parameters(&self, functions: &mut [Option<Function>]) {
        if self.parameters.is_empty() {
            return;
        }
        let mut starts: Vec<(u32, usize)> = functions.iter().enumerate()
            .filter_map(|(id, function)| Some((function.as_ref()?.source_span?.start, id)))
            .collect();
        starts.sort_unstable();
        for entry in &self.parameters {
            let Ok(found) = starts.binary_search_by_key(&entry.function, |&(start, _)| start) else { continue };
            let Some(function) = functions[starts[found].1].as_mut() else { continue };
            let mut languages = function.declared_parameters().to_vec();
            let slot = usize::from(entry.parameter);
            if languages.len() <= slot {
                languages.resize(slot + 1, ANY_LANGUAGES);
            }
            let merged = entry.languages & languages[slot];
            languages[slot] = if merged == 0 { entry.languages } else { merged };
            function.set_declared_parameters(languages.into());
        }
    }

    /// The receiver layout (`DeclaredLayout`) of the base class starting at `class_start`, when
    /// the program declares one.
    pub(crate) fn declared_layout(&self, class_start: u32) -> Option<&[Box<str>]> {
        self.layouts.iter().rev().find(|layout| layout.class_start == class_start).map(|layout| &*layout.names)
    }

    /// The folded value of `enumeration.member`, when the program declares one.
    #[cfg_attr(not(test), expect(dead_code, reason = "the fact store's enum folding reads it next"))]
    pub(crate) fn enum_constant(&self, enumeration: &str, member: &str) -> Option<&EnumConstant> {
        self.enum_constants.iter().rev()
            .find(|(name, key, _)| &**name == enumeration && &**key == member)
            .map(|(_, _, value)| value)
    }

    /// The declared signature of the function whose source span starts at `start`, when declared.
    pub fn declared_signature(&self, start: u32) -> Option<&DeclaredSignature> {
        self.signatures.iter().rev().find(|signature| signature.start == start)
    }

    /// The declared language types of local `name` of the function starting at `function`
    /// (`u32::MAX` for the top level), when the program declares one (the last declaration wins).
    pub fn declared_local(&self, function: u32, name: &str) -> Option<u16> {
        self.locals.iter().rev()
            .find(|local| local.function == function && &*local.name == name && local.languages != ANY_LANGUAGES)
            .map(|local| local.languages)
    }

    /// The declared element language types of local `name` of the function starting at `function`
    /// (`u32::MAX` for the top level), when its annotation is an array type with a closed element
    /// set (the last declaration wins).
    pub(crate) fn declared_local_elements(&self, function: u32, name: &str) -> Option<u16> {
        self.locals.iter().rev()
            .find(|local| local.function == function && &*local.name == name && local.elements != ANY_LANGUAGES)
            .map(|local| local.elements)
    }

    /// The declared language types of instance field `name` of the class starting at
    /// `class_start`, when the program declares one (the last declaration wins).
    pub fn declared_field(&self, class_start: u32, name: &str) -> Option<u16> {
        self.fields.iter().rev()
            .find(|field| !field.is_static && field.class_start == class_start && &*field.name == name
                && field.languages != ANY_LANGUAGES)
            .map(|field| field.languages)
    }

    /// The declared element language types of instance field `name` of the class starting at
    /// `class_start`, when its annotation is an array type with a closed element set (the last
    /// declaration wins).
    #[cfg_attr(not(test), allow(dead_code, reason = "the object model reads array element types next"))]
    pub(crate) fn declared_field_elements(&self, class_start: u32, name: &str) -> Option<u16> {
        self.fields.iter().rev()
            .find(|field| !field.is_static && field.class_start == class_start && &*field.name == name
                && field.elements != ANY_LANGUAGES)
            .map(|field| field.elements)
    }

    /// The declared language types of property `name` of parameter `parameter` of the function
    /// starting at `function`, from its type literal, when the program declares one (the last
    /// declaration wins).
    #[cfg_attr(not(test), allow(dead_code, reason = "the fact store reads parameter members next"))]
    pub fn declared_member(&self, function: u32, parameter: u16, name: &str) -> Option<u16> {
        self.members.iter().rev()
            .find(|member| member.function == function && member.parameter == parameter && &*member.name == name)
            .map(|member| member.languages)
    }

    /// The class (its span start) that parameter `parameter` of the function starting at
    /// `function` is declared an instance of, when the program declares one (the last declaration wins).
    #[cfg_attr(not(test), allow(dead_code, reason = "the fact store reads parameter classes next"))]
    pub fn declared_parameter_class(&self, function: u32, parameter: u16) -> Option<u32> {
        self.parameter_classes.iter().rev()
            .find(|entry| entry.function == function && entry.parameter == parameter)
            .map(|entry| entry.class)
    }

    /// The declared language types of property `member` of local `local` of the function starting at
    /// `function` (`u32::MAX` for the top level), from its object annotation, when the program declares
    /// one (the last declaration wins).
    #[cfg_attr(not(test), allow(dead_code, reason = "the object model seeds local member reads next"))]
    pub(crate) fn declared_local_member(&self, function: u32, local: &str, member: &str) -> Option<u16> {
        self.local_members.iter().rev()
            .find(|entry| entry.function == function && &*entry.local == local && &*entry.member == member)
            .map(|entry| entry.languages)
    }

    /// The named-resolved language types of parameter `parameter` of the function starting at
    /// `function`, when they differ from the name-free reading (the last declaration wins).
    #[cfg_attr(not(test), allow(dead_code, reason = "the parameter table is read by the tests and the function's declared_parameters"))]
    pub(crate) fn declared_parameter(&self, function: u32, parameter: u16) -> Option<u16> {
        self.parameters.iter().rev()
            .find(|entry| entry.function == function && entry.parameter == parameter)
            .map(|entry| entry.languages)
    }
}
