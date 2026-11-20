//! The decorator lowering's statements as AST nodes, built with the AST builder over the
//! decorator expressions and class nodes it moves in. Generated nodes carry the decorated
//! declaration's span, as the reparsed text did; moved nodes keep theirs.

use super::*;
use oxc_ast::builder::AstBuilder;

/// What a decoration's calls receive: the class itself (`C`), or a member of the class (its
/// target `$$quench_t` at the member's key).
#[derive(Clone, Copy)]
enum Site<'s> {
    Class(&'s str),
    Member(&'s str),
}

/// `import * as N from "source";` for each metadata namespace, at the head of a program.
pub(super) fn metadata_imports<'a>(allocator: &'a Allocator, namespaces: &[(String, String)]) -> ArenaVec<'a, Statement<'a>> {
    let emit = Emit::new(allocator, Span::empty(0));
    let imports: Vec<Statement<'a>> = namespaces.iter().map(|(source, namespace)| emit.namespace_import(source, namespace)).collect();
    emit.many(imports)
}

pub(super) struct Emit<'a> {
    allocator: &'a Allocator,
    ast: AstBuilder<'a>,
    span: Span,
}

impl<'a> Emit<'a> {
    pub(super) fn new(allocator: &'a Allocator, span: Span) -> Self {
        Self { allocator, ast: AstBuilder::new(allocator), span }
    }

    /// `let C = class C {…};`, or `export let C = …;` for a module's class.
    pub(super) fn class_binding(&self, name: &str, class: ArenaBox<'a, Class<'a>>, how: Export<'_>) -> Statement<'a> {
        let declarator = self.declarator(name, Expression::ClassExpression(class));
        let declaration = Declaration::new_variable_declaration(self.span, VariableDeclarationKind::Let, self.one(declarator), false, &self.ast);
        match how {
            Export::Module => Statement::new_export_declaration(self.span, declaration, &self.ast),
            Export::None | Export::Namespace(_) => Statement::from(declaration),
        }
    }

    /// `N.C = C;` for a class of namespace `N`.
    pub(super) fn publish(&self, namespace: &str, name: &str) -> Statement<'a> {
        let property = IdentifierName::new(self.span, self.text(name), &self.ast);
        let target = AssignmentTarget::new_static_member_expression(self.span, self.identifier(namespace), property, false, &self.ast);
        let value = Expression::new_assignment_expression(self.span, AssignmentOperator::Assign, target, self.identifier(name), &self.ast);
        self.statement(value)
    }

    /// One decoration as a block, in tsc's order: the temporaries (the target of a member,
    /// then the decorators and parameter decorators in source order), the descriptor of a
    /// member, the metadata, the parameter decorators in reverse, the decorators in reverse,
    /// and a member's descriptor defined.
    pub(super) fn decoration(&self, class: &str, decoration: Decoration<'a>) -> Statement<'a> {
        let Decoration { is_static, key, has_descriptor, decorators, parameters, metadata } = decoration;
        let count = decorators.len();
        let slots: Vec<(usize, usize)> = parameters.iter().enumerate().map(|(offset, (index, _))| (count + offset, *index)).collect();
        let values: Vec<Expression<'a>> = decorators.into_iter().chain(parameters.into_iter().map(|(_, value)| value)).collect();
        let mut statements = Vec::new();
        match key {
            None => {
                statements.extend(self.temporaries(None, values));
                statements.extend(self.applications(Site::Class(class), metadata, &slots, count));
            }
            Some(key) => {
                let site = Site::Member(&key);
                let target = if is_static { self.identifier(class) } else { self.member(self.identifier(class), "prototype") };
                statements.extend(self.temporaries(Some(target), values));
                let descriptor = if has_descriptor { self.descriptor(site) } else { self.void_zero() };
                statements.push(self.variables(VariableDeclarationKind::Let, vec![self.declarator("$$quench_r", descriptor)]));
                statements.extend(self.applications(site, metadata, &slots, count));
                statements.push(self.define(site));
            }
        }
        Statement::new_block_statement(self.span, self.many(statements), &self.ast)
    }

    /// `const $$quench_t = target, $$quench_d0 = …;`: the target (for a member) and each
    /// decorator's value, bound once in source order.
    fn temporaries(&self, target: Option<Expression<'a>>, values: Vec<Expression<'a>>) -> Option<Statement<'a>> {
        let mut declarators = Vec::new();
        if let Some(target) = target { declarators.push(self.declarator("$$quench_t", target)); }
        for (slot, value) in values.into_iter().enumerate() {
            declarators.push(self.declarator(&format!("$$quench_d{slot}"), value));
        }
        (!declarators.is_empty()).then(|| self.variables(VariableDeclarationKind::Const, declarators))
    }

