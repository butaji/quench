impl QuoteCodegen {
    pub(crate) fn gen_stmt(&self, stmt: &Stmt) -> Option<TokenStream> {
        use super::Stmt as S;
        match stmt {
            S::Block { stmts } => Some(self.gen_block(stmts)),
            S::Expr { expr: _ } => self.gen_expr_return(stmt),
            S::Return { arg: _ } => self.gen_expr_return(stmt),
            S::If { test, consequent, alternate } => self.gen_if_stmt(test, consequent, alternate.as_deref()),
            S::Switch { discriminant: _, cases: _ } => self.gen_switch_try(stmt),
            S::Try { block: _, handler: _, finalizer: _ } => self.gen_switch_try(stmt),
            S::For { init: _, test: _, update: _, body: _ } => self.gen_loop_stmt(stmt),
            S::While { test: _, body: _ } => self.gen_loop_stmt(stmt),
            S::DoWhile { body: _, test: _ } => self.gen_loop_stmt(stmt),
            S::Break { .. } => Some(self.gen_jump(stmt)),
            S::Continue { .. } => Some(self.gen_jump(stmt)),
            S::Throw { .. } => Some(self.gen_jump(stmt)),
            S::Labeled { label, body } => Some(self.gen_labeled_body(label, body)),
            S::With { obj, body } => {
                let obj_ts = self.gen_expr(obj);
                let body_ts = self.gen_block_stmt(body);
                Some(quote! { { let __with_obj = #obj_ts; #body_ts } })
            },
            S::FunctionDecl(_) => self.gen_decl_stmt(stmt),
            S::Variable(super::VariableDecl { kind: _, init: _, .. }) => self.gen_decl_stmt(stmt),
            _ => self.gen_catchall_stmt(stmt),
        }
    }

    #[allow(dead_code)]
    fn gen_jump_labeled(&self, stmt: &Stmt) -> Option<TokenStream> {
        use super::Stmt as S;
        match stmt {
            S::Break { .. } | S::Continue { .. } | S::Throw { .. } => Some(self.gen_jump(stmt)),
            S::Labeled { label, body } => Some(self.gen_labeled_body(label, body)),
            S::With { obj, body } => {
                let obj_ts = self.gen_expr(obj);
                let body_ts = self.gen_block_stmt(body);
                Some(quote! { { let __with_obj = #obj_ts; #body_ts } })
            },
            _ => None,
        }
    }

    fn gen_catchall_stmt(&self, stmt: &Stmt) -> Option<TokenStream> {
        use super::Stmt as S;
        match stmt {
            S::Empty => Some(quote! {}),
            S::ForIn { left, right, body, .. } => Some(self.gen_for_in(left, right, body)),
            S::ForOf { left, right, body, .. } => Some(self.gen_for_of(left, right, body, false)),
            S::Class(_) => None,
            S::ExportNamed { specifiers } => Some(self.gen_export_named(specifiers)),
            S::ExportDefault { expr } => Some(self.gen_export_default(expr)),
            S::ImportNamed { source, specifiers } => self.gen_import_stmt(source, Some(specifiers)),
            S::ImportDefault { source, local } => Some(self.gen_import_default(source, local)),
            _ => None,
        }
    }

    fn gen_import_stmt(&self, source: &String, specifiers: Option<&[super::ImportSpecifier]>) -> Option<TokenStream> {
        match specifiers { Some(specs) => Some(self.gen_import_named(source, specs)), None => None }
    }

    fn gen_if_stmt(&self, test: &Expr, consequent: &Box<Stmt>, alternate: Option<&Stmt>) -> Option<TokenStream> {
        Some(self.gen_if(test, consequent, alternate))
    }

    fn gen_expr_return(&self, stmt: &Stmt) -> Option<TokenStream> {
        use super::Stmt as S;
        match stmt { S::Expr { expr } => Some(self.gen_expr_stmt(expr)), S::Return { arg } => Some(self.gen_return(arg)), _ => None }
    }

    fn gen_switch_try(&self, stmt: &Stmt) -> Option<TokenStream> {
        use super::Stmt as S;
        match stmt { S::Switch { discriminant, cases } => Some(self.gen_switch(discriminant, cases)), S::Try { block, handler, finalizer } => Some(self.gen_try(block, handler, finalizer)), _ => None }
    }

    fn gen_loop_stmt(&self, stmt: &Stmt) -> Option<TokenStream> {
        use super::Stmt as S;
        match stmt { S::For { init, test, update, body } => Some(self.gen_for(init, test, update, body)), S::While { test, body } => Some(self.gen_while(test, body)), S::DoWhile { body, test } => Some(self.gen_while(test, body)), _ => None }
    }

    fn gen_decl_stmt(&self, stmt: &Stmt) -> Option<TokenStream> {
        use super::Stmt as S;
        match stmt {
            S::FunctionDecl(func) => Some(self.gen_fn(func)),
            S::Variable(var) => self.gen_var_decl(var),
            _ => None,
        }
    }

    #[allow(dead_code)]
    fn gen_iter_loop(&self, stmt: &Stmt) -> Option<TokenStream> {
        use super::Stmt as S;
        match stmt {
            S::ForIn { left, right, body, .. } => Some(self.gen_for_in(left, right, body)),
            S::ForOf { left, right, body, .. } => Some(self.gen_for_of(left, right, body, false)),
            S::While { test, body } => Some(self.gen_while(test, body)),
            _ => None,
        }
    }

    fn gen_labeled_body(&self, label: &String, body: &Stmt) -> TokenStream {
        let inner = self.gen_stmt(body).unwrap_or(quote! {});
        quote! { #label: #inner }
    }

    #[allow(dead_code)]
    fn gen_for_in_out(&self, _left: &Pat, right: &Expr, body: &Stmt) -> TokenStream {
        let left_ts = self.gen_for_init(&Some(super::ForInit::Variable(super::VariableKind::Let, vec![("__item".to_string(), None)])));
        let right_ts = self.gen_expr(right);
        let body_ts = self.gen_block_stmt(body);
        quote! { for #left_ts in #right_ts { #body_ts } }
    }

    fn gen_jump(&self, stmt: &Stmt) -> TokenStream {
        use super::Stmt as S;
        match stmt {
            S::Break { label } => Self::gen_labeled_jump("break", label.as_deref()),
            S::Continue { label } => Self::gen_labeled_jump("continue", label.as_deref()),
            _ => quote! {},
        }
    }

    fn gen_labeled_jump(kw: &str, label: Option<&str>) -> TokenStream {
        match label {
            Some(l) => quote! { #kw #l; },
            None => quote! { #kw; },
        }
    }

    fn gen_expr_stmt(&self, expr: &Expr) -> TokenStream {
        let expr = self.gen_expr(expr);
        quote! { #expr; }
    }

    fn gen_return(&self, arg: &Option<Expr>) -> TokenStream {
        match arg {
            Some(e) => {
                let expr = self.gen_expr(e);
                quote! { return #expr; }
            }
            None => quote! { return; },
        }
    }

    fn gen_export_named(&self, specifiers: &[super::Export]) -> TokenStream {
        use super::Export as E;
        let items: Vec<TokenStream> = specifiers.iter().map(|spec| {
            match spec {
                E::Named { name } => { let ident = syn::Ident::new(name, proc_macro2::Span::call_site()); quote! { #ident } }
                E::NamedWithValue { name, value } => { let ident = syn::Ident::new(name, proc_macro2::Span::call_site()); let val = self.gen_expr(value); quote! { #ident = #val } }
                E::ReExport { source, names } => { let mod_path = self.module_path(source); let idents: Vec<_> = names.iter().map(|n| syn::Ident::new(n, proc_macro2::Span::call_site())).collect(); quote! { pub use #mod_path::{#(#idents),*}; } }
                E::All { source } => { let mod_path = self.module_path(source); quote! { pub use #mod_path::*; } }
                E::Default { expr } => { let val = self.gen_expr(expr); quote! { #val } }
                E::NamedRenamed { local, exported } => {
                    let local_ident = syn::Ident::new(local, proc_macro2::Span::call_site());
                    let exported_ident = syn::Ident::new(exported, proc_macro2::Span::call_site());
                    quote! { #local_ident as #exported_ident }
                }
            }
        }).collect();

        if items.len() == 1 {
            let item = &items[0];
            if matches!(specifiers[0], E::Named { .. }) { quote! { pub use #item; } } else { quote! { #item } }
        } else {
            let uses: Vec<TokenStream> = specifiers.iter().filter_map(|spec| {
                if let E::Named { name } = spec { let ident = syn::Ident::new(name, proc_macro2::Span::call_site()); Some(quote! { pub use #ident; }) } else { None }
            }).collect();
            quote! { #(#uses)* }
        }
    }

    fn gen_export_default(&self, expr: &Expr) -> TokenStream {
        let val = self.gen_expr(expr);
        match expr {
            Expr::Function(_) => {
                // export default function -> fn default() { ... }
                // The function already has its own name
                quote! { #val }
            }
            Expr::ArrowFunction { .. } => {
                // export default arrow -> fn default() { ... }
                quote! { fn default() { #val } }
            }
            _ => {
                // export default expr -> pub const default = expr;
                quote! { pub const default = #val; }
            }
        }
    }

    fn gen_import_named(&self, source: &str, specifiers: &[super::ImportSpecifier]) -> TokenStream {
        use super::ImportSpecifier as I;
        let mod_path = self.module_path(source);

        let items: Vec<TokenStream> = specifiers.iter().map(|spec| {
            match spec {
                I::Named { name, alias } => {
                    let orig = syn::Ident::new(name, proc_macro2::Span::call_site());
                    match alias {
                        Some(a) => {
                            let alias_ident = syn::Ident::new(a, proc_macro2::Span::call_site());
                            quote! { #orig as #alias_ident }
                        }
                        None => quote! { #orig },
                    }
                }
                I::Default { name } => {
                    let ident = syn::Ident::new(name, proc_macro2::Span::call_site());
                    quote! { #ident }
                }
                I::Namespace { name } => {
                    let ident = syn::Ident::new(name, proc_macro2::Span::call_site());
                    quote! { * as #ident }
                }
            }
        }).collect();

        quote! { use #mod_path::{#(#items),*}; }
    }

    fn gen_import_default(&self, source: &str, local: &str) -> TokenStream {
        let mod_path = self.module_path(source);
        let ident = syn::Ident::new(local, proc_macro2::Span::call_site());
        quote! { use #mod_path::#ident; }
    }

    /// Convert a JS module path to a Rust module path
    /// e.g., "react" -> "::runts::react", "./utils/helper" -> "::runts::helper"
    fn module_path(&self, source: &str) -> syn::Path {
        // For relative paths, extract the last component
        let module_name = if source.starts_with('.') {
            source.split('/').last().unwrap_or(source)
        } else {
            source
        };

        syn::Ident::new(module_name, proc_macro2::Span::call_site()).into()
    }

    fn gen_if(&self, test: &Expr, cons: &Box<Stmt>, alt: Option<&Stmt>) -> TokenStream {
        let test = self.gen_expr(test);
        let cons = self.gen_block_stmt(cons);

        match alt {
            Some(a) => {
                let alt = self.gen_block_stmt(&Box::new(a.clone()));
                quote! {
                    if #test {
                        #cons
                    } else {
                        #alt
                    }
                }
            }
            None => quote! {
                if #test {
                    #cons
                }
            },
        }
    }

    fn gen_switch(&self, discriminant: &Expr, cases: &[super::SwitchCase]) -> TokenStream {
        let disc = self.gen_expr(discriminant);
        let arms: Vec<TokenStream> = cases
            .iter()
            .map(|case| {
                let test = case.test.as_ref().map(|t| self.gen_expr(t));
                let consequent: Vec<TokenStream> = case.consequent
                    .iter()
                    .filter_map(|s| self.gen_stmt(s))
                    .collect();
                match test {
                    Some(t) => quote! {
                        #t => {
                            #(#consequent)*
                        }
                    },
                    None => quote! {
                        _ => {
                            #(#consequent)*
                        }
                    },
                }
            })
            .collect();
        quote! {
            match #disc {
                #(#arms)*
            }
        }
    }

    fn gen_for(&self, init: &Option<super::ForInit>, test: &Option<Expr>, update: &Option<Expr>, body: &Box<Stmt>) -> TokenStream {
        let init_tokens = self.gen_for_init(init);
        let test_token = test.as_ref().map(|t| self.gen_expr(t));
        let update_token = update.as_ref().map(|u| self.gen_expr(u));
        let body_token = self.gen_block_stmt(body);
        quote! {
            for #init_tokens #test_token; #update_token {
                #body_token
            }
        }
    }

    fn gen_for_in(&self, left: &super::ForInit, right: &Expr, body: &Box<Stmt>) -> TokenStream {
        let left_token = self.gen_for_init(&Some(left.clone()));
        let right_token = self.gen_expr(right);
        let body_token = self.gen_block_stmt(body);
        quote! {
            for #left_token in #right_token {
                #body_token
            }
        }
    }

    fn gen_for_of(&self, left: &super::ForInit, right: &Expr, body: &Box<Stmt>, is_await: bool) -> TokenStream {
        let left_token = self.gen_for_init(&Some(left.clone()));
        let right_token = self.gen_expr(right);
        let body_token = self.gen_block_stmt(body);
        if is_await {
            // Extract variable name from left token for while let Some pattern
            let var_name = if let Some(super::ForInit::Variable(_, vars)) = Some(left) {
                vars.first().map(|(name, _)| {
                    syn::Ident::new(name, proc_macro2::Span::call_site())
                }).unwrap_or_else(|| syn::Ident::new("__item", proc_macro2::Span::call_site()))
            } else {
                syn::Ident::new("__item", proc_macro2::Span::call_site())
            };
            quote! {
                while let Some(#var_name) = #right_token.next().await {
                    #body_token
                }
            }
        } else {
            quote! {
                for #left_token in #right_token {
                    #body_token
                }
            }
        }
    }

    fn gen_for_init(&self, init: &Option<super::ForInit>) -> TokenStream {
        match init {
            Some(super::ForInit::Variable(kind, vars)) => {
                self.gen_for_var_init(*kind, vars)
            }
            Some(super::ForInit::Expr(e)) => {
                self.gen_expr(e)
            }
            None => quote! {},
        }
    }

    fn gen_for_var_init(&self, kind: super::VariableKind, vars: &[(String, Option<super::Expr>)]) -> TokenStream {
        let decls: Vec<TokenStream> = vars
            .iter()
            .map(|(name, init)| self.gen_var_declarator(name, init))
            .collect();
        let keyword = self.var_kind_to_keyword(kind);
        quote! { #keyword #(#decls),* }
    }

    fn gen_var_declarator(&self, name: &str, init: &Option<super::Expr>) -> TokenStream {
        let id = syn::Ident::new(name, proc_macro2::Span::call_site());
        match init {
            Some(expr) => {
                let e = self.gen_expr(expr);
                quote! { #id = #e }
            }
            None => quote! { #id },
        }
    }

    fn var_kind_to_keyword(&self, kind: super::VariableKind) -> TokenStream {
        match kind {
            super::VariableKind::Var => quote! { let mut },
            super::VariableKind::Let => quote! { let },
            super::VariableKind::Const => quote! { let },
        }
    }

    fn gen_var_decl(&self, var: &VariableDecl) -> Option<TokenStream> {
        let init = var.init.as_ref()?;
        let init_expr = self.gen_expr(init);

        // Handle patterns
        if let Some(ref pattern) = var.pattern {
            self.gen_var_decl_pattern(var.kind, pattern, &init_expr)
        } else {
            self.gen_var_decl_simple(var.kind, &var.name, &init_expr)
        }
    }

    fn gen_var_decl_pattern(&self, kind: VariableKind, pat: &Pat, init_expr: &TokenStream) -> Option<TokenStream> {
        let decls = self.gen_pat(pat, init_expr);
        if decls.is_empty() {
            return None;
        }
        let keyword = self.var_keyword(kind);
        if decls.len() == 1 {
            let decl = &decls[0];
            Some(quote! { #keyword #decl; })
        } else {
            let stmts: Vec<TokenStream> = decls
                .into_iter()
                .map(|d| quote! { #keyword #d; })
                .collect();
            Some(quote! { #(#stmts)* })
        }
    }

    fn gen_var_decl_simple(&self, kind: VariableKind, name: &str, init_expr: &TokenStream) -> Option<TokenStream> {
        let id = syn::Ident::new(name, proc_macro2::Span::call_site());
        let keyword = self.var_keyword(kind);
        Some(quote! { #keyword #id = #init_expr; })
    }

    fn var_keyword(&self, kind: VariableKind) -> TokenStream {
        match kind {
            VariableKind::Var => quote! { let mut },
            VariableKind::Let => quote! { let },
            VariableKind::Const => quote! { let },
        }
    }


    /// Generate variable declarations from a pattern
    /// Returns a list of "name = value" expressions
    fn gen_pat(&self, pat: &Pat, source: &TokenStream) -> Vec<TokenStream> {
        match pat {
            Pat::Ident { name, .. } => {
                let id = syn::Ident::new(name, proc_macro2::Span::call_site());
                vec![quote! { #id = #source }]
            }
            Pat::Object { props, rest } => {
                self.gen_object_pat(props, rest, source)
            }
            Pat::Array { elems, rest } => {
                self.gen_array_pat(elems, rest, source)
            }
            Pat::Rest { arg: _ } => {
                // Rest pattern: ...rest means clone the source
                vec![quote! { rest = #source.clone() }]
            }
            Pat::Default { arg, default } => {
                // Default value: use unwrap_or with the default expression
                let default_expr = self.gen_expr(default);
                let inner_decls = self.gen_pat(arg, source);
                inner_decls
                    .into_iter()
                    .map(|decl| {
                        quote! { #decl.unwrap_or(#default_expr) }
                    })
                    .collect()
            }
            Pat::Assign { left, right } => {
                // a = b pattern: assign right to left
                let right_expr = self.gen_expr(right);
                self.gen_pat(left, &right_expr)
            }
        }
    }

    /// Generate variable declarations from object destructuring pattern
    fn gen_object_pat(
        &self,
        props: &[ObjectPatProp],
        rest: &Option<Box<Pat>>,
        source: &TokenStream,
    ) -> Vec<TokenStream> {
        let mut decls = Vec::new();

        for prop in props {
            match prop {
                ObjectPatProp::Init { key, value } => {
                    let field = syn::Ident::new(key, proc_macro2::Span::call_site());
                    let field_access = quote! { #source.#field };
                    let inner_decls = self.gen_pat(value, &field_access);
                    decls.extend(inner_decls);
                }
                ObjectPatProp::Rest { arg } => {
                    let inner_decls = self.gen_pat(arg, &quote! { #source.clone() });
                    decls.extend(inner_decls);
                }
                ObjectPatProp::Spread { arg } => {
                    let inner_decls = self.gen_pat(arg, source);
                    decls.extend(inner_decls);
                }
                ObjectPatProp::Method { .. } => {
                    // Skip method definitions in patterns
                }
            }
        }

        // Handle rest at object level
        if let Some(rest_pat) = rest {
            let inner_decls = self.gen_pat(rest_pat, &quote! { #source.clone() });
            decls.extend(inner_decls);
        }

        decls
    }

    /// Generate variable declarations from array destructuring pattern
    fn gen_array_pat(
        &self,
        elems: &[Option<Pat>],
        rest: &Option<Box<Pat>>,
        source: &TokenStream,
    ) -> Vec<TokenStream> {
        let mut decls = Vec::new();

        for (i, elem) in elems.iter().enumerate() {
            let index = syn::Index::from(i);
            let index_access = quote! { #source.#index };

            if let Some(pat) = elem {
                let inner_decls = self.gen_pat(pat, &index_access);
                decls.extend(inner_decls);
            }
        }

        // Handle rest
        if let Some(rest_pat) = rest {
            let len = elems.len();
            let rest_access = quote! { #source[#len..].to_vec() };
            let inner_decls = self.gen_pat(rest_pat, &rest_access);
            decls.extend(inner_decls);
        }

        decls
    }

    fn gen_while(&self, test: &Expr, body: &Box<Stmt>) -> TokenStream {
        let test_token = self.gen_expr(test);
        let body_token = self.gen_block_stmt(body);
        quote! {
            while #test_token {
                #body_token
            }
        }
    }

    #[allow(dead_code)]
    fn gen_do_while(&self, body: &Box<Stmt>, test: &Expr) -> TokenStream {
        let body_token = self.gen_block_stmt(body);
        let test_token = self.gen_expr(test);
        quote! {
            loop {
                #body_token
                if !(#test_token) { break; }
            }
        }
    }

    #[allow(dead_code)]
    fn gen_break(&self, label: &Option<String>) -> TokenStream {
        match label {
            Some(l) => {
                let label_id = syn::Ident::new(l, proc_macro2::Span::call_site());
                quote! { break #label_id; }
            }
            None => quote! { break; },
        }
    }

    #[allow(dead_code)]
    fn gen_continue(&self, label: &Option<String>) -> TokenStream {
        match label {
            Some(l) => {
                let label_id = syn::Ident::new(l, proc_macro2::Span::call_site());
                quote! { continue #label_id; }
            }
            None => quote! { continue; },
        }
    }

    #[allow(dead_code)]
    fn gen_throw(&self, arg: &Expr) -> TokenStream {
        let expr = self.gen_expr(arg);
        quote! { std::panic::panic_any(#expr); }
    }

    fn gen_try(&self, block: &super::Block, handler: &Option<super::CatchClause>, finalizer: &Option<super::Block>) -> TokenStream {
        let block_stmts: Vec<TokenStream> = block.0.iter().filter_map(|s| self.gen_stmt(s)).collect();
        match (handler, finalizer) {
            (Some(h), Some(f)) => self.gen_try_catch_finally(&block_stmts, h, f),
            (Some(h), None) => self.gen_try_catch(&block_stmts, h),
            (None, Some(f)) => self.gen_try_finally(&block_stmts, f),
            (None, None) => quote! { #(#block_stmts)* },
        }
    }

    fn gen_try_catch_finally(&self, block_stmts: &[TokenStream], handler: &super::CatchClause, finalizer: &super::Block) -> TokenStream {
        let catch_param = syn::Ident::new(&handler.param, proc_macro2::Span::call_site());
        let catch_body: Vec<TokenStream> = handler.body.0.iter().filter_map(|s| self.gen_stmt(s)).collect();
        let finally_stmts: Vec<TokenStream> = finalizer.0.iter().filter_map(|s| self.gen_stmt(s)).collect();
        quote! { { let __result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| { #(#block_stmts)* })); match __result { Ok(v) => v, Err(e) => { let #catch_param = e; #(#catch_body)* } }; #(#finally_stmts)* } }
    }

    fn gen_try_catch(&self, block_stmts: &[TokenStream], handler: &super::CatchClause) -> TokenStream {
        let catch_param = syn::Ident::new(&handler.param, proc_macro2::Span::call_site());
        let catch_body: Vec<TokenStream> = handler.body.0.iter().filter_map(|s| self.gen_stmt(s)).collect();
        quote! { { let __result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| { #(#block_stmts)* })); match __result { Ok(v) => v, Err(e) => { let #catch_param = e; #(#catch_body)* } } } }
    }

    fn gen_try_finally(&self, block_stmts: &[TokenStream], finalizer: &super::Block) -> TokenStream {
        let finally_stmts: Vec<TokenStream> = finalizer.0.iter().filter_map(|s| self.gen_stmt(s)).collect();
        quote! { { #(#block_stmts)* #(#finally_stmts)* } }
    }

    fn gen_block(&self, stmts: &[Stmt]) -> TokenStream {
        let inner: Vec<TokenStream> = stmts
            .iter()
            .filter_map(|s| self.gen_stmt(s))
            .collect();
        quote! {
            {
                #(#inner)*
            }
        }
    }

    #[allow(dead_code)]
    fn gen_labeled(&self, label: &str, body: &Box<Stmt>) -> TokenStream {
        let body_token = self.gen_block_stmt(body);
        // Rust labels are lifetimes like 'loop:
        let lifetime = syn::Lifetime::new(&format!("'{}", label), proc_macro2::Span::call_site());
        quote! {
            #lifetime: #body_token
        }
    }

    #[allow(dead_code)]
    fn gen_with(&self, obj: &Expr, body: &Box<Stmt>) -> TokenStream {
        let obj_token = self.gen_expr(obj);
        let body_token = self.gen_block_stmt(body);
        quote! {
            {
                let __with_obj = #obj_token;
                #body_token
            }
        }
    }

    fn gen_block_stmt(&self, stmt: &Stmt) -> TokenStream {
        match stmt {
            Stmt::Block { stmts } => {
                let inner: Vec<_> = stmts.iter()
                    .filter_map(|s| self.gen_stmt(s))
                    .collect();
                quote! { #(#inner)* }
            }
            _ => self.gen_stmt(stmt).unwrap_or_default(),
        }
    }
    
    pub(crate) fn gen_expr(&self, expr: &Expr) -> TokenStream {
        use super::Expr as E;
        match expr {
            E::String(_) | E::Number(_) | E::BigInt(_) => self.gen_lit_expr(expr),
            E::Boolean(_) | E::Null | E::Undefined => self.gen_bool_null(expr),
            E::RegExp { .. } => self.gen_reg_exp(expr),
            E::Super | E::This => self.gen_super_this(expr),
            E::Block(stmts) => self.gen_block_expr(stmts),
            E::Invalid => panic!("codegen for Invalid expression"),
            _ => self.gen_expr_helper(expr),
        }
    }

    fn gen_bool_null(&self, expr: &Expr) -> TokenStream {
        use super::Expr as E;
        match expr {
            E::Boolean(b) => self.gen_bool_expr(*b),
            E::Null | E::Undefined => quote! { Value::Null },
            _ => quote! { Value::Null },
        }
    }

    fn gen_super_this(&self, expr: &Expr) -> TokenStream {
        use super::Expr as E;
        match expr {
            E::Super => quote! { super },
            E::This => quote! { self },
            _ => quote! { Value::Null },
        }
    }

    fn gen_lit_expr(&self, expr: &Expr) -> TokenStream {
        use super::Expr as E;
        match expr {
            E::String(s) => self.gen_string_expr(s),
            E::Number(n) => self.gen_number_expr(n),
            E::BigInt(n) => quote! { Value::BigInt(#n) },
            _ => quote! { Value::Null },
        }
    }

    fn gen_reg_exp(&self, expr: &Expr) -> TokenStream {
        use super::Expr as E;
        if let E::RegExp { pattern, flags } = expr {
            quote! { Value::RegExp(#pattern, #flags) }
        } else {
            quote! { Value::Null }
        }
    }
}
