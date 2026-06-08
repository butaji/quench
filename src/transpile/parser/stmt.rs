//! Statement conversion - consolidated implementations
//!
//! These functions are consolidated into this single file
//! to avoid module declaration issues.

use crate::transpile::hir::{self, Expr, Stmt, VariableKind, ForInit, SwitchCase};
use super::expr::{convert_expr, convert_binding_pattern};
use super::stmt_decl::var_to_decl;
use super::stmt_class::class_to_hir;
use oxc_ast::ast::*;

/// Convert statement to HIR
pub fn convert_statement(s: &Statement) -> Option<hir::ModuleItem> {
    match s {
        Statement::ImportDeclaration(i)=>Some(hir::ModuleItem::Import(import_to_hir(i))),
        Statement::ExportNamedDeclaration(e)=>convert_export_named(e),
        Statement::ExportDefaultDeclaration(e)=>{
            if let Some(expr)=export_default_kind_to_expr(&e.declaration){
                Some(hir::ModuleItem::Stmt(hir::Stmt::ExportDefault{expr}))
            } else {
                None
            }
        }
        Statement::ExportAllDeclaration(e)=>{
            Some(hir::ModuleItem::Stmt(hir::Stmt::ExportNamed{
                specifiers: vec![hir::Export::All{ source: e.source.value.to_string() }]
            }))
        }
        Statement::TSExportAssignment(_)=>None,
        Statement::TSNamespaceExportDeclaration(_)=>None,
        Statement::FunctionDeclaration(f)=>{
            if let Some(decl)=func_to_decl(f){
                Some(hir::ModuleItem::Decl(hir::Decl::Function(decl)))
            } else {
                None
            }
        }
        Statement::ClassDeclaration(c)=>{
            Some(hir::ModuleItem::Decl(class_to_hir(c)))
        }
        Statement::VariableDeclaration(v)=>{
            // Module-level variable declarations become Decl::Variable
            decl_var(v).map(hir::ModuleItem::Decl)
        }
        _=>Some(hir::ModuleItem::Stmt(stmt_to_hir_stmt(s).ok()?)),
    }
}

fn export_default_kind_to_expr(kind: &ExportDefaultDeclarationKind) -> Option<Expr> {
    match kind {
        ExportDefaultDeclarationKind::FunctionDeclaration(f) => {
            if let Some(decl) = func_to_decl(f) {
                Some(Expr::Function(decl))
            } else {
                None
            }
        }
        ExportDefaultDeclarationKind::ClassDeclaration(_) => {
            None
        }
        ExportDefaultDeclarationKind::TSInterfaceDeclaration(_) => None,
        _ => {
            if let Some(expr) = kind.as_expression() {
                convert_expr(expr).ok()
            } else {
                None
            }
        }
    }
}

fn export_name_str(name: &ModuleExportName) -> String {
    if let ModuleExportName::IdentifierName(i)=name{return i.name.to_string();}
    if let ModuleExportName::IdentifierReference(i)=name{return i.name.to_string();}
    if let ModuleExportName::StringLiteral(s)=name{return s.value.to_string();}
    String::new()
}

fn convert_export_named(e: &ExportNamedDeclaration) -> Option<hir::ModuleItem> {
    if let Some(source)=&e.source {
        return convert_reexport(source, &e.specifiers);
    }
    if let Some(declaration)=&e.declaration {
        return convert_export_decl(&declaration).map(|items| {
            items.into_iter().next().unwrap_or(hir::ModuleItem::Stmt(hir::Stmt::Empty))
        });
    }
    convert_named_exports(&e.specifiers)
}

fn convert_reexport(source: &StringLiteral, specifiers: &[ExportSpecifier]) -> Option<hir::ModuleItem> {
    if specifiers.is_empty() {
        Some(hir::ModuleItem::Stmt(hir::Stmt::ExportNamed{
            specifiers: vec![hir::Export::All{ source: source.value.to_string() }]
        }))
    } else {
        let names: Vec<String>=specifiers.iter().map(|s|export_name_str(&s.local)).collect();
        Some(hir::ModuleItem::Stmt(hir::Stmt::ExportNamed{
            specifiers: vec![hir::Export::ReExport{ source: source.value.to_string(), names }]
        }))
    }
}

fn convert_named_exports(specifiers: &[ExportSpecifier]) -> Option<hir::ModuleItem> {
    let exports: Vec<hir::Export>=specifiers.iter().map(|s|{
        let local=export_name_str(&s.local);
        let exported=export_name_str(&s.exported);
        if local==exported{ hir::Export::Named{name: local} }
        else { hir::Export::NamedRenamed{local, exported} }
    }).collect();
    if exports.is_empty() { None } else { Some(hir::ModuleItem::Stmt(hir::Stmt::ExportNamed{specifiers: exports})) }
}

