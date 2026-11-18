//! Lazy rest arrays. A function whose only parameter is an identifier rest list `...rest`,
//! which its body reads only as `rest.length`, `rest[key]`, `...rest` among a call's or a
//! `new`'s arguments, or the array of `f.apply(receiver, rest)`, compiles with no parameters
//! of its own: `rest` names the function's `arguments` binding (`FunctionCompiler::arguments_alias`),
//! whose values the activation keeps unmaterialized (`Function::kept_arguments`) and
//! materializes, if ever, as the rest array (`Function::rest_alias`). Its calls then
//! allocate no rest array for those reads, and `f.apply(receiver, rest)` forwards the values
//! passed (`CallForwardingArguments`). A `...rest` spread is the binding's last read, where
//! the object it makes is the array the activation made on entry before.
use super::FunctionOptions;
use super::early::{ARGUMENTS, is_direct_eval_call};
use oxc_ast::ast::{
    Argument, ArrowFunctionExpression, BindingIdentifier, BindingPattern, CallExpression, Class,
    Expression, ForInStatement, ForOfStatement, ForStatement, Function, IdentifierReference,
    NewExpression, Statement, TaggedTemplateExpression, TryStatement,
    VariableDeclaration, WhileStatement, DoWhileStatement, WithStatement,
};
use oxc_ast_visit::{Visit, walk};
use oxc_syntax::scope::ScopeFlags;

/// The rest parameter's name, where a function compiled with `options` from `body` may read it
/// as its `arguments` binding: an ordinary function or method, not a constructor, arrow,
/// initializer or suspending body, outside `with`. `unseen` states whether the function is a
/// program's root, streams its body or has an expression body, which `body` does not hold.
pub(super) fn rest_alias<'a>(options: &FunctionOptions<'_>, unseen: (bool, bool, bool), body: &[Statement<'a>]) -> Option<String> {
    let (root, streamed, expression_body) = unseen;
    let admitted = !(root || streamed || expression_body || excluded(options));
    let params = options.defaults.filter(|params| admitted && params.items.is_empty())?;
    let BindingPattern::BindingIdentifier(name) = &params.rest.as_ref()?.rest.argument else {
        return None;
    };
    let alias = name.name.as_str();
    if alias == ARGUMENTS || alias == "eval" {
        return None;
    }
    let mut uses = RestUses::new(alias);
    for statement in body {
        uses.visit_statement(statement);
    }
    uses.admits().then(|| alias.to_owned())
}

/// Whether a function compiled with `options` never aliases its rest list: a body that
/// overrides it, or a constructor's, initializer's, suspending function's or `with`'s.
fn excluded(options: &FunctionOptions<'_>) -> bool {
    options.rest_override
        || options.implicit_body.is_some()
        || options.class_constructor
        || options.derived_constructor
        || options.class_initializer
        || options.instance_fields.is_some()
        || options.async_function
        || options.generator
        || options.with_depth != 0
}

/// What a body does with the rest parameter `alias`.
struct RestUses<'s> {
    alias: &'s str,
    /// The start of every reference to `alias`.
    references: Vec<u32>,
    reads: usize,
    /// The start of each `...alias` spread's reference.
    spreads: Vec<u32>,
    loops: usize,
    spread_in_loop: bool,
    /// A construct that may give the body a handler (`Function::arguments_passed_last`).
    handlers: bool,
    safe: bool,
}

impl<'s> RestUses<'s> {
    fn new(alias: &'s str) -> Self {
        Self { alias, references: Vec::new(), reads: 0, spreads: Vec::new(), loops: 0, spread_in_loop: false, handlers: false, safe: true }
    }

    /// Whether every reference is one of the reads, and a spread, if any, is the only one and
    /// the last reference, outside loops and handlers, so the binding stays kept.
    fn admits(&self) -> bool {
        let spread_last = match self.spreads.as_slice() {
            [] => true,
            [spread] => !self.spread_in_loop && !self.handlers && self.references.iter().all(|start| start <= spread),
            _ => false,
        };
        self.safe && self.references.len() == self.reads && spread_last
    }

    fn names_alias(&self, expression: &Expression<'_>) -> bool {
        matches!(expression, Expression::Identifier(name) if name.name == self.alias)
    }

    /// Whether `expression` is `alias.length`, `alias.key` or `alias[key]`, outside an optional chain.
    fn alias_read(&self, expression: &Expression<'_>) -> bool {
        match expression {
            Expression::StaticMemberExpression(member) if !member.optional => self.names_alias(&member.object),
            Expression::ComputedMemberExpression(member) if !member.optional => self.names_alias(&member.object),
            _ => false,
        }
    }

