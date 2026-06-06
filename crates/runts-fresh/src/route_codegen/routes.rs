//! Route handling code

pub struct RouteHandler<'a> {
    modules: &'a [runts_hir::Module],
}

impl<'a> RouteHandler<'a> {
    pub fn new(modules: &'a [runts_hir::Module]) -> Self {
        Self { modules }
    }

    pub fn generate_route_code(&self, routes: &[&runts_plugin::RouteInfo]) -> (String, String, String) {
        let mut imports = String::new();
        let mut handlers = String::new();
        let mut router_calls = String::new();

        let module_by_path: std::collections::HashMap<String, &runts_hir::Module> =
            self.modules.iter().filter_map(|m| m.source_path.clone().map(|p| (p, m))).collect();

        for route in routes {
            let safe_name = self.module_name_from_path(&route.file_path);
            let axum_path = self.to_axum_path(&route.path);
            imports.push_str(&format!("mod {};\n", safe_name));

            let (render_arg, handler_sig) = self.make_handler_sig(&safe_name, &axum_path);
            handlers.push_str(&handler_sig);

            let route_methods = self.get_route_methods(route, &module_by_path);
            self.add_route_calls(&mut router_calls, &axum_path, &safe_name, &route_methods);
        }

        self.collect_middleware(&mut imports, &mut router_calls);
        (imports, handlers, router_calls)
    }

    fn make_handler_sig(&self, safe_name: &str, axum_path: &str) -> (String, String) {
        let dynamic_params: Vec<String> = axum_path.split('/')
            .filter_map(|seg| seg.strip_prefix(':').map(|s| s.to_string())).collect();

        let render_arg = match dynamic_params.len() {
            0 => "None".to_string(),
            1 => { let p = &dynamic_params[0]; format!("Some(&{p})") }
            _ => { let names = dynamic_params.join(", "); format!("Some((&({names})))") }
        };

        let handler_sig = self.build_handler_sig(safe_name, &dynamic_params, &render_arg);
        (render_arg, handler_sig)
    }

    fn build_handler_sig(&self, safe_name: &str, params: &[String], render_arg: &str) -> String {
        let render_call = format!("let v = {safe_name}::render({render_arg}); axum::response::Html(v.to_html())");
        match params.len() {
            0 => format!("async fn {safe_name}_handler() -> axum::response::Html<String> {{ {render_call} }}\n"),
            1 => { let p = &params[0]; format!("async fn {safe_name}_handler(Path({p}): Path<String>) -> axum::response::Html<String> {{ {render_call} }}\n") }
            _ => self.build_multi_param_handler(safe_name, params, &render_call),
        }
    }

    fn build_multi_param_handler(&self, safe_name: &str, params: &[String], render_call: &str) -> String {
        let names = params.join(", ");
        let types = (0..params.len()).map(|_| "String".to_string()).collect::<Vec<_>>().join(", ");
        let bind = format!("(({names}))");
        format!("async fn {safe_name}_handler(Path{bind}: Path<({types})>) -> axum::response::Html<String> {{ {render_call} }}\n")
    }

    fn get_route_methods(&self, route: &runts_plugin::RouteInfo, module_by_path: &std::collections::HashMap<String, &runts_hir::Module>) -> Vec<String> {
        let route_file = route.file_path.as_str();
        module_by_path.get(route_file).cloned()
            .or_else(|| module_by_path.values().find(|m| m.source_path.as_deref().map(|p| p.contains(route_file) || route_file.contains(p)).unwrap_or(false)).cloned())
            .map(|m| {
                let items_json = serde_json::to_value(&m.items).ok();
                items_json.map(|v| Self::extract_handler_methods(&v)).unwrap_or_default()
            })
            .unwrap_or_default()
    }

    fn extract_handler_methods(items: &serde_json::Value) -> Vec<String> {
        let mut methods = Vec::new();
        let items_arr = match items.as_array() { Some(a) => a, None => return methods };
        for item in items_arr {
            Self::process_handler_item(item, &mut methods);
        }
        methods
    }

