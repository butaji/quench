use super::class_method_name;
use oxc_ast::ast::{MethodDefinition, MethodDefinitionKind, PropertyKey};

/// The name of the function a class element's method, getter or setter makes: its key's,
/// `get ` or `set ` before an accessor's; none for a constructor.
pub(in crate::compile) fn class_method_function_name(method: &MethodDefinition<'_>) -> Option<String> {
    if method.kind == MethodDefinitionKind::Constructor {
        return None;
    }
    let name = match &method.key {
        PropertyKey::PrivateIdentifier(identifier) => Some(format!("#{}", identifier.name.as_str())),
        _ => class_method_name(&method.key),
    };
    match (method.kind, name) {
        (MethodDefinitionKind::Get, Some(name)) => Some(format!("get {name}")),
        (MethodDefinitionKind::Set, Some(name)) => Some(format!("set {name}")),
        (_, name) => name,
    }
}
