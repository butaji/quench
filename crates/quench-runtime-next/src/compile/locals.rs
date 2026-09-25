use super::*;
use rustc_hash::FxHashSet;

impl Compiler<'_> {
    pub(super) fn collect_locals(
        &mut self,
        body: &[Statement<'_>],
        output: &mut Vec<Atom>,
        strict: bool,
    ) -> FxHashSet<Atom> {
        let mut seen: FxHashSet<_> = output.iter().copied().collect();
        let mut function_scope = seen.clone();
        self.collect_locals_into(body, output, &mut seen, &mut function_scope, strict, false);
        function_scope
    }

    fn collect_locals_into(
        &mut self,
        body: &[Statement<'_>],
        output: &mut Vec<Atom>,
        seen: &mut FxHashSet<Atom>,
        function_scope: &mut FxHashSet<Atom>,
        strict: bool,
        nested: bool,
    ) {
        for statement in body {
            match statement {
                Statement::VariableDeclaration(declaration) => {
                    for item in &declaration.declarations {
                        self.collect_pattern_names(
                            &item.id,
                            output,
                            seen,
                            function_scope,
                            !nested || declaration.kind == VariableDeclarationKind::Var,
                        );
                    }
                }
                Statement::FunctionDeclaration(function) if !strict || !nested => self
                    .collect_name(
                        function.id.as_ref().map(|name| name.name.as_str()),
                        output,
                        seen,
                        function_scope,
                    ),
                Statement::ImportDeclaration(_) => {
                    for binding in super::module_import_bindings(std::slice::from_ref(statement)) {
                        self.collect_name(Some(&binding.local), output, seen, function_scope);
                    }
                }
                Statement::ExportDeclaration(export) => match &export.declaration {
                    Declaration::VariableDeclaration(declaration) => {
                        for item in &declaration.declarations {
                            self.collect_pattern_names(
                                &item.id,
                                output,
                                seen,
                                function_scope,
                                true,
                            );
                        }
                    }
                    Declaration::FunctionDeclaration(function) => self.collect_name(
                        function.id.as_ref().map(|name| name.name.as_str()),
                        output,
                        seen,
                        function_scope,
                    ),
                    Declaration::ClassDeclaration(class) => self.collect_name(
                        class.id.as_ref().map(|name| name.name.as_str()),
                        output,
                        seen,
                        function_scope,
                    ),
                    _ => {}
                },
                Statement::ExportDefaultDeclaration(export) => {
                    let has_default_binding = export.declaration.as_expression().is_some()
                        || matches!(
                            export.declaration,
                            oxc_ast::ast::ExportDefaultDeclarationKind::FunctionDeclaration(_)
                                | oxc_ast::ast::ExportDefaultDeclarationKind::ClassDeclaration(_)
                        );
                    if has_default_binding {
                        let binding = super::module_default_binding(self.source);
                        self.collect_name(Some(&binding), output, seen, function_scope);
                    }
                    match &export.declaration {
                        oxc_ast::ast::ExportDefaultDeclarationKind::FunctionDeclaration(
                            function,
                        ) => self.collect_name(
                            function.id.as_ref().map(|name| name.name.as_str()),
                            output,
                            seen,
                            function_scope,
                        ),
                        oxc_ast::ast::ExportDefaultDeclarationKind::ClassDeclaration(class) => self
                            .collect_name(
                                class.id.as_ref().map(|name| name.name.as_str()),
                                output,
                                seen,
                                function_scope,
                            ),
                        _ => {}
                    }
                }
                Statement::ClassDeclaration(class) if !nested => self.collect_name(
                    class.id.as_ref().map(|name| name.name.as_str()),
                    output,
                    seen,
                    function_scope,
                ),
                Statement::BlockStatement(block) => self.collect_locals_into(
                    &block.body,
                    output,
                    seen,
                    function_scope,
                    strict,
                    true,
                ),
                Statement::IfStatement(item) => {
                    self.collect_locals_into(
                        std::slice::from_ref(&item.consequent),
                        output,
                        seen,
                        function_scope,
                        strict,
                        true,
                    );
                    if let Some(other) = &item.alternate {
                        self.collect_locals_into(
                            std::slice::from_ref(other),
                            output,
                            seen,
                            function_scope,
                            strict,
                            true,
                        );
                    }
                }
                Statement::ForStatement(item) => {
                    if let Some(ForStatementInit::VariableDeclaration(declaration)) = &item.init {
                        self.collect_declaration(
                            declaration,
                            output,
                            seen,
                            function_scope,
                            declaration.kind == VariableDeclarationKind::Var,
                        );
                    }
                    self.collect_locals_into(
                        std::slice::from_ref(&item.body),
                        output,
                        seen,
                        function_scope,
                        strict,
                        true,
                    )
                }
                Statement::ForInStatement(item) => {
                    if let ForStatementLeft::VariableDeclaration(declaration) = &item.left {
                        self.collect_declaration(
                            declaration,
                            output,
                            seen,
                            function_scope,
                            declaration.kind == VariableDeclarationKind::Var,
                        );
                    }
                    self.collect_locals_into(
                        std::slice::from_ref(&item.body),
                        output,
                        seen,
                        function_scope,
                        strict,
                        true,
                    )
                }
                Statement::ForOfStatement(item) => {
                    if let ForStatementLeft::VariableDeclaration(declaration) = &item.left {
                        self.collect_declaration(
                            declaration,
                            output,
                            seen,
                            function_scope,
                            declaration.kind == VariableDeclarationKind::Var,
                        );
                    }
                    self.collect_locals_into(
                        std::slice::from_ref(&item.body),
                        output,
                        seen,
                        function_scope,
                        strict,
                        true,
                    )
                }
                Statement::WhileStatement(item) => self.collect_locals_into(
                    std::slice::from_ref(&item.body),
                    output,
                    seen,
                    function_scope,
                    strict,
                    true,
                ),
                Statement::DoWhileStatement(item) => self.collect_locals_into(
                    std::slice::from_ref(&item.body),
                    output,
                    seen,
                    function_scope,
                    strict,
                    true,
                ),
                Statement::SwitchStatement(item) => {
                    for case in &item.cases {
                        for statement in &case.consequent {
                            if matches!(statement, Statement::FunctionDeclaration(_))
                                || matches!(statement, Statement::VariableDeclaration(declaration)
                                    if super::is_lexical_binding_declaration(declaration.kind))
                            {
                                continue;
                            }
                            self.collect_locals_into(
                                std::slice::from_ref(statement),
                                output,
                                seen,
                                function_scope,
                                strict,
                                true,
                            );
                        }
                    }
                }
                Statement::TryStatement(item) => {
                    self.collect_locals_into(
                        &item.block.body,
                        output,
                        seen,
                        function_scope,
                        strict,
                        true,
                    );
                    if let Some(handler) = &item.handler {
                        if let Some(parameter) = &handler.param {
                            self.collect_pattern_names(
                                &parameter.pattern,
                                output,
                                seen,
                                function_scope,
                                false,
                            );
                        }
                        self.collect_locals_into(
                            &handler.body.body,
                            output,
                            seen,
                            function_scope,
                            strict,
                            true,
                        );
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
        function_scope: &mut FxHashSet<Atom>,
    ) {
        if let Some(name) = name {
            let atom = self.atom(name);
            function_scope.insert(atom);
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
        function_scope: &mut FxHashSet<Atom>,
        belongs_to_function_scope: bool,
    ) {
        for item in &declaration.declarations {
            self.collect_pattern_names(
                &item.id,
                output,
                seen,
                function_scope,
                belongs_to_function_scope,
            );
        }
    }

    fn collect_pattern_names(
        &mut self,
        pattern: &BindingPattern<'_>,
        output: &mut Vec<Atom>,
        seen: &mut FxHashSet<Atom>,
        function_scope: &mut FxHashSet<Atom>,
        belongs_to_function_scope: bool,
    ) {
        match pattern {
            BindingPattern::BindingIdentifier(id) => {
                let atom = self.atom(id.name.as_str());
                if belongs_to_function_scope {
                    function_scope.insert(atom);
                }
                if seen.insert(atom) {
                    output.push(atom);
                }
            }
            BindingPattern::ObjectPattern(object) => {
                for property in &object.properties {
                    self.collect_pattern_names(
                        &property.value,
                        output,
                        seen,
                        function_scope,
                        belongs_to_function_scope,
                    );
                }
                if let Some(rest) = &object.rest {
                    self.collect_pattern_names(
                        &rest.argument,
                        output,
                        seen,
                        function_scope,
                        belongs_to_function_scope,
                    );
                }
            }
            BindingPattern::ArrayPattern(array) => {
                for element in array.elements.iter().flatten() {
                    self.collect_pattern_names(
                        element,
                        output,
                        seen,
                        function_scope,
                        belongs_to_function_scope,
                    );
                }
                if let Some(rest) = &array.rest {
                    self.collect_pattern_names(
                        &rest.argument,
                        output,
                        seen,
                        function_scope,
                        belongs_to_function_scope,
                    );
                }
            }
            BindingPattern::AssignmentPattern(assignment) => self.collect_pattern_names(
                &assignment.left,
                output,
                seen,
                function_scope,
                belongs_to_function_scope,
            ),
        }
    }
}
