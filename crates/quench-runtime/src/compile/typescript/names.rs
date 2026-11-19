//! The type-level names of a TypeScript program (`TypeNames`): what each named type annotation
//! denotes. A name the program declares at its top level denotes that declaration; a name declared
//! twice, or bound anywhere else at a type level (an import, a nested declaration, a type
//! parameter), denotes nothing this pass knows, so its annotation stays open (`DECLARED_ANY`).

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_syntax::operator::{BinaryOperator, UnaryOperator};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::ValueType as V;
use crate::typed_ir::language_bit;
use super::DECLARED_ANY;

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
    /// Denotes nothing this pass knows.
    Unknown,
}

/// The type-level names of one program, by name.
#[derive(Default)]
pub(in crate::compile) struct TypeNames<'a> {
    names: FxHashMap<String, Named<'a>>,
}

impl<'a> TypeNames<'a> {
    /// The names `program` declares at its top level, then every other type-level binding made
    /// unknown: a nested declaration or a type parameter shadows the top-level name it repeats.
    pub(in crate::compile) fn of(program: &'a Program<'a>) -> Self {
        let mut names = Self::default();
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
        self.names.get(name).copied()
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
        self.declare(declaration.id.name.as_str(), Named::Interface);
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