/// Convert an export declaration to HIR module items
fn convert_export_decl(declaration: &oxc_ast::ast::Declaration) -> Option<Vec<hir::ModuleItem>> {
    match declaration {
        oxc_ast::ast::Declaration::VariableDeclaration(v) => {
            Some(var_to_decl(v).into_iter().map(hir::ModuleItem::Decl).collect())
        }
        oxc_ast::ast::Declaration::FunctionDeclaration(f) => {
            func_to_decl(f).map(|decl| vec![hir::ModuleItem::Decl(hir::Decl::Function(decl))])
        }
        oxc_ast::ast::Declaration::ClassDeclaration(c) => {
            Some(vec![hir::ModuleItem::Decl(class_to_hir(c))])
        }
        _ => None,
    }
}

/// Convert statement to HIR
pub fn stmt_to_hir_stmt(s: &Statement) -> Result<Stmt, ()> {
    match s {
        Statement::ExpressionStatement(e)=>stmt_expr(e),
        Statement::IfStatement(i)=>stmt_if(i),
        Statement::BlockStatement(b)=>stmt_block(b),
        Statement::ReturnStatement(r)=>stmt_return(r),
        Statement::SwitchStatement(sw)=>stmt_switch(sw),
        Statement::TryStatement(t)=>stmt_try(t),
        Statement::ThrowStatement(t)=>stmt_throw(t),
        Statement::BreakStatement(_)=>Ok(hir::Stmt::Break{label:None}),
        Statement::ContinueStatement(_)=>Ok(hir::Stmt::Continue{label:None}),
        Statement::LabeledStatement(l)=>stmt_labeled(l),
        Statement::WhileStatement(w)=>stmt_while(w),
        Statement::DoWhileStatement(d)=>stmt_do_while(d),
        Statement::ForStatement(f)=>stmt_for(f),
        Statement::ForInStatement(f)=>stmt_for_in(f),
        Statement::ForOfStatement(f)=>stmt_for_of(f),
        Statement::VariableDeclaration(v)=>stmt_var(v),
        Statement::FunctionDeclaration(f)=>stmt_func(f),
        Statement::ClassDeclaration(c)=>stmt_class(c),
        Statement::TSEnumDeclaration(e)=>Ok(hir::Stmt::Enum(conv_enum(e))),
        _=>Ok(hir::Stmt::Empty),
    }
}

/// Convert TypeScript enum declaration
fn conv_enum(e: &TSEnumDeclaration) -> hir::EnumDecl {
    use super::expr::convert_expr;
    use oxc_ast::ast::TSEnumMemberName;
    
    let members: Vec<hir::EnumMember> = e.body.members.iter().filter_map(|m| {
        let key = match &m.id {
            TSEnumMemberName::Identifier(id) => id.name.to_string(),
            TSEnumMemberName::String(s) => s.value.to_string(),
            _ => return None, // Computed names not supported
        };
        let value = m.initializer.as_ref().and_then(|init| {
            // Try to convert the initializer expression to a literal value
            if let Ok(expr) = convert_expr(init) {
                match expr {
                    hir::Expr::Number(n) => Some(hir::EnumValue::Number(n)),
                    hir::Expr::String(s) => Some(hir::EnumValue::String(s)),
                    _ => None,
                }
            } else {
                None
            }
        });
        Some(hir::EnumMember { key, value })
    }).collect();

    hir::EnumDecl {
        name: e.id.name.to_string(),
        members,
        is_const: e.r#const,
    }
}

fn stmt_expr(e: &ExpressionStatement) -> Result<Stmt, ()> {
    Ok(hir::Stmt::Expr{expr:convert_expr(&e.expression).map_err(|_|())?})
}

fn stmt_if(s: &IfStatement) -> Result<Stmt, ()> {
    Ok(hir::Stmt::If{
        test:convert_expr(&s.test).map_err(|_|())?,
        consequent:Box::new(stmt_to_hir_stmt(&s.consequent)?),
        alternate:s.alternate.as_ref().map(|a| stmt_to_hir_stmt(a).ok()).flatten().map(Box::new),
    })
}

fn stmt_block(b: &BlockStatement) -> Result<Stmt, ()> {
    Ok(hir::Stmt::Block{stmts:b.body.iter().filter_map(|s|stmt_to_hir_stmt(s).ok()).collect()})
}

fn stmt_return(r: &ReturnStatement) -> Result<Stmt, ()> {
    Ok(hir::Stmt::Return{arg:r.argument.as_ref().and_then(|a|convert_expr(a).ok())})
}

