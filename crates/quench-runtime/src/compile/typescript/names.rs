//! The type-level names of a TypeScript program (`TypeNames`): what each named type annotation
//! denotes. A name the program declares at its top level denotes that declaration; a name declared
//! twice, or bound anywhere else at a type level (an import, a nested declaration, a type
//! parameter), denotes nothing this pass knows, so its annotation stays open (`DECLARED_ANY`).

use std::rc::Rc;

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_syntax::operator::{BinaryOperator, UnaryOperator};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::ValueType as V;
use crate::bytecode::SavedTypeName;
use crate::typed_ir::language_bit;
use super::{DECLARED_ANY, declared_language_in};

/// The property members an object type declares, each with the language types its annotation admits
/// (`DECLARED_ANY` where open), in first-declaration order. A name is listed even where open, so a
/// declaration that overrides an inherited one replaces it.
pub(in crate::compile) type Members = Vec<(Box<str>, u16)>;

/// How many named links (an alias or an interface reached by name) a member lookup may follow.
const MAX_MEMBER_DEPTH: u8 = 8;

/// What a type-level name denotes.
#[derive(Clone, Copy)]
pub(in crate::compile) enum Named<'a> {
    /// A type alias, by its type annotation. An alias with type parameters is `open`: its body
    /// depends on the arguments, so no expansion is a fact.
    Alias { annotation: &'a TSType<'a>, open: bool },
    /// An interface: an object or a function (a primitive satisfies it only structurally).
    Interface,
    /// A class, by its span start (the key `DeclaredField::class_start` uses): an object.
    Class(u32),
    /// An enum, by the language types its members hold.
    Enum(u16),
    /// A top-level name of the program resolved when it loaded (`TypeNames::resolved`), by its language
    /// bits, whether it is an alias (open with type arguments), and the class's span start.
    Resolved { languages: u16, alias: bool, class: Option<u32> },
    /// Denotes nothing this pass knows.
    Unknown,
}

/// The type-level names of one program, by name: those its own code declares here, and the program's
/// saved top-level names (`saved`, sorted), which a declaration here shadows.
#[derive(Default)]
pub(in crate::compile) struct TypeNames<'a> {
    names: FxHashMap<String, Named<'a>>,
    /// The top-level declarations of each interface name, in source order: two blocks of one name merge.
    interfaces: FxHashMap<String, Vec<&'a TSInterfaceDeclaration<'a>>>,
    saved: Option<Rc<[SavedTypeName]>>,
}

impl<'a> TypeNames<'a> {
    /// The saved names alone: what a body compiled later resolves against (`resolved`).
    pub(in crate::compile) fn from_saved(saved: Option<Rc<[SavedTypeName]>>) -> Self {
        Self { names: FxHashMap::default(), interfaces: FxHashMap::default(), saved }
    }

    /// The names `program` declares at its top level, over `saved`, then every other type-level binding
    /// made unknown: a nested declaration or a type parameter shadows the name it repeats.
    pub(in crate::compile) fn of(program: &'a Program<'a>, saved: Option<Rc<[SavedTypeName]>>) -> Self {
        let mut names = Self::from_saved(saved);
        let mut top = FxHashSet::default();
        for statement in &program.body {
            names.top_level(statement, &mut top);
        }
        let mut shadowing = Shadowing { names: &mut names, top: &top };
        shadowing.visit_program(program);
        names
    }

