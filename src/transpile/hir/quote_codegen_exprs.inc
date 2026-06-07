impl QuoteCodegen {
    fn gen_expr_helper(&self, expr: &Expr) -> TokenStream {
        use super::Expr as E;
        match expr {
            E::Template { .. } | E::Ident { .. } | E::JSX(_) => self.gen_templ_ident_jsx(expr),
            E::Bin { .. } | E::Unary { .. } | E::Update { .. } => self.gen_bin_unary_update(expr),
            E::Logical { .. } | E::Cond { .. } | E::Assign { .. } => self.gen_log_cond_assign(expr),
            E::Array { elems } => self.gen_array_expr(elems),
            E::Object { members } => self.gen_object_expr(members),
            E::Function(_) | E::ArrowFunction { .. } => self.gen_fn_arrow(expr),
            E::Await { .. } | E::Yield { .. } => self.gen_await_yield(expr),
            E::Call { .. } | E::New { .. } | E::Member { .. } | E::StaticMember { .. } => self.gen_call_member_static(expr),
            E::Seq { .. } | E::Spread { .. } | E::TypeAnnot { .. } | _ => self.gen_seq_type_or_null(expr),
        }
    }

    fn gen_seq_type_or_null(&self, expr: &Expr) -> TokenStream {
        use super::Expr as E;
        if let E::Seq { left, right } = expr {
            self.gen_seq_expr(left, right)
        } else {
            quote! { Value::Null }
        }
    }

    fn gen_seq_expr(&self, left: &Expr, right: &Expr) -> TokenStream {
        let lhs = self.gen_expr(left);
        let rhs = self.gen_expr(right);
        quote! { { #lhs; #rhs } }
    }

    fn gen_templ_ident_jsx(&self, expr: &Expr) -> TokenStream {
        use super::Expr as E;
        match expr {
            E::Template { parts, exprs } => self.gen_template_expr(parts, exprs),
            E::Ident { name } => self.gen_ident_expr(name),
            E::JSX(jsx) => self.gen_jsx_expr(jsx),
            _ => quote! { Value::Null },
        }
    }

    fn gen_bin_unary_update(&self, expr: &Expr) -> TokenStream {
        use super::Expr as E;
        match expr {
            E::Bin { op, left, right } => self.gen_bin_expr(op, left, right),
            E::Unary { op, arg, prefix } => self.gen_unary_expr(op, arg, *prefix),
            E::Update { op, arg, prefix } => self.gen_update_expr(op, arg, *prefix),
            _ => quote! { Value::Null },
        }
    }

    fn gen_log_cond_assign(&self, expr: &Expr) -> TokenStream {
        use super::Expr as E;
        match expr {
            E::Logical { op, left, right } => self.gen_logical_expr(op, left, right),
            E::Cond { test, consequent, alternate } => self.gen_cond_expr(test, consequent, alternate),
            E::Assign { op, left, right } => self.gen_assign_expr(op, left, right),
            _ => quote! { Value::Null },
        }
    }

    fn gen_fn_arrow(&self, expr: &Expr) -> TokenStream {
        use super::Expr as E;
        match expr {
            E::Function(func) => self.gen_fn_expr(func),
            E::ArrowFunction { params, body, is_async } => self.gen_arrow_expr(params, body, *is_async),
            _ => quote! { Value::Null },
        }
    }

    fn gen_await_yield(&self, expr: &Expr) -> TokenStream {
        use super::Expr as E;
        if let E::Await { arg } = expr {
            self.gen_await_expr(arg)
        } else if let E::Yield { arg, delegate: _ } = expr {
            match arg {
                Some(a) => self.gen_await_expr(a),
                None => quote! { Value::Null },
            }
        } else {
            quote! { Value::Null }
        }
    }

    fn gen_call_member_static(&self, expr: &Expr) -> TokenStream {
        use super::Expr as E;
        match expr {
            E::Call { callee, arguments } | E::New { callee, arguments } => self.gen_call_expr(callee, arguments),
            E::Member { obj, property, computed } => self.gen_member_expr_full(obj, property, *computed),
            E::StaticMember { obj, property } => self.gen_static_member_expr(obj, property),
            _ => quote! { Value::Null },
        }
    }

    fn gen_string_expr(&self, s: &str) -> TokenStream {
        // String literals in Rust are &str, so we can use them directly
        quote! { #s }
    }

    fn gen_number_expr(&self, n: &f64) -> TokenStream {
        quote! { #n }
    }

    fn gen_bool_expr(&self, b: bool) -> TokenStream {
        quote! { #b }
    }

    fn gen_template_expr(&self, parts: &[super::TemplatePart], exprs: &[Expr]) -> TokenStream {
        let mut result = TokenStream::new();
        let mut expr_idx = 0;
        
        for part in parts {
            match part {
                super::TemplatePart::String { value: s } => {
                    let s = s.to_string();
                    result.extend(quote! { #s.to_string() });
                }
                super::TemplatePart::Type { value: _ } => {
                    // This shouldn't happen in expression context, but handle it
                    result.extend(quote! { String::new() });
                }
            }
            
            if expr_idx < exprs.len() {
                let expr = &exprs[expr_idx];
                let expr_ts = self.gen_expr(expr);
                result.extend(quote! { + &#expr_ts.to_string() });
                expr_idx += 1;
            }
        }
        
        if result.is_empty() {
            quote! { String::new() }
        } else {
            result
        }
    }

    fn gen_ident_expr(&self, name: &str) -> TokenStream {
        let id = syn::Ident::new(name, proc_macro2::Span::call_site());
        quote! { #id }
    }

    fn gen_static_member_expr(&self, obj: &Expr, property: &str) -> TokenStream {
        if let Expr::Ident { name: obj_name } = obj {
            let obj_name_str: &str = obj_name.as_str();
            match obj_name_str {
                "Number" => return Self::gen_number_const(property),
                "Math" => return Self::gen_math_const(property),
                _ => {}
            }
        }
        let obj = self.gen_expr(obj);
        let prop = syn::Ident::new(property, proc_macro2::Span::call_site());
        quote! { #obj.#prop }
    }

    fn gen_member_expr_full(&self, obj: &Expr, property: &Expr, computed: bool) -> TokenStream {
        if let Expr::Ident { name: obj_name } = obj {
            let obj_name_str: &str = obj_name.as_str();
            if let Expr::Ident { name: prop_name } = property {
                let prop_str = prop_name.as_str();
                if obj_name_str == "Number" { return Self::gen_number_const(prop_str); }
                if obj_name_str == "Math" { return Self::gen_math_const(prop_str); }
            }
        }
        let obj = self.gen_expr(obj);
        let prop = self.gen_expr(property);
        if computed { quote! { #obj[#prop] } } else { quote! { #obj.#prop } }
    }

    fn gen_number_const(prop: &str) -> TokenStream {
        match prop {
            "NaN" => quote! { f64::NAN },
            "POSITIVE_INFINITY" | "Infinity" => quote! { f64::INFINITY },
            "NEGATIVE_INFINITY" => quote! { f64::NEG_INFINITY },
            "MAX_VALUE" => quote! { f64::MAX },
            "MIN_VALUE" => quote! { f64::MIN_POSITIVE },
            _ => { let id = syn::Ident::new(prop, proc_macro2::Span::call_site()); quote! { Number.#id } }
        }
    }

    fn gen_math_const(prop: &str) -> TokenStream {
        let c = match prop {
            "PI" | "E" | "SQRT2" | "LN2" | "LN10" | "LOG2E" | "LOG10E" => prop,
            "SQRT1_2" => "FRAC_1_SQRT_2",
            _ => return { let id = syn::Ident::new(prop, proc_macro2::Span::call_site()); quote! { Math.#id } }
        };
        quote! { std::f64::consts::#c }
    }

    fn gen_unary_expr(&self, op: &super::UnaryOp, arg: &Expr, _prefix: bool) -> TokenStream {
        use super::UnaryOp as U;
        let arg_ts = self.gen_expr(arg);
        match op {
            U::Plus => quote! { #arg_ts },
            U::Minus => quote! { -#arg_ts },
            U::Not | U::BitNot => quote! { !#arg_ts },
            U::Typeof => self.gen_typeof_expr(&arg_ts),
            U::Void => quote! { () },
            U::Delete => quote! { false },
        }
    }

    fn gen_typeof_expr(&self, arg_ts: &TokenStream) -> TokenStream {
        let s = quote! { match &#arg_ts };
        let arms = self.typeof_arms();
        quote! { { #s { #(#arms),* } } }
    }

    fn typeof_arms(&self) -> Vec<TokenStream> {
        vec![
            quote! { Value::String(_) => "string" },
            quote! { Value::Number(_) => "number" },
            quote! { Value::Boolean(_) => "boolean" },
            quote! { Value::Null => "object" },
            quote! { Value::Undefined => "undefined" },
            quote! { Value::BigInt(_) => "bigint" },
            quote! { Value::Function(_) => "function" },
            quote! { Value::Object(_) | Value::Array(_) | Value::RegExp(_, _) => "object" },
            quote! { _ => "unknown" },
        ]
    }

    fn gen_logical_expr(&self, op: &super::LogicalOp, left: &Expr, right: &Expr) -> TokenStream {
        let lhs = self.gen_expr(left);
        let rhs = self.gen_expr(right);
        let op_str = match op {
            super::LogicalOp::And => "&&",
            super::LogicalOp::Or => "||",
            super::LogicalOp::NullishCoalescing => "??",
        };
        quote! { #lhs #op_str #rhs }
    }

    fn gen_cond_expr(&self, test: &Expr, consequent: &Expr, alternate: &Expr) -> TokenStream {
        let test = self.gen_expr(test);
        let cons = self.gen_expr(consequent);
        let alt = self.gen_expr(alternate);
        quote! { if #test { #cons } else { #alt } }
    }

    fn gen_array_expr(&self, elems: &[Option<Expr>]) -> TokenStream {
        let items: Vec<_> = elems.iter()
            .map(|e| e.as_ref().map(|e| self.gen_expr(e)).unwrap_or_else(|| quote! { Value::Null }))
            .collect();
        quote! { vec![#(#items),*] }
    }

    fn gen_object_expr(&self, members: &[super::ObjectMemberExpr]) -> TokenStream {
        let entries: Vec<TokenStream> = members.iter().filter_map(|m| {
            match &m.prop {
                super::ObjectProp::Init { key, value, computed: _ } => {
                    let key_ts = match key {
                        super::PropKey::Str(s) => {
                            let s = s.to_string();
                            quote! { #s.to_string() }
                        }
                        super::PropKey::Num(n) => {
                            let n_s = n.to_string();
                            quote! { #n_s.to_string() }
                        }
                        super::PropKey::Computed { expr } => {
                            let expr_ts = self.gen_expr(expr);
                            quote! { format!("{}", #expr_ts) }
                        }
                    };
                    let value_ts = self.gen_expr(value);
                    Some(quote! { (#key_ts, #value_ts) })
                }
                _ => None, // Get, Set, Method, Spread not yet supported
            }
        }).collect();
        
        if entries.is_empty() {
            quote! { std::collections::HashMap::new() }
        } else {
            quote! { std::collections::HashMap::from([#(#entries),*]) }
        }
    }

    fn gen_fn_expr(&self, func: &super::FunctionDecl) -> TokenStream {
        self.gen_fn(func)
    }

    fn gen_arrow_expr(&self, params: &[super::Param], body: &Expr, is_async: bool) -> TokenStream {
        let params_ts = self.gen_params(params);
        let body_ts = self.gen_expr(body);
        if is_async {
            quote! { async move |#params_ts| -> Value { #body_ts } }
        } else {
            quote! { move |#params_ts| -> Value { #body_ts } }
        }
    }

    fn gen_await_expr(&self, arg: &Expr) -> TokenStream {
        let arg_ts = self.gen_expr(arg);
        quote! { #arg_ts.await }
    }

    fn gen_block_expr(&self, stmts: &[super::Stmt]) -> TokenStream {
        let inner: Vec<_> = stmts.iter()
            .filter_map(|s| self.gen_stmt(s))
            .collect();
        quote! { { #(#inner)* } }
    }
}
