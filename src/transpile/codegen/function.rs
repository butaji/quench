//! Function generation

use anyhow::Result;
use crate::transpile::codegen::CodeGenerator;
use crate::transpile::hir::*;

pub struct FnGen;

impl FnGen {
    pub fn generate_function(cg: &mut CodeGenerator, decl: &FunctionDecl, is_component: bool) -> Result<String> {
        let async_prefix = if decl.is_async { "async " } else { "" };
        let mut output = String::new();

        // Generate struct for PageProps if this is a component
        if is_component {
            if let Some(param) = decl.params.first() {
                if let Some(Type::Ref { name, generics }) = &param.type_ {
                    if name == "PageProps" && generics.len() >= 1 {
                        output.push_str(&Self::generate_page_props_struct(cg, &decl.name, generics)?);
                        output.push('\n');
                    }
                }
            }
        }

        // Function signature
        let params: Vec<String> = decl.params.iter().map(|p| {
            let name = cg.to_snake_case(&p.name);
            let type_str = p.type_.as_ref().map(|t| cg.type_to_rust(t)).unwrap_or_else(|| "serde_json::Value".to_string());
            if p.optional {
                format!("{}: Option<{}>", name, type_str)
            } else {
                format!("{}: {}", name, type_str)
            }
        }).collect();
        let params_str = params.join(", ");

        let return_type = if is_component {
            "VNode".to_string()
        } else {
            decl.return_type.as_ref().map(|t| cg.type_to_rust(t)).unwrap_or_else(|| "serde_json::Value".to_string())
        };

        output.push_str(&format!("pub {}fn {}({}) -> {} {{\n", async_prefix, decl.name, params_str, return_type));

        // Function body
        if let Some(body) = &decl.body {
            cg.indent += 1;
            for stmt in &body.0 {
                let stmt_code = cg.stmt_to_rust(stmt)?;
                if !stmt_code.trim().is_empty() {
                    output.push_str(&format!("{}{}\n", cg.indent_str(), stmt_code.trim()));
                }
            }
            cg.indent -= 1;
        }

        output.push_str("}\n");
        Ok(output)
    }

    fn generate_page_props_struct(cg: &CodeGenerator, _name: &str, generics: &[Type]) -> Result<String> {
        let _data_type = generics.first().map(|g| cg.type_to_rust(g)).unwrap_or_else(|| "()".to_string());
        Ok(format!(
            "#[derive(Serialize, Deserialize, Debug, Clone)]\npub struct PageProps<T> {{\n    pub params: HashMap<String, String>,\n    pub url: String,\n    pub data: T,\n}}\n"
        ))
    }

    pub fn generate_handler_method(cg: &CodeGenerator, method: &str, value: &Expr) -> Result<String> {
        let method_lower = method.to_lowercase();
        let handler_name = format!("handle_{}", method_lower);

        let (is_async, params, body) = Self::extract_handler_body(value)?;

        let async_prefix = if is_async { "async " } else { "" };
        let mut ordered_params: Vec<_> = params.iter().collect();
        ordered_params.sort_by(|a, b| {
            let a_is_request = a.type_.as_ref().map(|t| matches!(t, Type::Ref { name, .. } if name == "Request")).unwrap_or(false);
            let b_is_request = b.type_.as_ref().map(|t| matches!(t, Type::Ref { name, .. } if name == "Request")).unwrap_or(false);
            match (a_is_request, b_is_request) {
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                _ => std::cmp::Ordering::Equal,
            }
        });
        let params_str = ordered_params.iter().map(|p| {
            let raw = p.name.trim_start_matches('_');
            let name = cg.to_snake_case(raw);
            p.type_.as_ref()
                .map(|t| format!("{}: {}", name, cg.type_to_rust(t)))
                .unwrap_or_else(|| name)
        }).collect::<Vec<_>>().join(", ");

        let body_str = Self::body_to_string(cg, &body);

        Ok(format!(
            r#"pub {}fn {}({}) -> Response<Body> {{
    {}
}}"#,
            async_prefix, handler_name, params_str, body_str
        ))
    }

    fn extract_handler_body(value: &Expr) -> Result<(bool, Vec<Param>, Box<Stmt>)> {
        match value {
            Expr::Arrow { params, body: _, is_async } => {
                let body_stmt = Stmt::Return { arg: None };
                Ok((*is_async, params.clone(), Box::new(body_stmt)))
            }
            Expr::Function { decl } => {
                let body_stmt = decl.body.as_ref()
                    .map(|b| Box::new(Stmt::Block(b.0.clone())))
                    .unwrap_or_else(|| Box::new(Stmt::Block(vec![])));
                Ok((decl.is_async, decl.params.clone(), body_stmt))
            }
            _ => Ok((false, vec![], Box::new(Stmt::Block(vec![])))),
        }
    }

    fn body_to_string(cg: &CodeGenerator, body: &Stmt) -> String {
        match body {
            Stmt::Block(_) => "{}".to_string(),
            Stmt::Return { arg } => {
                arg.as_ref()
                    .map(|e| format!("return {};", cg.expr_to_rust(e)))
                    .unwrap_or_else(|| "Response::builder().status(501).body(Body::from(\"Not Implemented\")).unwrap()".to_string())
            }
            _ => "Response::builder().status(501).body(Body::from(\"Not Implemented\")).unwrap()".to_string(),
        }
    }
}