    /// Counts the reads a call's or `new`'s `arguments` make: `...alias`, and the array of
    /// `f.apply(receiver, alias)` when the call's callee is `f.apply` (`apply`).
    fn argument_reads(&mut self, arguments: &[Argument<'_>], apply: bool) {
        for argument in arguments {
            if let Argument::SpreadElement(spread) = argument
                && let Expression::Identifier(name) = &spread.argument
                && name.name == self.alias
            {
                self.reads += 1;
                self.spreads.push(name.span.start);
                self.spread_in_loop |= self.loops != 0;
            }
        }
        if apply && let [_, Argument::Identifier(name)] = arguments && name.name == self.alias {
            self.reads += 1;
        }
    }

    fn in_loop(&mut self, walk: impl FnOnce(&mut Self)) {
        self.loops += 1;
        walk(self);
        self.loops -= 1;
    }
}

impl<'a> Visit<'a> for RestUses<'_> {
    // A `delete alias[key]` writes the array.
    fn visit_expression(&mut self, expression: &Expression<'a>) {
        if self.alias_read(expression) {
            self.reads += 1;
        }
        if let Expression::UnaryExpression(unary) = expression
            && unary.operator == oxc_syntax::operator::UnaryOperator::Delete
        {
            self.safe &= !self.alias_read(&unary.argument);
        }
        walk::walk_expression(self, expression);
    }

    fn visit_identifier_reference(&mut self, identifier: &IdentifierReference<'a>) {
        if identifier.name == self.alias {
            self.references.push(identifier.span.start);
        }
        self.safe &= identifier.name != ARGUMENTS;
    }

    fn visit_binding_identifier(&mut self, identifier: &BindingIdentifier<'a>) {
        self.safe &= identifier.name != self.alias && identifier.name != ARGUMENTS;
    }

    fn visit_binding_pattern(&mut self, pattern: &BindingPattern<'a>) {
        self.handlers |= matches!(pattern, BindingPattern::ArrayPattern(_));
        walk::walk_binding_pattern(self, pattern);
    }

    // A read as a callee binds `this` to the array; a direct eval reaches either binding.
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        self.safe &= !self.alias_read(&call.callee) && !is_direct_eval_call(call);
        let apply = matches!(&call.callee, Expression::StaticMemberExpression(member)
            if !member.optional && member.property.name == "apply");
        self.argument_reads(call.arguments.as_slice(), apply);
        walk::walk_call_expression(self, call);
    }

    fn visit_new_expression(&mut self, new: &NewExpression<'a>) {
        self.safe &= !self.alias_read(&new.callee);
        self.argument_reads(new.arguments.as_slice(), false);
        walk::walk_new_expression(self, new);
    }

    fn visit_tagged_template_expression(&mut self, tagged: &TaggedTemplateExpression<'a>) {
        self.safe &= !self.alias_read(&tagged.tag);
        walk::walk_tagged_template_expression(self, tagged);
    }

    fn visit_try_statement(&mut self, statement: &TryStatement<'a>) {
        self.handlers = true;
        walk::walk_try_statement(self, statement);
    }

    fn visit_variable_declaration(&mut self, declaration: &VariableDeclaration<'a>) {
        self.handlers |= declaration.kind.is_using();
        walk::walk_variable_declaration(self, declaration);
    }

    fn visit_for_statement(&mut self, statement: &ForStatement<'a>) {
        self.in_loop(|uses| walk::walk_for_statement(uses, statement));
    }

    fn visit_for_in_statement(&mut self, statement: &ForInStatement<'a>) {
        self.in_loop(|uses| walk::walk_for_in_statement(uses, statement));
    }

    // Its iterator's close is a handler's.
    fn visit_for_of_statement(&mut self, statement: &ForOfStatement<'a>) {
        self.handlers = true;
        self.in_loop(|uses| walk::walk_for_of_statement(uses, statement));
    }

    fn visit_while_statement(&mut self, statement: &WhileStatement<'a>) {
        self.in_loop(|uses| walk::walk_while_statement(uses, statement));
    }

    fn visit_do_while_statement(&mut self, statement: &DoWhileStatement<'a>) {
        self.in_loop(|uses| walk::walk_do_while_statement(uses, statement));
    }

    fn visit_with_statement(&mut self, _: &WithStatement<'a>) {
        self.safe = false;
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {
        self.safe = false;
    }

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {
        self.safe = false;
    }

    fn visit_class(&mut self, _: &Class<'a>) {
        self.safe = false;
    }
}

impl super::ast::FunctionCompiler<'_, '_> {
    /// Whether `name` reads the activation's `arguments` binding: `arguments`, or the rest list
    /// aliasing it (`rest_alias`).
    pub(super) fn names_arguments(&self, name: &str) -> bool {
        name == ARGUMENTS || self.arguments_alias.as_deref() == Some(name)
    }
}
