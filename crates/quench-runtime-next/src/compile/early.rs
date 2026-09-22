use oxc_ast::ast::{BindingPattern, Program, Statement, VariableDeclarationKind};
use rustc_hash::FxHashSet;

pub(super) fn block_early_error(program: &Program<'_>) -> Option<String> {
    validate_nested(&program.body)
}

fn validate_nested(statements: &[Statement<'_>]) -> Option<String> {
    for statement in statements {
        match statement {
            Statement::BlockStatement(block) => {
                if let Some(error) = validate_block(&block.body) {
                    return Some(error);
                }
            }
            Statement::IfStatement(statement) => {
                if let Some(error) = validate_loop_body(&statement.consequent) {
                    return Some(error);
                }
                if let Some(alternate) = &statement.alternate
                    && let Some(error) = validate_loop_body(alternate)
                {
                    return Some(error);
                }
            }
            Statement::WhileStatement(statement) => {
                if let Some(error) = validate_loop_body(&statement.body) {
                    return Some(error);
                }
            }
            Statement::DoWhileStatement(statement) => {
                if let Some(error) = validate_loop_body(&statement.body) {
                    return Some(error);
                }
            }
            Statement::ForStatement(statement) => {
                if let Some(error) = validate_loop_body(&statement.body) {
                    return Some(error);
                }
            }
            Statement::ForInStatement(statement) => {
                if let Some(error) = validate_loop_body(&statement.body) {
                    return Some(error);
                }
            }
            Statement::ForOfStatement(statement) => {
                if let Some(error) = validate_loop_body(&statement.body) {
                    return Some(error);
                }
            }
            Statement::FunctionDeclaration(function) => {
                if let Some(body) = &function.body
                    && let Some(error) = validate_nested(&body.statements)
                {
                    return Some(error);
                }
            }
            Statement::TryStatement(statement) => {
                if let Some(error) = validate_nested(&statement.block.body) {
                    return Some(error);
                }
                if let Some(handler) = &statement.handler
                    && let Some(error) = validate_block(&handler.body.body)
                {
                    return Some(error);
                }
                if let Some(finalizer) = &statement.finalizer
                    && let Some(error) = validate_block(&finalizer.body)
                {
                    return Some(error);
                }
            }
            _ => {}
        }
    }
    None
}

fn validate_loop_body(body: &Statement<'_>) -> Option<String> {
    if matches!(body, Statement::FunctionDeclaration(_)) {
        return Some("SyntaxError: function declaration is not a loop body".into());
    }
    validate_nested(std::slice::from_ref(body))
}

fn validate_block(statements: &[Statement<'_>]) -> Option<String> {
    let mut lexical = FxHashSet::default();
    let mut variables = FxHashSet::default();
    for statement in statements {
        match statement {
            Statement::VariableDeclaration(declaration)
                if declaration.kind == VariableDeclarationKind::Var =>
            {
                for item in &declaration.declarations {
                    collect_pattern_names(&item.id, &mut variables);
                }
            }
            Statement::VariableDeclaration(declaration) => {
                for item in &declaration.declarations {
                    let mut names = Vec::new();
                    collect_pattern_names(&item.id, &mut names);
                    for name in names {
                        if !lexical.insert(name.clone()) {
                            return Some("SyntaxError: duplicate lexical declaration".into());
                        }
                    }
                }
            }
            Statement::ClassDeclaration(class) => {
                if let Some(identifier) = &class.id
                    && !lexical.insert(identifier.name.to_string())
                {
                    return Some("SyntaxError: duplicate lexical declaration".into());
                }
            }
            Statement::FunctionDeclaration(function) => {
                if let Some(identifier) = &function.id
                    && !lexical.insert(identifier.name.to_string())
                {
                    return Some("SyntaxError: duplicate lexical declaration".into());
                }
            }
            _ => {}
        }
    }
    collect_nested_vars(statements, &mut variables);
    if lexical.iter().any(|name| variables.contains(name)) {
        return Some("SyntaxError: block lexical declaration conflicts with var".into());
    }
    validate_nested(statements)
}

fn collect_nested_vars(statements: &[Statement<'_>], names: &mut FxHashSet<String>) {
    for statement in statements {
        match statement {
            Statement::VariableDeclaration(declaration)
                if declaration.kind == VariableDeclarationKind::Var =>
            {
                for item in &declaration.declarations {
                    let mut declared = Vec::new();
                    collect_pattern_names(&item.id, &mut declared);
                    names.extend(declared);
                }
            }
            Statement::BlockStatement(block) => collect_nested_vars(&block.body, names),
            Statement::IfStatement(statement) => {
                collect_nested_vars(std::slice::from_ref(&statement.consequent), names);
                if let Some(alternate) = &statement.alternate {
                    collect_nested_vars(std::slice::from_ref(alternate), names);
                }
            }
            Statement::ForStatement(statement) => {
                if let Some(oxc_ast::ast::ForStatementInit::VariableDeclaration(declaration)) =
                    &statement.init
                    && declaration.kind == VariableDeclarationKind::Var
                {
                    for item in &declaration.declarations {
                        let mut declared = Vec::new();
                        collect_pattern_names(&item.id, &mut declared);
                        names.extend(declared);
                    }
                }
                collect_nested_vars(std::slice::from_ref(&statement.body), names);
            }
            Statement::WhileStatement(statement) => {
                collect_nested_vars(std::slice::from_ref(&statement.body), names)
            }
            Statement::DoWhileStatement(statement) => {
                collect_nested_vars(std::slice::from_ref(&statement.body), names)
            }
            Statement::TryStatement(statement) => {
                collect_nested_vars(&statement.block.body, names);
                if let Some(handler) = &statement.handler {
                    collect_nested_vars(&handler.body.body, names);
                }
                if let Some(finalizer) = &statement.finalizer {
                    collect_nested_vars(&finalizer.body, names);
                }
            }
            _ => {}
        }
    }
}

fn collect_pattern_names(pattern: &BindingPattern<'_>, names: &mut impl Extend<String>) {
    match pattern {
        BindingPattern::BindingIdentifier(identifier) => {
            names.extend(std::iter::once(identifier.name.to_string()));
        }
        BindingPattern::AssignmentPattern(pattern) => collect_pattern_names(&pattern.left, names),
        BindingPattern::ArrayPattern(pattern) => {
            for element in pattern.elements.iter().flatten() {
                collect_pattern_names(element, names);
            }
        }
        BindingPattern::ObjectPattern(pattern) => {
            for property in &pattern.properties {
                collect_pattern_names(&property.value, names);
            }
        }
    }
}

pub(super) fn strict_arguments_early_error(source: &str) -> bool {
    let masked = mask_literals_and_comments(source);
    let bytes = masked.as_bytes();
    let mut index = 0;
    while index + 9 <= bytes.len() {
        if bytes[index..].starts_with(b"arguments")
            && (index == 0 || !bytes[index - 1].is_ascii_alphanumeric())
            && (index + 9 == bytes.len() || !bytes[index + 9].is_ascii_alphanumeric())
        {
            let mut cursor = index + 9;
            while matches!(bytes.get(cursor), Some(b' ' | b'\t' | b'\n' | b'\r')) {
                cursor += 1;
            }
            if matches!(
                bytes.get(cursor),
                Some(b'=') | Some(b'+') | Some(b'-') | Some(b'*') | Some(b'/')
            ) || source[index.saturating_sub(7)..index].contains("delete")
            {
                return true;
            }
        }
        index += 1;
    }
    false
}

fn mask_literals_and_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut masked = bytes.to_vec();
    let mut quote = None;
    let mut index = 0;
    while index < bytes.len() {
        if let Some(delimiter) = quote {
            if bytes[index] == b'\\' && index + 1 < bytes.len() {
                masked[index] = b' ';
                masked[index + 1] = b' ';
                index += 2;
                continue;
            }
            if bytes[index] == delimiter {
                quote = None;
            } else if !matches!(bytes[index], b'\n' | b'\r') {
                masked[index] = b' ';
            }
            index += 1;
            continue;
        }
        if matches!(bytes[index], b'\'' | b'"' | b'`') {
            quote = Some(bytes[index]);
            masked[index] = b' ';
            index += 1;
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            masked[index] = b' ';
            masked[index + 1] = b' ';
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                masked[index] = b' ';
                index += 1;
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            masked[index] = b' ';
            masked[index + 1] = b' ';
            index += 2;
            while index + 1 < bytes.len() {
                if bytes[index] == b'*' && bytes[index + 1] == b'/' {
                    masked[index] = b' ';
                    masked[index + 1] = b' ';
                    index += 2;
                    break;
                }
                if !matches!(bytes[index], b'\n' | b'\r') {
                    masked[index] = b' ';
                }
                index += 1;
            }
            continue;
        }
        index += 1;
    }
    String::from_utf8(masked).unwrap_or_default()
}
