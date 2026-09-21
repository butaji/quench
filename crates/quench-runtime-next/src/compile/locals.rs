use super::*;
use rustc_hash::FxHashSet;

impl Compiler<'_> {
    pub(super) fn collect_locals(&mut self, body: &[Statement<'_>], output: &mut Vec<Atom>) {
        let mut seen: FxHashSet<_> = output.iter().copied().collect();
        self.collect_locals_into(body, output, &mut seen);
    }

    fn collect_locals_into(
        &mut self,
        body: &[Statement<'_>],
        output: &mut Vec<Atom>,
        seen: &mut FxHashSet<Atom>,
    ) {
        for statement in body {
            match statement {
                Statement::VariableDeclaration(declaration) => {
                    for item in &declaration.declarations {
                        self.collect_pattern_names(&item.id, output, seen);
                    }
                }
                Statement::FunctionDeclaration(function) => self.collect_name(
                    function.id.as_ref().map(|name| name.name.as_str()),
                    output,
                    seen,
                ),
                Statement::ClassDeclaration(class) => self.collect_name(
                    class.id.as_ref().map(|name| name.name.as_str()),
                    output,
                    seen,
                ),
                Statement::BlockStatement(block) => {
                    self.collect_locals_into(&block.body, output, seen)
                }
                Statement::IfStatement(item) => {
                    self.collect_locals_into(std::slice::from_ref(&item.consequent), output, seen);
                    if let Some(other) = &item.alternate {
                        self.collect_locals_into(std::slice::from_ref(other), output, seen);
                    }
                }
                Statement::ForStatement(item) => {
                    if let Some(ForStatementInit::VariableDeclaration(declaration)) = &item.init {
                        self.collect_declaration(declaration, output, seen);
                    }
                    self.collect_locals_into(std::slice::from_ref(&item.body), output, seen)
                }
                Statement::WhileStatement(item) => {
                    self.collect_locals_into(std::slice::from_ref(&item.body), output, seen)
                }
                Statement::DoWhileStatement(item) => {
                    self.collect_locals_into(std::slice::from_ref(&item.body), output, seen)
                }
                Statement::SwitchStatement(item) => {
                    for case in &item.cases {
                        self.collect_locals_into(&case.consequent, output, seen);
                    }
                }
                Statement::TryStatement(item) => {
                    self.collect_locals_into(&item.block.body, output, seen);
                    if let Some(handler) = &item.handler {
                        if let Some(parameter) = &handler.param {
                            self.collect_pattern_names(&parameter.pattern, output, seen);
                        }
                        self.collect_locals_into(&handler.body.body, output, seen);
                    }
                }
                _ => {}
            }
        }
    }

    fn collect_name(
        &mut self,
        name: Option<&str>,
        output: &mut Vec<Atom>,
        seen: &mut FxHashSet<Atom>,
    ) {
        if let Some(name) = name {
            let atom = self.atom(name);
            if seen.insert(atom) {
                output.push(atom);
            }
        }
    }

    fn collect_declaration(
        &mut self,
        declaration: &VariableDeclaration<'_>,
        output: &mut Vec<Atom>,
        seen: &mut FxHashSet<Atom>,
    ) {
        for item in &declaration.declarations {
            self.collect_pattern_names(&item.id, output, seen);
        }
    }

    fn collect_pattern_names(
        &mut self,
        pattern: &BindingPattern<'_>,
        output: &mut Vec<Atom>,
        seen: &mut FxHashSet<Atom>,
    ) {
        match pattern {
            BindingPattern::BindingIdentifier(id) => {
                self.collect_name(Some(id.name.as_str()), output, seen)
            }
            BindingPattern::ObjectPattern(object) => {
                for property in &object.properties {
                    self.collect_pattern_names(&property.value, output, seen);
                }
                if let Some(rest) = &object.rest {
                    self.collect_pattern_names(&rest.argument, output, seen);
                }
            }
            BindingPattern::ArrayPattern(array) => {
                for element in array.elements.iter().flatten() {
                    self.collect_pattern_names(element, output, seen);
                }
                if let Some(rest) = &array.rest {
                    self.collect_pattern_names(&rest.argument, output, seen);
                }
            }
            BindingPattern::AssignmentPattern(assignment) => {
                self.collect_pattern_names(&assignment.left, output, seen)
            }
        }
    }
}
