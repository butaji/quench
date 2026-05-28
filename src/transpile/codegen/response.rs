//! Response generation

use crate::transpile::codegen::CodeGenerator;
use crate::transpile::hir::*;

pub struct ResponseGen;

impl ResponseGen {
    pub fn generate_new_response(cg: &CodeGenerator, args: &[Expr]) -> String {
        let body = args.first().map(|a| cg.expr_to_rust(a)).unwrap_or_default();
        let mut lines = vec![
            "{".to_string(),
            format!("    let __body = {};", body),
            "    let mut __resp = Response::builder();".to_string(),
        ];

        if let Some(init_expr) = args.get(1) {
            if let Expr::Object { props } = init_expr {
                Self::process_init_props(cg, props, &mut lines);
            }
        }

        lines.push("    __resp.body(Body::from(__body)).unwrap()".to_string());
        lines.join("\n")
    }

    fn process_init_props(cg: &CodeGenerator, props: &[ObjectProp], lines: &mut Vec<String>) {
        for prop in props {
            if let ObjectProp::Init { key, value } = prop {
                let k = match key {
                    PropKey::Ident(s) | PropKey::String(s) => s.as_str(),
                    _ => continue,
                };
                match k {
                    "status" | "statusCode" => {
                        let v = cg.expr_to_rust(value);
                        lines.push(format!("    __resp = __resp.status({} as u16);", v));
                    }
                    "headers" => {
                        if let Expr::Object { props: header_props } = value {
                            for hp in header_props {
                                if let ObjectProp::Init { key: hk, value: hv } = hp {
                                    let header_name = match hk {
                                        PropKey::Ident(s) | PropKey::String(s) => s.clone(),
                                        _ => continue,
                                    };
                                    let header_val = cg.expr_to_rust(hv);
                                    lines.push(format!("    __resp = __resp.header(\"{}\", {});", header_name, header_val));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