    fn process_handler_item(item: &serde_json::Value, out: &mut Vec<String>) {
        let obj = match item.as_object() { Some(o) => o, None => return };
        if let Some(decl) = obj.get("Decl") {
            if let Some(var) = decl.get("Variable") {
                if Self::is_handler_variable(var) {
                    Self::extract_from_var(var, out);
                }
            }
        }
    }

    fn is_handler_variable(variable: &serde_json::Value) -> bool {
        let obj = match variable.as_object() { Some(o) => o, None => return false };
        if let Some(init) = obj.get("init") {
            if let Some(member) = init.get("Member") {
                if let Some(obj) = member.get("obj") {
                    if let Some(obj_str) = obj.as_str() {
                        return obj_str == "handlers" || obj_str == "Handler" || obj_str == "router";
                    }
                }
            }
        }
        false
    }

    fn extract_from_var(var: &serde_json::Value, out: &mut Vec<String>) {
        let obj = match var.as_object() { Some(o) => o, None => return };
        if let Some(init) = obj.get("init") {
            if let Some(member) = init.get("Member") {
                Self::extract_method_from_member(member, out);
            }
        }
    }

    fn extract_method_from_member(member: &serde_json::Value, out: &mut Vec<String>) {
        let obj = match member.as_object() { Some(o) => o, None => return };
        if let Some(prop) = obj.get("property") {
            if let Some(prop_val) = Self::get_handler_method_value(prop) {
                if let Some(method_name) = prop_val.as_str() {
                    if method_name.starts_with("on") || method_name.starts_with("handle") {
                        let http_method = method_name.trim_start_matches("on").trim_start_matches("handle");
                        Self::add_http_method_str(http_method, out);
                    }
                }
            }
        }
    }

    fn get_handler_method_value<'b>(prop: &'b serde_json::Value) -> Option<&'b serde_json::Value> {
        prop.as_str().map(|_| prop).or_else(|| prop.get("$method")).or_else(|| prop.get("value"))
    }

    fn add_http_method_str(method: &str, out: &mut Vec<String>) {
        static METHODS: &[(&str, &str)] = &[
            ("GET", "GET"), ("POST", "POST"), ("PUT", "PUT"), ("DELETE", "DELETE"),
            ("PATCH", "PATCH"), ("HEAD", "HEAD"), ("OPTIONS", "OPTIONS"),
        ];
        let upper = method.to_uppercase();
        if let Some((_, m)) = METHODS.iter().find(|(k, _)| *k == upper) { out.push(m.to_string()); }
    }

    fn add_route_calls(&self, router_calls: &mut String, axum_path: &str, safe_name: &str, methods: &[String]) {
        if methods.is_empty() {
            router_calls.push_str(&format!("        .route(\"{}\", axum::routing::get({}_handler))\n", axum_path, safe_name));
        } else {
            for method in methods {
                let axum_name = Self::method_to_axum(method);
                router_calls.push_str(&format!("        .route(\"{}\", axum::routing::{}({}_handler))\n", axum_path, axum_name, safe_name));
            }
        }
    }

    fn method_to_axum(method: &str) -> &'static str {
        match method { "GET" => "get", "POST" => "post", "PUT" => "put", "DELETE" => "delete", "PATCH" => "patch", "HEAD" => "head", "OPTIONS" => "options", _ => "get" }
    }

    fn collect_middleware(&self, imports: &mut String, router_calls: &mut String) {
        for m in self.modules {
            if let Some(path) = &m.source_path {
                if Self::is_middleware_file(path) {
                    let name = path.rsplit_once('/').map(|(_, l)| l).unwrap_or(path)
                        .replace(".ts", "").replace(".tsx", "");
                    imports.push_str(&format!("mod {};\n", name));
                    router_calls.push_str(&format!("        .layer(axum::middleware::from_fn({}::{}_middleware))\n", name, name));
                }
            }
        }
    }

    fn is_middleware_file(path: &str) -> bool {
        let leaf = path.rsplit_once('/').map(|(_, l)| l).unwrap_or(path);
        leaf == "_middleware.ts" || leaf == "_middleware.tsx"
    }

    fn module_name_from_path(&self, path: &str) -> String {
        path.replace('/', "_").replace(".ts", "").replace(".tsx", "")
    }

    fn to_axum_path(&self, path: &str) -> String {
        path.replace("{", "<").replace("}", ">")
    }
}