    /// What `name` denotes in this program, if the program declares it.
    pub(in crate::compile) fn get(&self, name: &str) -> Option<Named<'a>> {
        if let Some(named) = self.names.get(name) {
            return Some(*named);
        }
        let saved = self.saved.as_deref()?;
        let at = saved.binary_search_by(|entry| (*entry.name).cmp(name)).ok()?;
        let entry = &saved[at];
        Some(Named::Resolved { languages: entry.languages, alias: entry.alias, class: entry.class })
    }

    /// These names, as the program saves them: each name it denotes here, resolved to its language
    /// bits with these names (an alias's expansion included), over the saved names it does not
    /// redeclare. Sorted by name.
    pub(in crate::compile) fn resolved(&self) -> Rc<[SavedTypeName]> {
        let mut entries: Vec<SavedTypeName> = self.names.iter().map(|(name, named)| SavedTypeName {
            name: Box::from(name.as_str()),
            languages: match *named {
                Named::Alias { annotation, open } => if open { super::DECLARED_ANY } else { super::languages(annotation, self, 0) },
                Named::Interface => language_bit(V::Object) | language_bit(V::Function),
                Named::Class(_) => language_bit(V::Object),
                Named::Enum(bits) | Named::Resolved { languages: bits, .. } => bits,
                Named::Unknown => super::DECLARED_ANY,
            },
            alias: matches!(named, Named::Alias { .. } | Named::Resolved { alias: true, .. }),
            class: match *named {
                Named::Class(class) | Named::Resolved { class: Some(class), .. } => Some(class),
                _ => None,
            },
            members: self.named_members(name, 0, &mut Vec::new()).map(Vec::into_boxed_slice),
        }).collect();
        if let Some(saved) = &self.saved {
            entries.extend(saved.iter().filter(|entry| !self.names.contains_key(&*entry.name)).cloned());
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries.dedup_by(|a, b| a.name == b.name);
        entries.into()
    }

    /// The property members an object-shaped annotation declares: a type literal, a name of an interface
    /// or alias this program declares, or an intersection or parentheses of these. `None` where the
    /// annotation is not object-shaped by these names (a union, a primitive, a builtin, an open name).
    /// A generic member typed by a type parameter is open, since the parameter is an `Unknown` name.
    pub(in crate::compile) fn members_of(&self, ty: &TSType<'_>) -> Option<Members> {
        self.type_members(ty, 0, &mut Vec::new())
    }

    /// The members of the object type `ty` denotes, following named links at `depth`; `path` holds
    /// the names being resolved, so a cycle ends.
    fn type_members(&self, ty: &TSType<'_>, depth: u8, path: &mut Vec<String>) -> Option<Members> {
        match ty {
            TSType::TSTypeLiteral(literal) => {
                let mut members = Members::new();
                for member in &literal.members {
                    if let Some(entry) = self.signature_member(member) {
                        merge(&mut members, entry);
                    }
                }
                Some(members)
            }
            TSType::TSTypeReference(reference) => match &reference.type_name {
                TSTypeName::IdentifierReference(name) => self.named_members(name.name.as_str(), depth, path),
                _ => None,
            },
            TSType::TSParenthesizedType(inner) => self.type_members(&inner.type_annotation, depth, path),
            // A value of `A & B` has each part's members, so every known part's fact holds; parts that
            // disagree on a name are open.
            TSType::TSIntersectionType(intersection) => {
                let mut members: Option<Members> = None;
                for part in &intersection.types {
                    if let Some(part) = self.type_members(part, depth, path) {
                        let accumulated = members.get_or_insert_with(Members::new);
                        for entry in part {
                            merge(accumulated, entry);
                        }
                    }
                }
                members
            }
            _ => None,
        }
    }

    /// The members of the interface or alias `name`, where the program declares it as one.
    fn named_members(&self, name: &str, depth: u8, path: &mut Vec<String>) -> Option<Members> {
        if depth >= MAX_MEMBER_DEPTH || path.iter().any(|seen| seen.as_str() == name) {
            return None;
        }
        path.push(name.to_string());
        let members = match self.get(name)? {
            Named::Interface => self.interface_members(name, depth, path),
            Named::Alias { annotation, .. } => self.type_members(annotation, depth + 1, path),
            Named::Resolved { .. } => self.saved_members(name),
            Named::Class(_) | Named::Enum(_) | Named::Unknown => None,
        };
        path.pop();
        members
    }

    /// The members the program saved for its top-level name `name` when it loaded.
    fn saved_members(&self, name: &str) -> Option<Members> {
        let saved = self.saved.as_deref()?;
        let at = saved.binary_search_by(|entry| (*entry.name).cmp(name)).ok()?;
        saved[at].members.as_deref().map(<[_]>::to_vec)
    }

    /// The members of the interface `name`: its own blocks' members, which override each `extends`
    /// parent's (a parent the program does not declare contributes nothing, and the own members stand).
    fn interface_members(&self, name: &str, depth: u8, path: &mut Vec<String>) -> Option<Members> {
        let declarations = self.interfaces.get(name)?;
        let mut own = Members::new();
        let mut inherited = Members::new();
        for declaration in declarations {
            for member in &declaration.body.body {
                if let Some(entry) = self.signature_member(member) {
                    merge(&mut own, entry);
                }
            }
            for heritage in &declaration.extends {
                let TSTypeName::IdentifierReference(parent) = &heritage.type_name else { continue };
                if let Some(parent) = self.named_members(parent.name.as_str(), depth + 1, path) {
                    for entry in parent {
                        merge(&mut inherited, entry);
                    }
                }
            }
        }
        for entry in own {
            set(&mut inherited, entry);
        }
        Some(inherited)
    }

    /// The member a property or method signature declares under a static identifier key. A method
    /// is a function, which no property fact covers, so its name is open. Other signatures name no property.
    fn signature_member(&self, member: &TSSignature<'_>) -> Option<(Box<str>, u16)> {
        match member {
            TSSignature::TSPropertySignature(property) => {
                let PropertyKey::StaticIdentifier(identifier) = &property.key else { return None };
                let mut languages = property.type_annotation.as_deref()
                    .map_or(DECLARED_ANY, |annotation| declared_language_in(&annotation.type_annotation, self));
                if property.optional {
                    languages |= language_bit(V::Undefined);
                }
                Some((Box::from(identifier.name.as_str()), languages))
            }
            TSSignature::TSMethodSignature(method) => {
                let PropertyKey::StaticIdentifier(identifier) = &method.key else { return None };
                Some((Box::from(identifier.name.as_str()), DECLARED_ANY))
            }
            _ => None,
        }
    }

    /// Record the top-level declarations of one statement; `top` collects their span starts.
    fn top_level(&mut self, statement: &'a Statement<'a>, top: &mut FxHashSet<u32>) {
        match statement {
            Statement::ClassDeclaration(class) => self.class(class, top),
            Statement::TSInterfaceDeclaration(declaration) => self.interface(declaration, top),
            Statement::TSTypeAliasDeclaration(declaration) => self.alias(declaration, top),
            Statement::TSEnumDeclaration(declaration) => self.enumeration(declaration, top),
            Statement::ExportDeclaration(export) => self.exported(&export.declaration, top),
            Statement::ExportDefaultDeclaration(export) => {
                if let ExportDefaultDeclarationKind::ClassDeclaration(class) = &export.declaration {
                    self.class(class, top);
                }
            }
            Statement::ImportDeclaration(import) => {
                for specifier in import.specifiers.iter().flatten() {
                    let local = match specifier {
                        ImportDeclarationSpecifier::ImportSpecifier(specifier) => &specifier.local,
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(specifier) => &specifier.local,
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier) => &specifier.local,
                    };
                    self.declare(local.name.as_str(), Named::Unknown);
                }
            }
            Statement::TSImportEqualsDeclaration(import) => self.declare(import.id.name.as_str(), Named::Unknown),
            _ => {}
        }
    }

    fn exported(&mut self, declaration: &'a Declaration<'a>, top: &mut FxHashSet<u32>) {
        match declaration {
            Declaration::ClassDeclaration(class) => self.class(class, top),
            Declaration::TSInterfaceDeclaration(declaration) => self.interface(declaration, top),
            Declaration::TSTypeAliasDeclaration(declaration) => self.alias(declaration, top),
            Declaration::TSEnumDeclaration(declaration) => self.enumeration(declaration, top),
            _ => {}
        }
    }

    fn class(&mut self, class: &'a Class<'a>, top: &mut FxHashSet<u32>) {
        let Some(id) = &class.id else { return };
        top.insert(class.span.start);
        self.declare(id.name.as_str(), Named::Class(class.span.start));
    }

    fn interface(&mut self, declaration: &'a TSInterfaceDeclaration<'a>, top: &mut FxHashSet<u32>) {
        top.insert(declaration.span.start);
        let name = declaration.id.name.as_str();
        self.interfaces.entry(name.to_string()).or_default().push(declaration);
        self.declare(name, Named::Interface);
    }

    fn alias(&mut self, declaration: &'a TSTypeAliasDeclaration<'a>, top: &mut FxHashSet<u32>) {
        top.insert(declaration.span.start);
        let named = Named::Alias { annotation: &declaration.type_annotation, open: declaration.type_parameters.is_some() };
        self.declare(declaration.id.name.as_str(), named);
    }

    fn enumeration(&mut self, declaration: &'a TSEnumDeclaration<'a>, top: &mut FxHashSet<u32>) {
        top.insert(declaration.span.start);
        let languages = enum_languages(&declaration.body.members);
        let named = if languages == DECLARED_ANY { Named::Unknown } else { Named::Enum(languages) };
        self.declare(declaration.id.name.as_str(), named);
    }

    /// Declare `name` at the top level. A second declaration makes it unknown, except that two
    /// interfaces merge into one interface.
    fn declare(&mut self, name: &str, named: Named<'a>) {
        let merged = match (self.names.get(name).copied(), named) {
            (None, named) => named,
            (Some(Named::Interface), Named::Interface) => Named::Interface,
            (Some(_), _) => Named::Unknown,
        };
        self.names.insert(name.to_string(), merged);
    }
}

