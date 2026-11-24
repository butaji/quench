//! JSDoc as a spelling of TypeScript annotations: a JavaScript tree whose JSDoc comments
//! declare types (`@param`, `@returns`, `@type`, `@typedef`, `@template`, a `@type` cast) gets
//! those types as the TypeScript nodes the same text would hold, before admission. Every fact a
//! declared type seeds is then read from one place (`signatures`, `fields`, `declared_parameters`),
//! so a JSDoc function and its TypeScript twin give the same facts and the same code. A type the
//! TypeScript grammar does not parse, once the Closure spellings are rewritten, declares nothing.

use oxc_allocator::{Allocator, Box as ArenaBox, TakeIn};
use oxc_ast::ast::*;
use oxc_ast_visit::{VisitMut, walk_mut};
use oxc_span::{SourceType, Span};
use oxc_syntax::scope::ScopeFlags;
use rustc_hash::FxHashMap;

/// Gives `program` (JavaScript) the TypeScript annotations its JSDoc comments declare. Whether
/// it gave any, so the tree is admitted as typed text is (`admit`).
/// Each comment that types a function from outside the function's text is recorded in `declared`
/// (`DeclaredTypes::document`), so a body compiled later from its text alone gets it back.
pub(in crate::compile) fn annotate<'a>(allocator: &'a Allocator, program: &mut Program<'a>, declared: &mut crate::bytecode::DeclaredTypes) -> bool {
    let text = program.source_text;
    let docs: FxHashMap<u32, Doc<'a>> = program.comments.iter()
        .filter(|comment| comment.is_jsdoc())
        .filter_map(|comment| {
            let body = &text[comment.span.start as usize..comment.span.end as usize];
            body.contains('@').then(|| (comment.attached_to, Doc::parse(body, comment.span)))
        })
        .filter(|(_, doc)| !doc.is_empty())
        .collect();
    if docs.is_empty() {
        return false;
    }
    // Every `@typedef` is a top-level alias, wherever its comment stands, in source order.
    let mut typedefs: Vec<(u32, Statement<'a>)> = docs.values().flat_map(|doc| doc.typedefs.iter()
        .filter_map(|&(name, ty)| type_alias(allocator, name, ty, doc.span)).map(|alias| (doc.span.start, alias))).collect();
    typedefs.sort_by_key(|(start, _)| *start);
    let mut annotate = Annotate { allocator, docs, annotated: !typedefs.is_empty(), documented: Vec::new() };
    annotate.visit_program(program);
    for (function, doc) in annotate.documented.drain(..) {
        declared.document(function, doc.start, doc.end);
    }
    for (index, (_, alias)) in typedefs.into_iter().enumerate() {
        program.body.insert(index, alias);
    }
    annotate.annotated
}

/// The type-declaring tags of one JSDoc comment.
#[derive(Default)]
struct Doc<'s> {
    span: Span,
    /// `@param {T} name`, `@param {T} [name]` (optional): the name, the type, whether optional.
    params: Vec<(&'s str, &'s str, bool)>,
    returns: Option<&'s str>,
    ty: Option<&'s str>,
    /// `@typedef {T} Name`.
    typedefs: Vec<(&'s str, &'s str)>,
    /// `@template T, U`.
    templates: Vec<&'s str>,
}

impl<'s> Doc<'s> {
    fn parse(body: &'s str, span: Span) -> Self {
        let mut doc = Doc { span, ..Doc::default() };
        let mut rest = body;
        while let Some(at) = rest.find('@') {
            rest = &rest[at + 1..];
            let tag_end = rest.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(rest.len());
            let (tag, after) = rest.split_at(tag_end);
            let (ty, after) = braced(after);
            match (tag, ty) {
                ("param" | "arg" | "argument", Some(ty)) => {
                    if let Some((name, optional)) = param_name(after) { doc.params.push((name, ty, optional)); }
                }
                ("returns" | "return", Some(ty)) => doc.returns = Some(ty),
                ("type", Some(ty)) => doc.ty = Some(ty),
                ("typedef", Some(ty)) => {
                    if let Some(name) = word(after) { doc.typedefs.push((name, ty)); }
                }
                ("template", None) => doc.templates.extend(after.split(['\n', '*']).next().unwrap_or("")
                    .split(',').map(str::trim).filter(|name| is_identifier(name))),
                _ => {}
            }
            rest = after;
        }
        doc
    }

