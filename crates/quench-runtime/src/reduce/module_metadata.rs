use oxc::ast::ast::{
    BindingPattern, BindingPatternKind, Declaration, ExportDefaultDeclaration,
    ExportNamedDeclaration, ImportPhase, ModuleExportName, Statement,
};
use oxc::ast::ast::{Expression, ImportExpression, Program};

/// Static module edges and names discovered directly from OXC's module AST.
///
/// This is metadata only: it does not alter reduction or provide module
/// execution semantics. Hosts can use it to construct a linker input.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ModuleMetadata {
    pub import_specifiers: Vec<String>,
    pub imports: Vec<ImportBinding>,
    pub import_attributes: Vec<(String, String)>,
    pub deferred_imports: Vec<String>,
    pub exports: Vec<ExportBinding>,
    pub reexports: Vec<ReexportBinding>,
    pub exported_names: Vec<String>,
    pub dynamic_imports: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBinding {
    pub source: String,
    pub imported: String,
    pub local: String,
    pub deferred: bool,
}

/// One local binding exposed under an exported module name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportBinding {
    pub local: String,
    pub exported: String,
}

/// A binding re-exposed from another module without a local runtime slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReexportBinding {
    pub source: String,
    pub imported: String,
    pub exported: String,
}

impl ModuleMetadata {
    pub(crate) fn from_program(program: &Program<'_>) -> Self {
        let mut metadata = Self::from_statements(&program.body);
        let mut collector = DynamicImportCollector::default();
        oxc::ast::visit::walk::walk_program(&mut collector, program);
        metadata.dynamic_imports = collector.specifiers;
        metadata
    }

    pub(crate) fn from_statements(statements: &[Statement<'_>]) -> Self {
        let mut metadata = Self::default();
        for statement in statements {
            metadata.visit_statement(statement);
        }
        metadata
    }

    fn visit_statement(&mut self, statement: &Statement<'_>) {
        match statement {
            Statement::ImportDeclaration(import) => {
                let source = import.source.value.to_string();
                push_unique(&mut self.import_specifiers, &source);
                if matches!(import.phase, Some(ImportPhase::Defer)) {
                    push_unique(&mut self.deferred_imports, &source);
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
                        self.imports.push(ImportBinding {
                            source: source.clone(),
                            imported,
                            local,
                            deferred: matches!(import.phase, Some(ImportPhase::Defer)),
                        });
                    }
                }
                if let Some(with_clause) = &import.with_clause {
                    for attribute in &with_clause.with_entries {
                        let key = match &attribute.key {
                            oxc::ast::ast::ImportAttributeKey::Identifier(key) => {
                                key.name.to_string()
                            }
                            oxc::ast::ast::ImportAttributeKey::StringLiteral(key) => {
                                key.value.to_string()
                            }
                        };
                        self.import_attributes
                            .push((source.clone(), format!("{key}={}", attribute.value.value)));
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
            });
        }
        if let Some(declaration) = &export.declaration {
            declaration_names(declaration, &mut self.exported_names);
            self.exports.extend(declaration_bindings(declaration));
        }
    }

    fn visit_named_reexports(&mut self, export: &ExportNamedDeclaration<'_>, source: &str) {
        push_unique(&mut self.import_specifiers, source);
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

    fn visit_default(&mut self, _export: &ExportDefaultDeclaration<'_>) {
        push_unique(&mut self.exported_names, "default");
        self.exports.push(ExportBinding {
            local: "default".to_string(),
            exported: "default".to_string(),
        });
    }
}

#[derive(Default)]
struct DynamicImportCollector {
    specifiers: Vec<String>,
}

impl<'a> oxc::ast::visit::Visit<'a> for DynamicImportCollector {
    fn visit_import_expression(&mut self, import: &ImportExpression<'a>) {
        if let Expression::StringLiteral(source) = &import.source {
            push_unique(&mut self.specifiers, source.value.as_str());
        }
        oxc::ast::visit::walk::walk_import_expression(self, import);
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

fn push_unique(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_string());
    }
}
