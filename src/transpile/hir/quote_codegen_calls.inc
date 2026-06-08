/// Function call codegen helpers for bind, call, apply, Number methods

impl QuoteCodegen {
    /// Handle Function.prototype.call, .apply, .bind
    fn gen_fn_method_call(&self, callee: &Expr, arguments: &[Expr]) -> Option<TokenStream> {
        if let Expr::StaticMember { obj, property, .. } = callee {
            if let Expr::Ident { name: fn_name } = obj.as_ref() {
                match property.as_str() {
                    "call" => return Some(self.gen_call_method(fn_name, arguments)),
                    "apply" => return Some(self.gen_apply_method(fn_name, arguments)),
                    "bind" => return Some(self.gen_bind_method(fn_name, arguments)),
                    _ => {}
                }
            }
        }
        None
    }

    /// Handle fn.call(ctx, arg1, arg2, ...) → fn(arg1, arg2, ...)
    fn gen_call_method(&self, fn_name: &str, arguments: &[Expr]) -> TokenStream {
        let args: Vec<_> = arguments.iter().skip(1).map(|a| {
            let arg = self.gen_expr(a);
            if matches!(a, Expr::String(_)) { quote! { #arg.to_string() } } else { arg }
        }).collect();
        if args.is_empty() {
            quote! { #fn_name() }
        } else {
            quote! { #fn_name(#(#args),*) }
        }
    }

    /// Handle fn.apply(ctx, argsArray) → fn(argsArray[0], argsArray[1], ...)
    fn gen_apply_method(&self, fn_name: &str, arguments: &[Expr]) -> TokenStream {
        if arguments.len() >= 2 {
            let args_array = self.gen_expr(&arguments[1]);
            quote! { #fn_name(#args_array) }
        } else {
            quote! { #fn_name() }
        }
    }

    /// Handle fn.bind(ctx, arg1, arg2, ...) → move || fn(arg1, arg2, ...)
    fn gen_bind_method(&self, fn_name: &str, arguments: &[Expr]) -> TokenStream {
        let bound_args: Vec<_> = arguments.iter().skip(1).map(|a| {
            let arg = self.gen_expr(a);
            if matches!(a, Expr::String(_)) { quote! { #arg.to_string() } } else { arg }
        }).collect();
        if bound_args.is_empty() {
            quote! { || #fn_name() }
        } else {
            quote! { move || #fn_name(#(#bound_args),*) }
        }
    }

    /// Handle Number static methods: Number.isFinite, Number.isNaN, Number.parseInt, Number.parseFloat
    fn gen_number_method_call(&self, obj: &Expr, property: &str, arguments: &[Expr]) -> Option<TokenStream> {
        if let Expr::Ident { name } = obj { if name != "Number" { return None; } } else { return None; }
        
        let arg0 = arguments.first().map(|a| self.gen_expr(a));
        
        match property {
            "isFinite" => {
                if let Some(expr) = arg0 {
                    Some(quote! {
                        {
                            let __n = #expr;
                            if let Value::Number(n) = __n { n.is_finite() } else { false }
                        }
                    })
                } else {
                    Some(quote! { false })
                }
            }
            "isNaN" => {
                if let Some(expr) = arg0 {
                    Some(quote! {
                        {
                            let __n = #expr;
                            if let Value::Number(n) = __n { n.is_nan() } else { false }
                        }
                    })
                } else {
                    Some(quote! { false })
                }
            }
            "parseInt" => Some(quote! { 0.0 }),
            "parseFloat" => Some(quote! { 0.0 }),
            "isInteger" => {
                if let Some(expr) = arg0 {
                    Some(quote! {
                        {
                            let __n = #expr;
                            if let Value::Number(n) = __n { n.fract() == 0.0 } else { false }
                        }
                    })
                } else {
                    Some(quote! { false })
                }
            }
            _ => None
        }
    }

    /// Handle array immutable methods: toSpliced, with
    fn gen_array_method_call(&self, obj: &Expr, property: &str, arguments: &[Expr]) -> Option<TokenStream> {
        let obj_ts = self.gen_expr(obj);
        match property {
            "toSpliced" => {
                let start = arguments.first().map(|a| self.gen_expr(a)).unwrap_or_else(|| quote! { 0.0 });
                let delete = arguments.get(1).map(|a| self.gen_expr(a)).unwrap_or_else(|| quote! { 0.0 });
                let items: Vec<_> = arguments.iter().skip(2).map(|a| self.gen_expr(a)).collect();
                Some(quote! {
                    {
                        let mut __arr = #obj_ts.clone();
                        let __start = #start as usize;
                        let __delete = #delete as usize;
                        let __items = vec![#(#items),*];
                        __arr.splice(__start..(__start + __delete), __items);
                        __arr
                    }
                })
            }
            "with" => {
                let idx = arguments.first().map(|a| self.gen_expr(a)).unwrap_or_else(|| quote! { 0.0 });
                let val = arguments.get(1).map(|a| self.gen_expr(a)).unwrap_or_else(|| quote! { Value::Null });
                Some(quote! {
                    {
                        let mut __arr = #obj_ts.clone();
                        let __idx = #idx as usize;
                        if __idx < __arr.len() {
                            __arr[__idx] = #val;
                        }
                        __arr
                    }
                })
            }
            _ => None,
        }
    }

    fn gen_console_call(&self, obj: &Expr, property: &str, arguments: &[Expr]) -> Option<TokenStream> {
        if let Expr::Ident { name } = obj { if name != "console" { return None; } } else { return None; }
        match property { "log" | "error" | "info" | "table" | "warn" => Some(self.gen_console_output(property, arguments)), "assert" => Some(self.gen_console_assert(arguments)), _ => None }
    }
    fn gen_console_output(&self, method: &str, args: &[Expr]) -> TokenStream {
        let is_err = method == "error" || method == "warn";
        let gen_args: Vec<_> = args.iter().map(|a| self.gen_expr(a)).collect();
        if args.len() == 1 { if is_err { syn::parse_quote! { eprintln!("{}", #(#gen_args),*) } } else { syn::parse_quote! { println!("{}", #(#gen_args),*) } } }
        else { let fmts: Vec<_> = args.iter().map(|_| quote! { "{}" }).collect(); if is_err { syn::parse_quote! { eprintln!(#(#fmts),*, #(#gen_args),*) } } else { syn::parse_quote! { println!(#(#fmts),*, #(#gen_args),*) } } }
    }
    fn gen_console_assert(&self, args: &[Expr]) -> TokenStream {
        let gen_args: Vec<_> = args.iter().map(|a| self.gen_expr(a)).collect();
        if args.len() >= 2 { let cond = gen_args.first().unwrap(); let msg = gen_args.get(1).unwrap(); quote! { assert!(#cond, "{}", #msg) } }
        else if args.len() == 1 { quote! { assert!(#(gen_args.first().unwrap())) } } else { quote! { () } }
    }
}