    fn is_empty(&self) -> bool {
        self.params.is_empty() && self.returns.is_none() && self.ty.is_none() && self.typedefs.is_empty() && self.templates.is_empty()
    }
}

/// The `{…}` type expression at the start of `text` (after blanks), braces balanced, and the text after it.
fn braced(text: &str) -> (Option<&str>, &str) {
    let trimmed = text.trim_start_matches([' ', '\t']);
    if !trimmed.starts_with('{') {
        return (None, text);
    }
    let mut depth = 0usize;
    for (index, c) in trimmed.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 { return (Some(trimmed[1..index].trim()), &trimmed[index + 1..]); }
            }
            '\n' if trimmed[..index].trim_end().ends_with('@') => break,
            _ => {}
        }
    }
    (None, text)
}

/// A parameter tag's name: `name`, or `[name]` / `[name=default]` (optional). A dotted name
/// documents a member of an object parameter, which this does not type.
fn param_name(text: &str) -> Option<(&str, bool)> {
    let text = text.trim_start();
    if let Some(inner) = text.strip_prefix('[') {
        let end = inner.find([']', '='])?;
        let name = inner[..end].trim();
        return is_identifier(name).then_some((name, true));
    }
    word(text).map(|name| (name, false))
}

fn word(text: &str) -> Option<&str> {
    let text = text.trim_start();
    let end = text.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$' || c == '.')).unwrap_or(text.len());
    let name = &text[..end];
    is_identifier(name).then_some(name)
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

/// The TypeScript spelling of a JSDoc type expression: Closure's `*`, `?T`, `!T`, `T=`, `...T`,
/// `Array.<T>` and `function(…)` rewritten; everything else is TypeScript already.
fn typescript_spelling(ty: &str) -> String {
    let ty = ty.trim();
    if ty.is_empty() || ty == "*" || ty == "?" {
        return "any".into();
    }
    if let Some(rest) = ty.strip_prefix("...") {
        return format!("({})[]", typescript_spelling(rest));
    }
    if let Some(rest) = ty.strip_prefix('?') {
        return format!("({}) | null", typescript_spelling(rest));
    }
    if let Some(rest) = ty.strip_prefix('!') {
        return typescript_spelling(rest);
    }
    if let Some(rest) = ty.strip_suffix('=') {
        return format!("({}) | undefined", typescript_spelling(rest));
    }
    if ty.starts_with("function(") {
        return "(...args: any[]) => any".into();
    }
    ty.replace(".<", "<")
}

/// Parses `snippet`, TypeScript, into the arena: its first statement, where it parsed without error.
fn parse_statement<'a>(allocator: &'a Allocator, snippet: String) -> Option<Statement<'a>> {
    let text = allocator.alloc_str(&snippet);
    let mut parsed = oxc_parser::Parser::new(allocator, text, SourceType::mjs().with_typescript(true)).parse();
    if !parsed.diagnostics.is_empty() || parsed.stack_exhausted || parsed.program.body.len() != 1 {
        return None;
    }
    parsed.program.body.pop()
}

/// The annotation `: T` for the JSDoc type `ty`.
fn annotation<'a>(allocator: &'a Allocator, ty: &str) -> Option<ArenaBox<'a, TSTypeAnnotation<'a>>> {
    let Statement::VariableDeclaration(mut declaration) = parse_statement(allocator, format!("let _: {};", typescript_spelling(ty)))? else { return None };
    declaration.declarations.first_mut()?.type_annotation.take()
}