/// Makes every type-level binding that is not a top-level declaration unknown.
struct Shadowing<'n, 'a> {
    names: &'n mut TypeNames<'a>,
    /// The span starts of the top-level declarations, which `TypeNames::of` has recorded.
    top: &'n FxHashSet<u32>,
}

impl Shadowing<'_, '_> {
    /// A nested declaration of `name` starting at `start` shadows any top-level name it repeats.
    fn nested(&mut self, name: &str, start: u32) {
        if !self.top.contains(&start) {
            self.names.names.insert(name.to_string(), Named::Unknown);
        }
    }
}

impl<'a> Visit<'a> for Shadowing<'_, 'a> {
    fn visit_ts_type_parameter(&mut self, parameter: &TSTypeParameter<'a>) {
        self.names.names.insert(parameter.name.name.to_string(), Named::Unknown);
        walk::walk_ts_type_parameter(self, parameter);
    }

    fn visit_ts_type_alias_declaration(&mut self, declaration: &TSTypeAliasDeclaration<'a>) {
        self.nested(declaration.id.name.as_str(), declaration.span.start);
        walk::walk_ts_type_alias_declaration(self, declaration);
    }

    fn visit_ts_interface_declaration(&mut self, declaration: &TSInterfaceDeclaration<'a>) {
        self.nested(declaration.id.name.as_str(), declaration.span.start);
        walk::walk_ts_interface_declaration(self, declaration);
    }

