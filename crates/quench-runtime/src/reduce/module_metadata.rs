use oxc::ast::ast::{
    BindingPattern, BindingPatternKind, Declaration, ExportDefaultDeclaration,
    ExportDefaultDeclarationKind, ExportNamedDeclaration, ModuleExportName, Statement,
};

/// Static module edges and names discovered directly from OXC's module AST.
///
/// This is metadata only: it does not alter reduction or provide module
/// execution semantics. Hosts can use it to construct a linker input.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ModuleMetadata {
    pub import_specifiers: Vec<String>,
    pub import_types: Vec<(String, String)>,
    pub imports: Vec<ImportBinding>,
    pub exports: Vec<ExportBinding>,
    pub reexports: Vec<ReexportBinding>,
    pub exported_names: Vec<String>,
    pub has_top_level_await: bool,
    pub requests: Vec<ModuleRequest>,
}

/// One ModuleRequest Record: specifier + phase, in source order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleRequest {
    pub source: String,
    pub deferred: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBinding {
    pub source: String,
    pub imported: String,
    pub local: String,
    pub deferred: bool,
}

impl ImportBinding {
    /// Side-effect `import "./x"` has no imported/local names.
    pub fn is_binding(&self) -> bool {
        !self.imported.is_empty() || !self.local.is_empty()
    }
}

/// One local binding exposed under an exported module name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportBinding {
    pub local: String,
    pub exported: String,
    pub source: bool,
}

/// A binding re-exposed from another module without a local runtime slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReexportBinding {
    pub source: String,
    pub imported: String,
    pub exported: String,
}

