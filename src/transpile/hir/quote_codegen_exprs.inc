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
            E::Call { .. } | E::New { .. } | E::Member { .. } | E::StaticMember { .. } | E::PrivateMember { .. } => self.gen_call_member_static(expr),
            E::Spread { .. } => self.gen_spread_expr(expr),
            E::Seq { .. } => self.gen_seq_expr_helper(expr),
            E::MetaProperty { kind } => self.gen_meta_prop(kind),
            E::ImportExpression { source } => self.gen_import_expr(source),
            // TypeScript type assertions - erased at runtime, emit inner expr
            E::TypeAssertion { expr, .. } => self.gen_expr(expr),
            E::Satisfies { expr, .. } => self.gen_expr(expr),
            E::NonNull { expr } => self.gen_expr(expr),
            // TypeAnnot only has type info, no inner expr - emit Null
            E::TypeAnnot { .. } => quote! { Value::Null },
            _ => quote! { Value::Null },
        }
    }

    fn gen_meta_prop(&self, kind: &super::MetaPropKind) -> TokenStream {
        use super::MetaPropKind;
        match kind {
            MetaPropKind::NewTarget => quote! { Value::Null },
            MetaPropKind::ImportMeta => quote! { Value::Object(std::collections::HashMap::new()) },
        }
    }

    fn gen_import_expr(&self, source: &Expr) -> TokenStream {
        // Dynamic import() - returns a Promise that resolves to the module namespace
        // For codegen, we emit a placeholder that compiles but returns Null at runtime
        // (The dev path handles this properly in rquickjs)
        let source_ts = self.gen_expr(source);
        quote! { {
            fn __dynamic_import(path: &str) -> Value {
                Value::Null
            }
            let __module_path = #source_ts;
            if let Value::String(s) = __module_path {
                __dynamic_import(&s)
            } else {
                Value::Null
            }
        } }
    }

    fn gen_spread_expr(&self, expr: &Expr) -> TokenStream {
        use super::Expr as E;
        if let E::Spread { arg } = expr {
            let arg_ts = self.gen_expr(arg);
            // Spread in array context: expand the iterable
            // In Rust, we need to handle this differently depending on context
            // For now, use into_iter() if it's a vec, or collect into vec
            quote! { {
                let __spread_arg = #arg_ts;
                match __spread_arg {
                    Value::Array(arr) => arr,
                    Value::Object(map) => map.values().cloned().collect::<Vec<_>>(),
                    _ => vec![__spread_arg],
                }
            } }
        } else {
            quote! { Value::Null }
        }
    }

    fn gen_seq_expr_helper(&self, expr: &Expr) -> TokenStream {
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
            E::Call { callee, arguments, optional: false } | E::New { callee, arguments, optional: false } => self.gen_call_expr(callee, arguments),
            E::Call { callee, arguments, optional: true } | E::New { callee, arguments, optional: true } => self.gen_optional_call(callee, arguments),
            E::Member { obj, property, computed, optional } => self.gen_member_expr_full_opt(obj, property, *computed, *optional),
            E::StaticMember { obj, property, optional: false } => self.gen_static_member_expr(obj, property),
            E::StaticMember { obj, property, optional: true } => self.gen_static_member_expr_opt(obj, property),
            E::PrivateMember { obj, property, optional } => self.gen_private_member_expr_opt(obj, property, *optional),
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
        // Convert JS template literal to format!() macro
        // `Hello ${name}!` -> format!("Hello {}!", name)
        let mut format_str = String::new();
        let mut args: Vec<TokenStream> = Vec::new();
        let mut expr_idx = 0;
        
        for part in parts {
            match part {
                super::TemplatePart::String { value: s } => {
                    // Escape {} in the string part for format!
                    let escaped = s.replace("{", "{{}").replace("}", "{}}");
                    format_str.push_str(&escaped);
                }
                super::TemplatePart::Type { value: _ } => {
                    format_str.push_str("{{}}");
                }
            }
            
            if expr_idx < exprs.len() {
                let expr = &exprs[expr_idx];
                let expr_ts = self.gen_expr(expr);
                args.push(expr_ts);
                format_str.push_str("{}");
                expr_idx += 1;
            }
        }
        
        if args.is_empty() {
            // Static string, no interpolation
            let s = format_str.replace("{{" , "{").replace("}}", "}");
            quote! { #s.to_string() }
        } else {
            // Has interpolation - use format!
            quote! { format!(#format_str, #(#args),*) }
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

    #[allow(dead_code)]
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
            "EPSILON" => quote! { f64::EPSILON },
            "MAX_SAFE_INTEGER" => quote! { 9007199254740991.0f64 },
            "MIN_SAFE_INTEGER" => quote! { -9007199254740991.0f64 },
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
        match op {
            super::LogicalOp::And => quote! { #lhs && #rhs },
            super::LogicalOp::Or => quote! { #lhs || #rhs },
            super::LogicalOp::NullishCoalescing => {
                // JS ?? operator: nullish coalescing
                // x ?? y -> if x is null/undefined then y else x
                // We use Option pattern: x.unwrap_or_else(|| y)
                quote! { {
                    let __lhs = #lhs;
                    if __lhs.is_null() || __lhs.is_undefined() {
                        #rhs
                    } else {
                        __lhs
                    }
                } }
            }
        }
    }

    fn gen_cond_expr(&self, test: &Expr, consequent: &Expr, alternate: &Expr) -> TokenStream {
        let test = self.gen_expr(test);
        let cons = self.gen_expr(consequent);
        let alt = self.gen_expr(alternate);
        quote! { if #test { #cons } else { #alt } }
    }

    fn gen_array_expr(&self, elems: &[Option<Expr>]) -> TokenStream {
        // Handle arrays with spread: [a, ...b, c] -> concatenate vectors
        let mut concat_parts: Vec<TokenStream> = Vec::new();
        let mut current_vec: Vec<TokenStream> = Vec::new();
        
        for elem in elems {
            match elem {
                Some(Expr::Spread { arg }) => {
                    // Finish current vec if non-empty
                    if !current_vec.is_empty() {
                        concat_parts.push(quote! { vec![#(#current_vec),*] });
                        current_vec.clear();
                    }
                    // Add spread arg
                    let arg_ts = self.gen_expr(arg);
                    concat_parts.push(quote! { {
                        let __spread_arg = #arg_ts;
                        match __spread_arg {
                            Value::Array(arr) => arr,
                            _ => vec![__spread_arg],
                        }
                    } });
                }
                Some(expr) => {
                    current_vec.push(self.gen_expr(expr));
                }
                None => {
                    current_vec.push(quote! { Value::Null });
                }
            }
        }
        
        // Add any remaining items
        if !current_vec.is_empty() {
            concat_parts.push(quote! { vec![#(#current_vec),*] });
        }
        
        // Combine all parts
        if concat_parts.is_empty() {
            quote! { Vec::<Value>::new() }
        } else if concat_parts.len() == 1 {
            concat_parts.pop().unwrap()
        } else {
            quote! { {
                let mut __result = Vec::new();
                #( __result.extend(#concat_parts); )*
                __result
            } }
        }
    }

    fn gen_object_expr(&self, members: &[super::ObjectMemberExpr]) -> TokenStream {
        // Handle objects with spread: { a, ...b, c }
        let mut entries: Vec<TokenStream> = Vec::new();
        let mut concat_parts: Vec<TokenStream> = Vec::new();
        let mut current_map: Vec<TokenStream> = Vec::new();
        
        for m in members {
            match &m.prop {
                super::ObjectProp::Spread { arg } => {
                    // Finish current map if non-empty
                    if !current_map.is_empty() {
                        entries.push(quote! { std::collections::HashMap::from([#(#current_map),*]) });
                        current_map.clear();
                    }
                    // Add spread arg
                    let arg_ts = self.gen_expr(arg);
                    concat_parts.push(quote! { {
                        let __spread_arg = #arg_ts;
                        match __spread_arg {
                            Value::Object(map) => map,
                            _ => std::collections::HashMap::new(),
                        }
                    } });
                }
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
                    current_map.push(quote! { (#key_ts, #value_ts) });
                }
                _ => { /* Get, Set, Method - skip for now */ }
            }
        }
        
        // Add remaining entries
        if !current_map.is_empty() {
            entries.push(quote! { std::collections::HashMap::from([#(#current_map),*]) });
        }
        
        // Combine all parts
        if entries.is_empty() {
            quote! { std::collections::HashMap::new() }
        } else if entries.len() == 1 {
            entries.pop().unwrap()
        } else {
            quote! { {
                let mut __result = std::collections::HashMap::new();
                #( for (k, v) in #entries { __result.insert(k, v); } )*
                __result
            } }
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

    // Optional chaining codegen helpers
    fn gen_optional_call(&self, callee: &Expr, arguments: &[Expr]) -> TokenStream {
        let args: Vec<_> = arguments.iter().map(|a| {
            let arg = self.gen_expr(a);
            if matches!(a, Expr::String(_)) { quote! { #arg.to_string() } } else { arg }
        }).collect();
        let callee_ts = self.gen_expr(callee);
        quote! {
            {
                let __opt_val = #callee_ts;
                if matches!(__opt_val, Value::Null | Value::Undefined) {
                    Value::Undefined
                } else {
                    #callee_ts(#(#args),*)
                }
            }
        }
    }

    fn gen_static_member_expr_opt(&self, obj: &Expr, property: &str) -> TokenStream {
        let prop = syn::Ident::new(property, proc_macro2::Span::call_site());
        let obj_ts = self.gen_expr(obj);
        quote! {
            {
                let __opt_val = #obj_ts;
                if matches!(__opt_val, Value::Null | Value::Undefined) {
                    Value::Undefined
                } else {
                    __opt_val.#prop
                }
            }
        }
    }

    fn gen_member_expr_full_opt(&self, obj: &Expr, property: &Expr, computed: bool, optional: bool) -> TokenStream {
        let prop = self.gen_expr(property);
        let obj_ts = self.gen_expr(obj);
        if optional {
            if computed {
                quote! {
                    {
                        let __opt_val = #obj_ts;
                        if matches!(__opt_val, Value::Null | Value::Undefined) {
                            Value::Undefined
                        } else {
                            __opt_val[#prop]
                        }
                    }
                }
            } else {
                quote! {
                    {
                        let __opt_val = #obj_ts;
                        if matches!(__opt_val, Value::Null | Value::Undefined) {
                            Value::Undefined
                        } else {
                            __opt_val.#prop
                        }
                    }
                }
            }
        } else {
            if computed { quote! { #obj_ts[#prop] } } else { quote! { #obj_ts.#prop } }
        }
    }

    fn gen_private_member_expr_opt(&self, obj: &Expr, property: &str, optional: bool) -> TokenStream {
        let prop = syn::Ident::new(property, proc_macro2::Span::call_site());
        let obj_ts = self.gen_expr(obj);
        if optional {
            quote! {
                {
                    let __opt_val = #obj_ts;
                    if matches!(__opt_val, Value::Null | Value::Undefined) {
                        Value::Undefined
                    } else {
                        __opt_val.#prop
                    }
                }
            }
        } else {
            quote! { #obj_ts.#prop }
        }
    }
}