    fn visit_ts_enum_declaration(&mut self, declaration: &TSEnumDeclaration<'a>) {
        self.nested(declaration.id.name.as_str(), declaration.span.start);
        walk::walk_ts_enum_declaration(self, declaration);
    }

    fn visit_class(&mut self, class: &Class<'a>) {
        if let Some(id) = &class.id {
            self.nested(id.name.as_str(), class.span.start);
        }
        walk::walk_class(self, class);
    }
}

/// Add one member to `members`; a name declared twice with different sets is open, as TypeScript
/// requires merged declarations to agree.
fn merge(members: &mut Members, (name, languages): (Box<str>, u16)) {
    match members.iter().position(|(known, _)| *known == name) {
        Some(index) => {
            if members[index].1 != languages {
                members[index].1 = DECLARED_ANY;
            }
        }
        None => members.push((name, languages)),
    }
}

/// Set the member `name` to `languages`, replacing any entry of that name (an override).
fn set(members: &mut Members, (name, languages): (Box<str>, u16)) {
    match members.iter().position(|(known, _)| *known == name) {
        Some(index) => members[index].1 = languages,
        None => members.push((name, languages)),
    }
}

/// The language types an enum's values hold: a member without an initializer or with a numeric
/// one holds a number, a string or a template without substitutions holds a string. Any other
/// initializer is `DECLARED_ANY`, as is an enum with no member at all.
fn enum_languages(members: &[TSEnumMember<'_>]) -> u16 {
    let (mut numbers, mut strings) = (false, false);
    for member in members {
        match member.initializer.as_ref() {
            None | Some(Expression::NumericLiteral(_)) => numbers = true,
            Some(Expression::UnaryExpression(unary)) if matches!(
                unary.operator,
                UnaryOperator::UnaryPlus | UnaryOperator::UnaryNegation | UnaryOperator::BitwiseNot
            ) => numbers = true,
            Some(Expression::BinaryExpression(binary)) if !matches!(binary.operator, BinaryOperator::Addition) => {
                numbers = true;
            }
            Some(Expression::StringLiteral(_)) => strings = true,
            Some(Expression::TemplateLiteral(template)) if template.expressions.is_empty() => strings = true,
            Some(_) => return DECLARED_ANY,
        }
    }
    match (numbers, strings) {
        (true, true) => language_bit(V::Number) | language_bit(V::String),
        (true, false) => language_bit(V::Number),
        (false, true) => language_bit(V::String),
        (false, false) => DECLARED_ANY,
    }
}