/// The type parameters `<T, U>` `@template` names.
fn type_parameters<'a>(allocator: &'a Allocator, names: &[&str]) -> Option<ArenaBox<'a, TSTypeParameterDeclaration<'a>>> {
    let Statement::FunctionDeclaration(mut function) = parse_statement(allocator, format!("function _<{}>() {{}}", names.join(", ")))? else { return None };
    function.type_parameters.take()
}

/// `type Name = T;`, at the comment's span so each alias has a declaration start of its own.
fn type_alias<'a>(allocator: &'a Allocator, name: &str, ty: &str, span: Span) -> Option<Statement<'a>> {
    let mut statement = parse_statement(allocator, format!("type {name} = {};", typescript_spelling(ty)))?;
    let Statement::TSTypeAliasDeclaration(alias) = &mut statement else { return None };
    alias.span = span;
    Some(statement)
}

struct Annotate<'a> {
    allocator: &'a Allocator,
    /// The comments, by the start of the token each leads.
    docs: FxHashMap<u32, Doc<'a>>,
    annotated: bool,
    /// The functions typed by a comment outside their text, by start, with the comment's range.
    documented: Vec<(u32, Span)>,
}

impl<'a> Annotate<'a> {
    /// The signature `doc` declares, given to a function's parameters, return and type parameters
    /// where the source gave none.
    fn signature(&mut self, doc: &Doc<'_>, start: u32, params: &mut FormalParameters<'a>, returns: &mut Option<ArenaBox<'a, TSTypeAnnotation<'a>>>,
        templates: &mut Option<ArenaBox<'a, TSTypeParameterDeclaration<'a>>>) {
        if doc.span.end <= start && (!doc.params.is_empty() || doc.returns.is_some() || !doc.templates.is_empty()) {
            self.documented.push((start, doc.span));
        }
        self.parameters(doc, params);
        if returns.is_none() && let Some(ty) = doc.returns {
            *returns = annotation(self.allocator, ty);
            self.annotated |= returns.is_some();
        }
        if templates.is_none() && !doc.templates.is_empty() {
            *templates = type_parameters(self.allocator, &doc.templates);
        }
    }

    /// Each `@param` given to the parameter it names, where the source gave it no annotation.
    fn parameters(&mut self, doc: &Doc<'_>, params: &mut FormalParameters<'a>) {
        for item in params.items.iter_mut() {
            let Some(name) = item.pattern.get_identifier_name() else { continue };
            let Some(&(_, ty, optional)) = doc.params.iter().find(|(param, ..)| *param == name.as_str()) else { continue };
            if item.type_annotation.is_some() { continue; }
            let spelled = if optional { format!("({ty}) | undefined") } else { ty.to_owned() };
            item.type_annotation = annotation(self.allocator, &spelled);
            self.annotated |= item.type_annotation.is_some();
        }
    }

    /// A doc leading a declaration (`const f = …`, `export …`) given to what it declares.
    fn declaration(&mut self, doc: &Doc<'_>, declaration: &mut Declaration<'a>) {
        match declaration {
            Declaration::FunctionDeclaration(function) => self.function(doc, function),
            Declaration::VariableDeclaration(variables) => self.variables(doc, variables),
            _ => {}
        }
    }

    fn function(&mut self, doc: &Doc<'_>, function: &mut Function<'a>) {
        self.signature(doc, function.span.start, &mut function.params, &mut function.return_type, &mut function.type_parameters);
    }

    fn arrow(&mut self, doc: &Doc<'_>, arrow: &mut ArrowFunctionExpression<'a>) {
        self.signature(doc, arrow.span.start, &mut arrow.params, &mut arrow.return_type, &mut arrow.type_parameters);
    }

    fn variables(&mut self, doc: &Doc<'_>, variables: &mut VariableDeclaration<'a>) {
        let [declarator] = variables.declarations.as_mut_slice() else { return };
        if let Some(ty) = doc.ty && declarator.type_annotation.is_none() {
            declarator.type_annotation = annotation(self.allocator, ty);
            self.annotated |= declarator.type_annotation.is_some();
        }
        match declarator.init.as_mut().map(Expression::without_parentheses_mut) {
            Some(Expression::FunctionExpression(function)) => self.function(doc, function),
            Some(Expression::ArrowFunctionExpression(arrow)) => self.arrow(doc, arrow),
            _ => {}
        }
    }