fn stmt_switch(s: &SwitchStatement) -> Result<Stmt, ()> {
    let discriminant=convert_expr(&s.discriminant).map_err(|_|())?;
    let cases:Vec<SwitchCase>=s.cases.iter().map(|c|SwitchCase{
        test:c.test.as_ref().and_then(|t|convert_expr(t).ok()),
        consequent:c.consequent.iter().filter_map(|s|stmt_to_hir_stmt(s).ok()).collect(),
    }).collect();
    Ok(hir::Stmt::Switch{discriminant, cases})
}

fn stmt_try(t: &TryStatement) -> Result<Stmt, ()> {
    let block=hir::Block(t.block.body.iter().filter_map(|s|stmt_to_hir_stmt(s).ok()).collect());
    let handler=t.handler.as_ref().map(|h|hir::CatchClause{
        param:catch_param(h),
        body:Box::new(hir::Block(h.body.body.iter().filter_map(|s|stmt_to_hir_stmt(s).ok()).collect())),
    });
    let finalizer=t.finalizer.as_ref().map(|f|hir::Block(f.body.iter().filter_map(|s|stmt_to_hir_stmt(s).ok()).collect()));
    Ok(hir::Stmt::Try{block, handler, finalizer})
}

fn catch_param(h: &CatchClause) -> String {
    h.param.as_ref().and_then(|p|{
        if let BindingPattern::BindingIdentifier(i)=&p.pattern{Some(i.name.to_string())}else{None}
    }).unwrap_or_default()
}

fn stmt_throw(t: &ThrowStatement) -> Result<Stmt, ()> {
    Ok(hir::Stmt::Throw{arg:convert_expr(&t.argument).map_err(|_|())?})
}

fn stmt_labeled(l: &LabeledStatement) -> Result<Stmt, ()> {
    Ok(hir::Stmt::Labeled{label:l.label.name.to_string(), body:Box::new(stmt_to_hir_stmt(&l.body)?)})
}

fn stmt_while(w: &WhileStatement) -> Result<Stmt, ()> {
    Ok(hir::Stmt::While{test:convert_expr(&w.test).map_err(|_|())?, body:Box::new(stmt_to_hir_stmt(&w.body)?)})
}

fn stmt_do_while(d: &DoWhileStatement) -> Result<Stmt, ()> {
    Ok(hir::Stmt::DoWhile{body:Box::new(stmt_to_hir_stmt(&d.body)?), test:convert_expr(&d.test).map_err(|_|())?})
}

fn stmt_for(f: &ForStatement) -> Result<Stmt, ()> {
    let init=for_init(f.init.as_ref());
    let test=f.test.as_ref().and_then(|t|convert_expr(t).ok());
    let update=f.update.as_ref().and_then(|u|convert_expr(u).ok());
    let body=Box::new(stmt_to_hir_stmt(&f.body)?);
    Ok(hir::Stmt::For{init: Some(init), test, update, body})
}

fn for_init(init: Option<&ForStatementInit>) -> ForInit {
    if let Some(ForStatementInit::VariableDeclaration(v))=init{
        let kind=var_kind(v.kind);
        let vars=v.declarations.iter().filter_map(|d|{
            let name=binding_name(&d.id);
            let init=d.init.as_ref().and_then(|e|convert_expr(e).ok());
            Some((name, init))
        }).collect();
        return ForInit::Variable(kind, vars);
    }
    if let Some(i)=init{
        if let Some(expr)=i.as_expression(){
            if let Ok(e)=convert_expr(expr){return ForInit::Expr(Box::new(e));}
        }
    }
    ForInit::Variable(VariableKind::Let, vec![])
}

fn stmt_for_in(f: &ForInStatement) -> Result<Stmt, ()> {
    let left=for_left(&f.left);
    let right=convert_expr(&f.right).map_err(|_|())?;
    let body=Box::new(stmt_to_hir_stmt(&f.body)?);
    Ok(hir::Stmt::ForIn{left, right, body})
}