    /// `$$quench_r = Object.getOwnPropertyDescriptor($$quench_t, "key");`'s initializer.
    fn descriptor(&self, site: Site<'_>) -> Expression<'a> {
        let callee = self.member(self.identifier("Object"), "getOwnPropertyDescriptor");
        self.call(callee, vec![self.target(site), self.key(site)])
    }

    /// `if (typeof Reflect === "object" && typeof Reflect.metadata === "function") { … }`
    /// (the metadata, last first), then the parameter decorators and the decorators.
    fn applications(&self, site: Site<'_>, metadata: Vec<(&'static str, Runtime)>, slots: &[(usize, usize)], count: usize) -> Vec<Statement<'a>> {
        let mut statements = vec![self.metadata_block(site, metadata)];
        for &(slot, index) in slots.iter().rev() {
            let callee = self.identifier(&format!("$$quench_d{slot}"));
            statements.push(self.statement(self.call(callee, self.parameter_arguments(site, index))));
        }
        for slot in (0..count).rev() {
            statements.push(self.apply(site, self.identifier(&format!("$$quench_d{slot}"))));
        }
        statements
    }

    fn metadata_block(&self, site: Site<'_>, metadata: Vec<(&'static str, Runtime)>) -> Statement<'a> {
        let object = self.compare(self.typeof_(self.identifier("Reflect")), BinaryOperator::StrictEquality, self.string("object"));
        let function = self.compare(self.typeof_(self.member(self.identifier("Reflect"), "metadata")), BinaryOperator::StrictEquality, self.string("function"));
        let applied: Vec<Statement<'a>> = metadata.into_iter().rev().map(|(name, value)| {
            let callee = self.member(self.identifier("Reflect"), "metadata");
            let metadata = self.call(callee, vec![self.string(name), self.runtime(value)]);
            self.apply(site, metadata)
        }).collect();
        let body = Statement::new_block_statement(self.span, self.many(applied), &self.ast);
        Statement::new_if_statement(self.span, self.and(object, function), body, None, &self.ast)
    }

    /// `result = callee(site…) || result;`, the result being the class or the descriptor.
    fn apply(&self, site: Site<'_>, callee: Expression<'a>) -> Statement<'a> {
        let result = Self::result(site);
        let call = self.call(callee, self.applied_arguments(site));
        let value = self.or(call, self.identifier(result));
        self.statement(self.assign(result, value))
    }

    /// `$$quench_r && Object.defineProperty($$quench_t, "key", $$quench_r);`
    fn define(&self, site: Site<'_>) -> Statement<'a> {
        let callee = self.member(self.identifier("Object"), "defineProperty");
        let define = self.call(callee, vec![self.target(site), self.key(site), self.identifier("$$quench_r")]);
        self.statement(self.and(self.identifier("$$quench_r"), define))
    }

    /// `import * as N from "source";`
    fn namespace_import(&self, source: &str, namespace: &str) -> Statement<'a> {
        let local = BindingIdentifier::new(self.span, self.text(namespace), &self.ast);
        let specifier = ImportNamespaceSpecifier::boxed(self.span, local, &self.ast);
        let specifier = ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier);
        let literal = StringLiteral::new(self.span, self.text(source), Some(self.text(&enums::quote(source)).into()), &self.ast);
        Statement::new_import_declaration(self.span, Some(self.one(specifier)), literal, None, None, ImportOrExportKind::Value, &self.ast)
    }

    /// The class (`C`) or the member's target (`$$quench_t`).
    fn target(&self, site: Site<'_>) -> Expression<'a> {
        match site {
            Site::Class(name) => self.identifier(name),
            Site::Member(_) => self.identifier("$$quench_t"),
        }
    }

    /// `void 0` for the class, the member's key otherwise.
    fn key(&self, site: Site<'_>) -> Expression<'a> {
        match site {
            Site::Class(_) => self.void_zero(),
            Site::Member(key) => self.string(key),
        }
    }

    fn result(site: Site<'_>) -> &str {
        match site {
            Site::Class(name) => name,
            Site::Member(_) => "$$quench_r",
        }
    }

    /// The arguments of a decorator or metadata call: the class, or the target, key and descriptor.
    fn applied_arguments(&self, site: Site<'_>) -> Vec<Expression<'a>> {
        match site {
            Site::Class(name) => vec![self.identifier(name)],
            Site::Member(_) => vec![self.target(site), self.key(site), self.identifier("$$quench_r")],
        }
    }

    /// The arguments of a parameter decorator: the target, the key and the parameter's index.
    fn parameter_arguments(&self, site: Site<'_>, index: usize) -> Vec<Expression<'a>> {
        vec![self.target(site), self.key(site), self.number(index)]
    }

    /// A metadata value's expression: a global, `void 0`, or a reference that is a
    /// constructor at run time, else `Object`.
    fn runtime(&self, value: Runtime) -> Expression<'a> {
        match value {
            Runtime::Global(name) => self.identifier(name),
            Runtime::Void => self.void_zero(),
            Runtime::Guarded(reference) => self.constructor_or_object(None, &reference),
            Runtime::Qualified(reference) => self.constructor_or_object(Some(&reference.binding), &reference),
            Runtime::Array(items) => {
                let elements: Vec<ArrayExpressionElement<'a>> = items.into_iter().map(|item| ArrayExpressionElement::from(self.runtime(item))).collect();
                Expression::new_array_expression(self.span, self.many(elements), &self.ast)
            }
        }
    }

    /// `(typeof R === "function" ? R : Object)`, with `typeof B !== "undefined" && B !== null &&`
    /// first when the root `B` may be missing.
    fn constructor_or_object(&self, root: Option<&Binding>, reference: &Reference) -> Expression<'a> {
        let function = self.compare(self.typeof_(self.reference(reference)), BinaryOperator::StrictEquality, self.string("function"));
        let test = match root {
            None => function,
            Some(root) => {
                let defined = self.compare(self.typeof_(self.binding(root)), BinaryOperator::StrictInequality, self.string("undefined"));
                let present = self.compare(self.binding(root), BinaryOperator::StrictInequality, Expression::new_null_literal(self.span, &self.ast));
                self.and(self.and(defined, present), function)
            }
        };
        let conditional = self.conditional(test, self.reference(reference), self.identifier("Object"));
        Expression::new_parenthesized_expression(self.span, conditional, &self.ast)
    }

    fn binding(&self, binding: &Binding) -> Expression<'a> {
        match binding {
            Binding::Name(name) => self.identifier(name),
            Binding::This => Expression::new_this_expression(self.span, &self.ast),
            Binding::Imported { namespace, name } => self.computed(self.identifier(namespace), self.string(name)),
        }
    }

    fn reference(&self, reference: &Reference) -> Expression<'a> {
        reference.members.iter().fold(self.binding(&reference.binding), |object, member| self.member(object, member))
    }

    fn text(&self, text: &str) -> &'a str {
        self.allocator.alloc_str(text)
    }

    fn one<T>(&self, item: T) -> ArenaVec<'a, T> {
        let mut items = ArenaVec::with_capacity_in(1, &self.allocator);
        items.push(item);
        items
    }

    fn many<T>(&self, items: Vec<T>) -> ArenaVec<'a, T> {
        let mut list = ArenaVec::with_capacity_in(items.len(), &self.allocator);
        for item in items { list.push(item); }
        list
    }

    fn identifier(&self, name: &str) -> Expression<'a> {
        Expression::new_identifier(self.span, self.text(name), &self.ast)
    }

    fn string(&self, value: &str) -> Expression<'a> {
        Expression::new_string_literal(self.span, self.text(value), Some(self.text(&enums::quote(value)).into()), &self.ast)
    }

    fn number(&self, value: usize) -> Expression<'a> {
        let text = self.text(&value.to_string());
        Expression::new_numeric_literal(self.span, value as f64, Some(text.into()), NumberBase::Decimal, &self.ast)
    }

    fn void_zero(&self) -> Expression<'a> {
        let zero = self.number(0);
        Expression::new_unary_expression(self.span, UnaryOperator::Void, zero, &self.ast)
    }

    fn typeof_(&self, operand: Expression<'a>) -> Expression<'a> {
        Expression::new_unary_expression(self.span, UnaryOperator::Typeof, operand, &self.ast)
    }

    fn compare(&self, left: Expression<'a>, operator: BinaryOperator, right: Expression<'a>) -> Expression<'a> {
        Expression::new_binary_expression(self.span, left, operator, right, &self.ast)
    }

    fn and(&self, left: Expression<'a>, right: Expression<'a>) -> Expression<'a> {
        Expression::new_logical_expression(self.span, left, LogicalOperator::And, right, &self.ast)
    }

    fn or(&self, left: Expression<'a>, right: Expression<'a>) -> Expression<'a> {
        Expression::new_logical_expression(self.span, left, LogicalOperator::Or, right, &self.ast)
    }

    fn conditional(&self, test: Expression<'a>, consequent: Expression<'a>, alternate: Expression<'a>) -> Expression<'a> {
        Expression::new_conditional_expression(self.span, test, consequent, alternate, &self.ast)
    }

    fn member(&self, object: Expression<'a>, property: &str) -> Expression<'a> {
        let property = IdentifierName::new(self.span, self.text(property), &self.ast);
        Expression::new_static_member_expression(self.span, object, property, false, &self.ast)
    }

    fn computed(&self, object: Expression<'a>, key: Expression<'a>) -> Expression<'a> {
        Expression::new_computed_member_expression(self.span, object, key, false, &self.ast)
    }

    fn call(&self, callee: Expression<'a>, arguments: Vec<Expression<'a>>) -> Expression<'a> {
        let arguments: Vec<Argument<'a>> = arguments.into_iter().map(Argument::from).collect();
        Expression::new_call_expression(self.span, callee, None, self.many(arguments), false, &self.ast)
    }

    fn assign(&self, name: &str, value: Expression<'a>) -> Expression<'a> {
        let target = AssignmentTarget::new_assignment_target_identifier(self.span, self.text(name), &self.ast);
        Expression::new_assignment_expression(self.span, AssignmentOperator::Assign, target, value, &self.ast)
    }

    fn declarator(&self, name: &str, init: Expression<'a>) -> VariableDeclarator<'a> {
        let id = BindingPattern::new_binding_identifier(self.span, self.text(name), &self.ast);
        VariableDeclarator::new(self.span, id, None, Some(init), false, &self.ast)
    }

    fn variables(&self, kind: VariableDeclarationKind, declarators: Vec<VariableDeclarator<'a>>) -> Statement<'a> {
        Statement::new_variable_declaration(self.span, kind, self.many(declarators), false, &self.ast)
    }

    fn statement(&self, expression: Expression<'a>) -> Statement<'a> {
        Statement::new_expression_statement(self.span, expression, &self.ast)
    }
}