    fn take(&mut self, start: u32) -> Option<Doc<'a>> { self.docs.remove(&start) }
}

impl<'a> VisitMut<'a> for Annotate<'a> {
    fn visit_statement(&mut self, statement: &mut Statement<'a>) {
        use oxc_span::GetSpan;
        if let Some(doc) = self.take(statement.span().start) {
            match statement {
                Statement::ExportDeclaration(export) => self.declaration(&doc, &mut export.declaration),
                Statement::ExportDefaultDeclaration(export) => {
                    if let ExportDefaultDeclarationKind::FunctionDeclaration(function) = &mut export.declaration { self.function(&doc, function); }
                }
                Statement::FunctionDeclaration(function) => self.function(&doc, function),
                Statement::VariableDeclaration(variables) => self.variables(&doc, variables),
                _ => {}
            }
        }
        walk_mut::walk_statement(self, statement);
    }

    fn visit_method_definition(&mut self, method: &mut MethodDefinition<'a>) {
        if let Some(doc) = self.take(method.span.start) {
            // A method's body compiled alone is parsed from the method's own text.
            if doc.span.end <= method.span.start { self.documented.push((method.span.start, doc.span)); }
            self.function(&doc, &mut method.value);
        }
        walk_mut::walk_method_definition(self, method);
    }

    fn visit_property_definition(&mut self, property: &mut PropertyDefinition<'a>) {
        if let Some(doc) = self.take(property.span.start) {
            if let Some(ty) = doc.ty && property.type_annotation.is_none() {
                property.type_annotation = annotation(self.allocator, ty);
                self.annotated |= property.type_annotation.is_some();
            }
            match property.value.as_mut().map(Expression::without_parentheses_mut) {
                Some(Expression::FunctionExpression(function)) => self.function(&doc, function),
                Some(Expression::ArrowFunctionExpression(arrow)) => self.arrow(&doc, arrow),
                _ => {}
            }
        }
        walk_mut::walk_property_definition(self, property);
    }

    fn visit_object_property(&mut self, property: &mut ObjectProperty<'a>) {
        if let Some(doc) = self.take(property.span.start) {
            match property.value.without_parentheses_mut() {
                Expression::FunctionExpression(function) => self.function(&doc, function),
                Expression::ArrowFunctionExpression(arrow) => self.arrow(&doc, arrow),
                _ => {}
            }
        }
        walk_mut::walk_object_property(self, property);
    }

    fn visit_arrow_function_expression(&mut self, arrow: &mut ArrowFunctionExpression<'a>) {
        if let Some(doc) = self.take(arrow.span.start) { self.arrow(&doc, arrow); }
        walk_mut::walk_arrow_function_expression(self, arrow);
    }

    fn visit_function(&mut self, function: &mut Function<'a>, flags: ScopeFlags) {
        if let Some(doc) = self.take(function.span.start) { self.function(&doc, function); }
        walk_mut::walk_function(self, function, flags);
    }

    /// `/** @type {T} */ (e)`: the cast `e as T`.
    fn visit_expression(&mut self, expression: &mut Expression<'a>) {
        if let Expression::ParenthesizedExpression(parenthesized) = expression
            && let Some(ty) = self.docs.get(&parenthesized.span.start).and_then(|doc| doc.ty)
            && let Some(Statement::ExpressionStatement(mut statement)) =
                parse_statement(self.allocator, format!("0 as {};", typescript_spelling(ty)))
            && let Expression::TSAsExpression(cast) = &mut statement.expression {
            self.docs.remove(&parenthesized.span.start);
            cast.expression = parenthesized.expression.take_in(&self.allocator);
            cast.span = parenthesized.span;
            *expression = statement.expression.take_in(&self.allocator);
            self.annotated = true;
        }
        walk_mut::walk_expression(self, expression);
    }
}