fn stmt_for_of(f: &ForOfStatement) -> Result<Stmt, ()> {
    let left=for_left(&f.left);
    let right=convert_expr(&f.right).map_err(|_|())?;
    let body=Box::new(stmt_to_hir_stmt(&f.body)?);
    Ok(hir::Stmt::ForOf{left, right, body, is_await:f.r#await})
}

fn for_left(left: &ForStatementLeft) -> ForInit {
    if let ForStatementLeft::VariableDeclaration(v)=left{
        let kind=var_kind(v.kind);
        let vars=v.declarations.iter().filter_map(|d|{
            let name=binding_name(&d.id);
            Some((name, None))
        }).collect();
        return ForInit::Variable(kind, vars);
    }
    if let ForStatementLeft::AssignmentTargetIdentifier(id)=left{
        return ForInit::Expr(Box::new(hir::Expr::Ident{name:id.name.to_string()}));
    }
    ForInit::Variable(VariableKind::Let, vec![])
}

fn stmt_var(v: &VariableDeclaration) -> Result<Stmt, ()> {
    let kind=var_kind(v.kind);
    if let Some(decl)=v.declarations.first(){
        let name=binding_name(&decl.id);
        let init=decl.init.as_ref().and_then(|e|convert_expr(e).ok());
        let pattern=convert_binding_pattern(&decl.id);
        Ok(hir::Stmt::Variable(hir::VariableDecl{name, kind, type_:None, init, pattern}))
    } else {
        Ok(hir::Stmt::Empty)
    }
}

fn decl_var(v: &VariableDeclaration) -> Option<hir::Decl> {
    let kind=var_kind(v.kind);
    if let Some(decl)=v.declarations.first(){
        let name=binding_name(&decl.id);
        let init=decl.init.as_ref().and_then(|e|convert_expr(e).ok());
        let pattern=convert_binding_pattern(&decl.id);
        Some(hir::Decl::Variable(hir::VariableDecl{name, kind, type_:None, init, pattern}))
    } else {
        None
    }
}

fn stmt_func(f: &Function) -> Result<Stmt, ()> {
    let decl=func_to_decl(f).ok_or(())?;
    Ok(hir::Stmt::FunctionDecl(decl))
}

fn stmt_class(c: &Class) -> Result<Stmt, ()> {
    let decl=class_to_hir(c);
    if let hir::Decl::Class(cd)=decl{
        Ok(hir::Stmt::Class(cd))
    } else {
        Ok(hir::Stmt::Empty)
    }
}

fn var_kind(k: VariableDeclarationKind) -> VariableKind {
    match k {
        VariableDeclarationKind::Const=>VariableKind::Const,
        VariableDeclarationKind::Let=>VariableKind::Let,
        VariableDeclarationKind::Var=>VariableKind::Var,
        _=>VariableKind::Let,
    }
}

fn binding_name(pat: &BindingPattern) -> String {
    if let BindingPattern::BindingIdentifier(i)=pat{i.name.to_string()}else{String::new()}
}

fn func_to_decl(f: &Function) -> Option<hir::FunctionDecl> {
    let params: Vec<hir::Param> = f.params.items.iter().filter_map(|p| {
        let name = match &p.pattern {
            BindingPattern::BindingIdentifier(i) => i.name.to_string(),
            _ => String::new(),
        };
        let pat = convert_binding_pattern(&p.pattern);
        Some(hir::Param {
            name,
            type_: None,
            default: None,
            optional: false,
            pattern: pat,
            ownership: hir::Ownership::Owned,
        })
    }).collect();
    let body = f.body.as_ref().map(|b| {
        hir::Block(b.statements.iter().filter_map(|s| stmt_to_hir_stmt(s).ok()).collect())
    });
    Some(hir::FunctionDecl {
        name: f.id.as_ref().map(|i| i.name.to_string()).unwrap_or_default(),
        generics: vec![],
        params,
        return_type: None,
        body,
        is_async: f.r#async,
        is_generator: f.generator,
        decorators: vec![],
        throws: false,
        error_type: None,
    })
}

fn import_spec_to_hir(s: &ImportDeclarationSpecifier) -> hir::ImportSpecifier {
    match s {
        ImportDeclarationSpecifier::ImportSpecifier(s) => {
            let imported_name = match &s.imported {
                ModuleExportName::IdentifierName(i) => i.name.to_string(),
                ModuleExportName::IdentifierReference(i) => i.name.to_string(),
                ModuleExportName::StringLiteral(s) => s.value.to_string(),
            };
            let local_name = s.local.name.to_string();
            let alias = if imported_name == local_name { None } else { Some(local_name) };
            hir::ImportSpecifier::Named { name: imported_name, alias }
        }
        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
            hir::ImportSpecifier::Default { name: s.local.name.to_string() }
        }
        ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
            hir::ImportSpecifier::Namespace { name: s.local.name.to_string() }
        }
    }
}

fn is_all_type_only(specs: &[ImportDeclarationSpecifier]) -> bool {
    use oxc_ast::ast::ImportOrExportKind;
    !specs.is_empty()
        && specs.iter().all(|s| {
            matches!(
                s,
                ImportDeclarationSpecifier::ImportSpecifier(s)
                    if s.import_kind == ImportOrExportKind::Type
            )
        })
}

fn import_to_hir(i: &ImportDeclaration) -> hir::Import {
    use oxc_ast::ast::ImportOrExportKind;
    let specs = i.specifiers.as_ref().map_or(vec![], |v| {
        v.iter().map(import_spec_to_hir).collect()
    });
    let type_only = i.import_kind == ImportOrExportKind::Type
        || i.specifiers.as_ref().map_or(false, |s| is_all_type_only(s));
    hir::Import {
        source: i.source.value.to_string(),
        specifiers: specs,
        type_only,
    }
}
