use super::{Analyzer, AnalyzeError, ClassDecl, Type, TypeMember, VariableDecl};
use crate::transpile::hir::FunctionDecl;

impl Analyzer {
    pub(crate) fn validate_function_signature(&mut self, func: &FunctionDecl) {
        if let Some(ret_type) = &func.return_type {
            self.validate_type_compatibility(ret_type, "return type");
        }
        for param in &func.params {
            if let Some(param_type) = &param.type_ {
                self.validate_type_compatibility(param_type, &format!("parameter '{}'", param.name));
            }
        }
        if func.throws {
            if let Some(err_type) = &func.error_type {
                self.validate_type_compatibility(err_type, "error type");
            }
        }
    }

    pub(crate) fn analyze_function_body(&mut self, func: &FunctionDecl) {
        if let Some(body) = &func.body {
            for stmt in &body.0 {
                self.analyze_stmt(stmt);
            }
        }
    }

    pub(crate) fn validate_type_compatibility(&mut self, ty: &Type, context: &str) {
        match ty {
            Type::Ref { name, generics } => self.validate_ref_type(name, generics, context),
            Type::Union { types } | Type::Intersection { types } => self.validate_type_list(types, context),
            Type::Array { elem } => self.validate_type_compatibility(elem, context),
            Type::Function { params, ret } => self.validate_function_type(params, ret, context),
            Type::Object { members } => self.validate_object_members(members),
            Type::Conditional { check, extends, true_type, false_type } => {
                self.validate_conditional_type(check, extends, true_type, false_type, context);
            }
            Type::Mapped { from, to } | Type::Index { obj: from, index: to } => {
                self.validate_mapped_type(from, to, context)
            }
            _ => {}
        }
    }

    pub(crate) fn validate_mapped_type(&mut self, from: &Type, to: &Type, context: &str) {
        self.validate_type_compatibility(from, context);
        self.validate_type_compatibility(to, context);
    }

    pub(crate) fn validate_type_compatible(&mut self, member: &TypeMember) {
        self.validate_type_compatibility(&member.type_, &format!("member '{}'", member.key));
    }

    pub(crate) fn validate_ref_type(&mut self, name: &str, generics: &[Type], context: &str) {
        if !self.types.contains(name) && !self.functions.contains(name) {
            self.warnings.push(format!("Unknown type reference: {}", name));
        }
        for g in generics {
            self.validate_type_compatibility(g, context);
        }
    }

    pub(crate) fn validate_type_list(&mut self, types: &[Type], context: &str) {
        for t in types {
            self.validate_type_compatibility(t, context);
        }
    }

    pub(crate) fn validate_function_type(&mut self, params: &[Type], ret: &Box<Type>, context: &str) {
        for p in params {
            self.validate_type_compatibility(p, context);
        }
        self.validate_type_compatibility(ret, context);
    }

    pub(crate) fn validate_object_members(&mut self, members: &[TypeMember]) {
        for m in members {
            self.validate_type_compatible(m);
        }
    }

    pub(crate) fn validate_conditional_type(
        &mut self,
        check: &Box<Type>,
        extends: &Box<Type>,
        true_type: &Box<Type>,
        false_type: &Box<Type>,
        context: &str,
    ) {
        self.validate_type_compatibility(check, context);
        self.validate_type_compatibility(extends, context);
        self.validate_type_compatibility(true_type, context);
        self.validate_type_compatibility(false_type, context);
    }

    pub(crate) fn validate_class_members(&mut self, class: &ClassDecl) {
        for member in &class.members {
            if let Some(ty) = &member.type_ {
                self.validate_type_compatibility(ty, &format!("class member '{}'", member.name));
            }
        }
    }

    pub(crate) fn validate_variable_decl(&mut self, var: &VariableDecl) {
        if let Some(ty) = &var.type_ {
            self.validate_type_compatibility(ty, &format!("variable '{}'", var.name));
        }
        if let Some(init) = &var.init {
            if let Some(expected) = &var.type_ {
                let actual = self.infer_type(init);
                if !self.types_compatible(expected, &actual) {
                    self.errors.push(AnalyzeError::TypeError {
                        message: format!("Variable '{}' has type {:?} but is initialized with {:?}", var.name, expected, actual),
                        location: format!("variable '{}'", var.name),
                    });
                }
            }
        }
    }
}