impl ModuleMetadata {
    pub(crate) fn from_statements(statements: &[Statement<'_>]) -> Self {
        let mut metadata = Self::default();
        for statement in statements {
            metadata.visit_statement(statement);
        }
        metadata.mark_source_exports();
        metadata.has_top_level_await = statements.iter().any(statement_has_tla);
        for specifier in dynamic_import_specifiers(statements) {
            push_unique(&mut metadata.import_specifiers, &specifier);
        }
        metadata
    }

    fn mark_source_exports(&mut self) {
        for export in &mut self.exports {
            export.source = self
                .imports
                .iter()
                .any(|import| import.local == export.local && import.imported == "source");
        }
    }

    fn visit_statement(&mut self, statement: &Statement<'_>) {
        match statement {
            Statement::ImportDeclaration(import) => {
                let source = import.source.value.to_string();
                let deferred = import.phase == Some(oxc::ast::ast::ImportPhase::Defer);
                push_unique(&mut self.import_specifiers, &source);
                self.push_request(&source, deferred);
                if let Some(with_clause) = &import.with_clause {
                    for entry in &with_clause.with_entries {
                        let key = match &entry.key {
                            oxc::ast::ast::ImportAttributeKey::Identifier(key) => {
                                key.name.to_string()
                            }
                            oxc::ast::ast::ImportAttributeKey::StringLiteral(key) => {
                                key.value.to_string()
                            }
                        };
                        self.import_types
                            .push((source.clone(), format!("{key}={}", entry.value.value)));
                    }
                }
                if import
                    .specifiers
                    .as_ref()
                    .is_none_or(|specifiers| specifiers.is_empty())
                {
                    self.imports.push(ImportBinding {
                        source: source.clone(),
                        imported: String::new(),
                        local: String::new(),
                        deferred,
                    });
                }
                if let Some(specifiers) = &import.specifiers {
                    for specifier in specifiers {
                        let (imported, local) = match specifier {
                            oxc::ast::ast::ImportDeclarationSpecifier::ImportSpecifier(value) => (
                                module_export_name(&value.imported),
                                value.local.name.to_string(),
                            ),
                            oxc::ast::ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(
                                value,
                            ) => ("default".to_string(), value.local.name.to_string()),
                            oxc::ast::ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(
                                value,
                            ) => ("*".to_string(), value.local.name.to_string()),
                        };
                        let imported = if import.phase == Some(oxc::ast::ast::ImportPhase::Source) {
                            "source".to_string()
                        } else {
                            imported
                        };
                        self.imports.push(ImportBinding {
                            source: source.clone(),
                            imported,
                            local,
                            deferred,
                        });
                    }
                }
            }
            Statement::ExportNamedDeclaration(export) => self.visit_named(export),
            Statement::ExportDefaultDeclaration(export) => self.visit_default(export),
            Statement::ExportAllDeclaration(export) => self.visit_all(export),
            _ => {}
        }
    }

    fn visit_named(&mut self, export: &ExportNamedDeclaration<'_>) {
        if let Some(source) = &export.source {
            self.visit_named_reexports(export, source.value.as_str());
            return;
        }
        for specifier in &export.specifiers {
            push_name(&mut self.exported_names, Some(&specifier.exported));
            self.exports.push(ExportBinding {
                local: module_export_name(&specifier.local),
                exported: module_export_name(&specifier.exported),
                source: false,
            });
        }
        if let Some(declaration) = &export.declaration {
            declaration_names(declaration, &mut self.exported_names);
            self.exports.extend(declaration_bindings(declaration));
        }
    }

    fn visit_named_reexports(&mut self, export: &ExportNamedDeclaration<'_>, source: &str) {
        push_unique(&mut self.import_specifiers, source);
        self.push_request(source, false);
        for specifier in &export.specifiers {
            let exported = module_export_name(&specifier.exported);
            push_unique(&mut self.exported_names, &exported);
            self.reexports.push(ReexportBinding {
                source: source.to_string(),
                imported: module_export_name(&specifier.local),
                exported,
            });
        }
    }

    fn visit_all(&mut self, export: &oxc::ast::ast::ExportAllDeclaration<'_>) {
        let source = export.source.value.to_string();
        push_unique(&mut self.import_specifiers, &source);
        self.push_request(&source, false);
        if let Some(exported) = &export.exported {
            let exported = module_export_name(exported);
            push_unique(&mut self.exported_names, &exported);
            self.reexports.push(ReexportBinding {
                source,
                imported: "*".to_string(),
                exported,
            });
        } else {
            self.reexports.push(ReexportBinding {
                source,
                imported: "*all*".to_string(),
                exported: "*all*".to_string(),
            });
        }
    }

    fn push_request(&mut self, source: &str, deferred: bool) {
        if self
            .requests
            .iter()
            .any(|request| request.source == source && request.deferred == deferred)
        {
            return;
        }
        self.requests.push(ModuleRequest {
            source: source.to_string(),
            deferred,
        });
    }

    fn visit_default(&mut self, export: &ExportDefaultDeclaration<'_>) {
        push_unique(&mut self.exported_names, "default");
        self.exports.push(ExportBinding {
            local: default_local_name(export),
            exported: "default".to_string(),
            source: false,
        });
    }
}

fn declaration_bindings(declaration: &Declaration<'_>) -> Vec<ExportBinding> {
    let mut names = Vec::new();
    declaration_names(declaration, &mut names);
    names
        .into_iter()
        .map(|name| ExportBinding {
            local: name.clone(),
            exported: name,
            source: false,
        })
        .collect()
}

fn declaration_names(declaration: &Declaration<'_>, names: &mut Vec<String>) {
    match declaration {
        Declaration::VariableDeclaration(declaration) => {
            for declarator in &declaration.declarations {
                binding_names(&declarator.id, names);
            }
        }
        Declaration::FunctionDeclaration(function) => {
            if let Some(id) = &function.id {
                push_unique(names, id.name.as_str());
            }
        }
        Declaration::ClassDeclaration(class) => {
            if let Some(id) = &class.id {
                push_unique(names, id.name.as_str());
            }
        }
        _ => {}
    }
}

fn binding_names(pattern: &BindingPattern<'_>, names: &mut Vec<String>) {
    match &pattern.kind {
        BindingPatternKind::BindingIdentifier(identifier) => {
            push_unique(names, identifier.name.as_str());
        }
        BindingPatternKind::ObjectPattern(object) => {
            for property in &object.properties {
                binding_names(&property.value, names);
            }
            if let Some(rest) = &object.rest {
                binding_names(&rest.argument, names);
            }
        }
        BindingPatternKind::ArrayPattern(array) => {
            for element in array.elements.iter().flatten() {
                binding_names(element, names);
            }
            if let Some(rest) = &array.rest {
                binding_names(&rest.argument, names);
            }
        }
        BindingPatternKind::AssignmentPattern(assignment) => binding_names(&assignment.left, names),
    }
}

fn default_local_name(export: &ExportDefaultDeclaration<'_>) -> String {
    match &export.declaration {
        ExportDefaultDeclarationKind::FunctionDeclaration(function) => function
            .id
            .as_ref()
            .map_or_else(|| "default".to_string(), |id| id.name.to_string()),
        ExportDefaultDeclarationKind::ClassDeclaration(class) => class
            .id
            .as_ref()
            .map_or_else(|| "default".to_string(), |id| id.name.to_string()),
        _ => "default".to_string(),
    }
}

fn push_name(names: &mut Vec<String>, name: Option<&ModuleExportName<'_>>) {
    if let Some(name) = name {
        let value = match name {
            ModuleExportName::IdentifierName(identifier) => identifier.name.as_str(),
            ModuleExportName::IdentifierReference(identifier) => identifier.name.as_str(),
            ModuleExportName::StringLiteral(literal) => literal.value.as_str(),
        };
        push_unique(names, value);
    }
}

fn module_export_name(name: &ModuleExportName<'_>) -> String {
    match name {
        ModuleExportName::IdentifierName(identifier) => identifier.name.to_string(),
        ModuleExportName::IdentifierReference(identifier) => identifier.name.to_string(),
        ModuleExportName::StringLiteral(literal) => literal.value.to_string(),
    }
}

fn dynamic_import_specifiers(statements: &[Statement<'_>]) -> Vec<String> {
    struct Finder {
        specifiers: Vec<String>,
    }
    impl<'a> oxc::ast::visit::Visit<'a> for Finder {
        fn visit_import_expression(&mut self, import: &oxc::ast::ast::ImportExpression<'a>) {
            if let oxc::ast::ast::Expression::StringLiteral(literal) = &import.source {
                push_unique(&mut self.specifiers, literal.value.as_str());
            }
        }
    }
    let mut finder = Finder {
        specifiers: Vec::new(),
    };
    for statement in statements {
        oxc::ast::visit::walk::walk_statement(&mut finder, statement);
    }
    finder.specifiers
}

fn statement_has_tla(statement: &Statement<'_>) -> bool {
    struct Finder {
        found: bool,
    }
    impl<'a> oxc::ast::visit::Visit<'a> for Finder {
        fn visit_await_expression(&mut self, _: &oxc::ast::ast::AwaitExpression<'a>) {
            self.found = true;
        }
        fn visit_function(
            &mut self,
            _: &oxc::ast::ast::Function<'a>,
            _: oxc::syntax::scope::ScopeFlags,
        ) {
        }
        fn visit_arrow_function_expression(
            &mut self,
            _: &oxc::ast::ast::ArrowFunctionExpression<'a>,
        ) {
        }
    }
    let mut finder = Finder { found: false };
    oxc::ast::visit::walk::walk_statement(&mut finder, statement);
    finder.found
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_string());
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn import_defer_keeps_eager_and_deferred_specifiers() {
        let metadata = crate::reduce::inspect_module_source(
            "import \"./setup.js\";\nimport defer * as ns from \"./dep.js\";\n",
        )
        .expect("inspect");
        assert!(metadata.import_specifiers.iter().any(|s| s == "./setup.js"));
        assert!(metadata.import_specifiers.iter().any(|s| s == "./dep.js"));
        assert!(metadata.imports.iter().any(|binding| binding.deferred));
    }

    #[test]
    fn import_attributes_record_type_text() {
        let metadata = crate::reduce::inspect_module_source(
            "import value from \"./a.js\" with { type: \"text\" };\n",
        )
        .expect("inspect");
        assert!(
            metadata
                .import_types
                .iter()
                .any(|(source, attribute)| source == "./a.js" && attribute == "type=text"),
            "{:?}",
            metadata.import_types
        );
    }
}
