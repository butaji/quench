//! Basic TypeScript/TSX parser for runts

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::fs;

use super::hir::*;

/// Simple parser for TypeScript/TSX
pub struct Parser {
    source: String,
    pos: usize,
}

impl Parser {
    pub fn new() -> Self {
        Self {
            source: String::new(),
            pos: 0,
        }
    }

    /// Parse from source string (for testing)
    pub fn parse_source(&mut self, source: &str) -> Result<Module> {
        self.source = source.to_string();
        self.pos = 0;
        self.parse_module()
    }

    /// Parse a TypeScript/TSX file
    pub fn parse_file(&mut self, path: &PathBuf) -> Result<Module> {
        let source = fs::read_to_string(path)
            .context("Failed to read file")?;

        self.source = source;
        self.pos = 0;

        self.parse_module()
    }

    pub fn parse_module(&mut self) -> Result<Module> {
        let mut items = Vec::new();

        if self.source.starts_with('\u{feff}') {
            self.pos = 1;
        }

        self.skip_ws_and_comments();

        while !self.is_at_end() {
            match self.current() {
                'i' if self.check_word("import") => {
                    if let Some(import) = self.parse_import()? {
                        items.push(ModuleItem::Import(import));
                    }
                }
                'e' if self.check_word("export") => {
                    if let Some(export) = self.parse_export()? {
                        items.push(ModuleItem::Export(export));
                    }
                }
                't' if self.check_word("type") => {
                    if let Some(decl) = self.parse_type_alias()? {
                        items.push(ModuleItem::Decl(Decl::Type(decl)));
                    }
                }
                'a' if self.check_word("async") => {
                    // Check if it's async function
                    self.advance_by(5);  // Skip "async"
                    self.skip_ws_and_comments();
                    let ahead = self.source[self.pos..].trim_start();
                    if ahead.starts_with("function") {
                        // Skip "function" keyword before calling parse_function
                        self.advance_by(8);
                        self.skip_ws_and_comments();
                        if let Some(decl) = self.parse_function_after_keyword()? {
                            // Set is_async to true since we already consumed "async"
                            let mut decl = decl;
                            decl.is_async = true;
                            items.push(ModuleItem::Decl(Decl::Function(decl)));
                        }
                    } else {
                        // Not async function, skip this statement
                        self.skip_statement();
                    }
                }
                'f' if self.check_word("function") => {
                    if let Some(decl) = self.parse_function()? {
                        items.push(ModuleItem::Decl(Decl::Function(decl)));
                    }
                }
                'i' if self.check_word("interface") => {
                    if let Some(decl) = self.parse_interface()? {
                        items.push(ModuleItem::Decl(Decl::Type(decl)));
                    }
                }
                'c' | 'l' | 'v' if self.is_var_keyword() => {
                    if let Some(decl) = self.parse_variable_decl()? {
                        items.push(ModuleItem::Decl(Decl::Variable(decl)));
                    }
                }
                'd' if self.check_word("default") => {
                    if let Some(export) = self.parse_default_export()? {
                        items.push(ModuleItem::Export(export));
                    }
                }
                _ => {
                    // Check if we're at a closing brace (end of block/statement)
                    if self.current() == '}' {
                        break;
                    }
                    self.skip_statement();
                }
            }
            self.skip_ws_and_comments();
        }

        Ok(Module {
            source: String::new(),
            items,
            types: Default::default(),
        })
    }

    fn parse_import(&mut self) -> Result<Option<Import>> {
        if !self.check_word("import") {
            return Ok(None);
        }
        self.advance_by(6);
        self.skip_ws_and_comments();

        let type_only = if self.check_word("type") {
            self.advance_by(4);
            self.skip_ws_and_comments();
            true
        } else {
            false
        };

        if self.current() == '"' || self.current() == '\'' {
            let source = self.parse_string()?;
            self.skip_ws_and_comments();
            self.expect(';')?;
            return Ok(Some(Import {
                source,
                specifiers: vec![],
                type_only,
            }));
        }

        let mut specifiers = Vec::new();

        if self.current() == '{' {
            self.advance();
            self.skip_ws_and_comments();
            while !self.check('}') {
                if self.check(',') {
                    self.advance();
                    self.skip_ws_and_comments();
                    continue;
                }
                let name = self.parse_identifier()?;
                let alias = if self.skip_word("as") {
                    self.skip_ws_and_comments();
                    Some(self.parse_identifier()?)
                } else {
                    None
                };
                specifiers.push(ImportSpecifier::Named { name, alias });
                self.skip_ws_and_comments();
                if self.check(',') {
                    self.advance();
                    self.skip_ws_and_comments();
                }
            }
            self.expect('}')?;
        } else if self.is_ident_char(self.current()) || self.current().is_alphabetic() {
            let name = self.parse_identifier()?;
            specifiers.push(ImportSpecifier::Default { name });
        }

        self.skip_ws_and_comments();
        self.skip_word("from");
        self.skip_ws_and_comments();

        let source = self.parse_string()?;
        self.skip_ws_and_comments();
        self.expect(';')?;

        Ok(Some(Import {
            source,
            specifiers,
            type_only,
        }))
    }

    fn parse_export(&mut self) -> Result<Option<Export>> {
        if !self.check_word("export") {
            return Ok(None);
        }
        self.advance_by(6);
        self.skip_ws_and_comments();

        if self.check_word("type") {
            self.advance_by(4);
            self.skip_ws_and_comments();
        }

        if self.check('{') {
            let mut names = Vec::new();
            self.advance();
            self.skip_ws_and_comments();
            while !self.check('}') {
                if self.check(',') {
                    self.advance();
                    continue;
                }
                names.push(self.parse_identifier()?);
                self.skip_ws_and_comments();
                if self.check(',') {
                    self.advance();
                    self.skip_ws_and_comments();
                }
            }
            self.expect('}')?;
            self.skip_ws_and_comments();
            self.expect(';')?;
            return Ok(Some(Export::ReExport {
                source: String::new(),
                names,
            }));
        }

        if self.check_word("const") || self.check_word("let") || self.check_word("var") {
            // Skip past the keyword
            if self.check_word("const") {
                self.advance_by(5);
            } else if self.check_word("let") {
                self.advance_by(3);
            } else {
                self.advance_by(3);
            }
            self.skip_ws_and_comments();
            
            // Parse the rest: name = value
            let name = self.parse_identifier()?;
            self.skip_ws_and_comments();
            
            // Parse the initializer - capture full expression for route handlers
            let value = if self.check('=') {
                self.advance();
                self.skip_ws_and_comments();
                Some(self.parse_expression()?)
            } else {
                None
            };
            
            self.skip_ws_and_comments();
            if self.check(';') {
                self.advance();
            }
            
            // Return as Named export with value for handlers
            if let Some(expr) = value {
                return Ok(Some(Export::NamedWithValue { 
                    name: name.clone(), 
                    value: expr 
                }));
            }
            
            return Ok(Some(Export::Named { name }));
        }

        if self.check_word("default") {
            self.advance_by(7);
            self.skip_ws_and_comments();
            return self.parse_default_export();
        }

        if self.check_word("function") {
            self.advance_by(8);
            self.skip_ws_and_comments();
            let name = self.parse_identifier()?;
            return Ok(Some(Export::Named { name }));
        }

        Ok(None)
    }

    fn parse_default_export(&mut self) -> Result<Option<Export>> {
        // Check if it's an export default function
        self.skip_ws_and_comments();
        if self.check_word("function") {
            // Parse function declaration and store the full function expression
            if let Some(func) = self.parse_function()? {
                return Ok(Some(Export::Default { 
                    expr: Expr::Function { decl: func }
                }));
            }
        }
        
        let expr = self.parse_expression()?;
        self.skip_ws_and_comments();
        if self.check(';') {
            self.advance();
        }
        Ok(Some(Export::Default { expr }))
    }

    fn parse_type_alias(&mut self) -> Result<Option<TypeDecl>> {
        if !self.check_word("type") {
            return Ok(None);
        }
        self.advance_by(4);
        self.skip_ws_and_comments();

        let name = self.parse_identifier()?;
        self.skip_ws_and_comments();

        let mut generics = Vec::new();
        if self.check('<') {
            self.advance();
            self.skip_ws_and_comments();
            while !self.check('>') {
                let param = self.parse_identifier()?;
                generics.push(GenericParam {
                    name: param,
                    bound: None,
                    default: None,
                });
                self.skip_ws_and_comments();
                if self.check(',') {
                    self.advance();
                    self.skip_ws_and_comments();
                }
            }
            self.expect('>')?;
        }

        self.skip_ws_and_comments();
        self.expect('=')?;
        self.skip_ws_and_comments();

        let type_ = self.parse_type()?;

        self.skip_ws_and_comments();
        self.expect(';')?;

        Ok(Some(TypeDecl {
            name,
            generics,
            type_,
        }))
    }

    fn parse_interface(&mut self) -> Result<Option<TypeDecl>> {
        if !self.check_word("interface") {
            return Ok(None);
        }
        self.advance_by(9);
        self.skip_ws_and_comments();

        let name = self.parse_identifier()?;
        self.skip_ws_and_comments();

        let mut generics = Vec::new();
        if self.check('<') {
            self.advance();
            self.skip_ws_and_comments();
            while !self.check('>') {
                let param = self.parse_identifier()?;
                generics.push(GenericParam {
                    name: param,
                    bound: None,
                    default: None,
                });
                self.skip_ws_and_comments();
                if self.check(',') {
                    self.advance();
                    self.skip_ws_and_comments();
                }
            }
            self.expect('>')?;
        }

        self.skip_ws_and_comments();
        let members = self.parse_object_type_members()?;

        Ok(Some(TypeDecl {
            name,
            generics,
            type_: Type::Object { members },
        }))
    }

    fn parse_function(&mut self) -> Result<Option<FunctionDecl>> {
        if !self.check_word("function") {
            return Ok(None);
        }
        self.advance_by(8);
        self.skip_ws_and_comments();

        let name = self.parse_identifier()?;
        self.skip_ws_and_comments();

        let generics = self.parse_generic_params()?;

        self.expect('(')?;
        let params = self.parse_params()?;
        self.expect(')')?;
        self.skip_ws_and_comments();

        let return_type = if self.check(':') {
            self.advance();
            self.skip_ws_and_comments();
            Some(self.parse_type()?)
        } else {
            None
        };

        self.skip_ws_and_comments();

        let is_async = self.check_word("async");
        if is_async {
            self.advance_by(5);
            self.skip_ws_and_comments();
        }

        let body = if self.check('{') {
            Some(self.parse_block()?)
        } else {
            None
        };

        Ok(Some(FunctionDecl {
            name,
            generics,
            params,
            return_type,
            body,
            is_async,
            is_generator: false,
            decorators: vec![],
        }))
    }

    /// Parse function after "function" keyword has been consumed
    fn parse_function_after_keyword(&mut self) -> Result<Option<FunctionDecl>> {
        // Position is already after "function" keyword
        
        let name = self.parse_identifier()?;
        self.skip_ws_and_comments();

        let generics = self.parse_generic_params()?;

        self.expect('(')?;
        let params = self.parse_params()?;
        self.expect(')')?;
        self.skip_ws_and_comments();

        let return_type = if self.check(':') {
            self.advance();
            self.skip_ws_and_comments();
            Some(self.parse_type()?)
        } else {
            None
        };

        self.skip_ws_and_comments();

        // Don't check for async here - it's already been consumed by caller
        let is_async = false;  // Will be set by caller

        let body = if self.check('{') {
            Some(self.parse_block()?)
        } else {
            None
        };

        Ok(Some(FunctionDecl {
            name,
            generics,
            params,
            return_type,
            body,
            is_async,
            is_generator: false,
            decorators: vec![],
        }))
    }

    fn parse_variable_decl(&mut self) -> Result<Option<VariableDecl>> {
        let kind = if self.check_word("const") {
            self.advance_by(5);
            VariableKind::Const
        } else if self.check_word("let") {
            self.advance_by(3);
            VariableKind::Let
        } else if self.check_word("var") {
            self.advance_by(3);
            VariableKind::Var
        } else {
            return Ok(None);
        };

        self.skip_ws_and_comments();
        
        // Check for destructuring patterns
        let (name, pattern) = if self.check('{') {
            // Object destructuring: const { a, b } = obj
            let pattern = self.parse_destructuring_pattern()?;
            (String::from("_destructured"), Some(pattern))
        } else if self.check('[') {
            // Array destructuring: const [first, ...rest] = arr
            let pattern = self.parse_array_destructuring_pattern()?;
            (String::from("_destructured"), Some(pattern))
        } else {
            let name = self.parse_identifier()?;
            (name, None)
        };
        
        self.skip_ws_and_comments();

        let type_ = if self.check(':') {
            self.advance();
            self.skip_ws_and_comments();
            Some(self.parse_type()?)
        } else {
            None
        };

        self.skip_ws_and_comments();

        let init = if self.check('=') {
            self.advance();
            self.skip_ws_and_comments();
            Some(self.parse_expression()?)
        } else {
            None
        };

        self.skip_ws_and_comments();
        if self.check(';') {
            self.advance();
        }

        Ok(Some(VariableDecl {
            name,
            kind,
            type_,
            init,
            pattern,
        }))
    }

    /// Parse a destructuring pattern { a, b, ...rest }
    fn parse_destructuring_pattern(&mut self) -> Result<Pat> {
        self.expect('{')?;
        self.skip_ws_and_comments();
        
        let mut props = Vec::new();
        
        while !self.check('}') {
            self.skip_ws_and_comments();
            if self.check('}') { break; }
            
            if self.check_str("...") {
                self.advance_by(3);
                self.skip_ws_and_comments();
                let rest_name = self.parse_identifier()?;
                props.push(ObjectPatProp::Rest { arg: Box::new(Pat::Ident { name: rest_name, type_: None }) });
            } else {
                let key = self.parse_identifier()?;
                self.skip_ws_and_comments();
                
                // Skip default value if present: { key = default }
                if self.check('=') {
                    self.advance();
                    self.skip_ws_and_comments();
                    // Skip over the default expression (simple for now)
                    while !self.is_at_end() && self.current() != ',' && self.current() != '}' {
                        self.advance();
                    }
                    self.skip_ws_and_comments();
                }
                
                // Check for alias: { key: alias }
                let value: Pat = if self.check(':') {
                    self.advance();
                    self.skip_ws_and_comments();
                    if self.check('{') {
                        // Nested object destructuring
                        self.parse_destructuring_pattern()?
                    } else if self.check('[') {
                        // Nested array destructuring
                        self.parse_array_destructuring_pattern()?
                    } else {
                        let name = self.parse_identifier()?;
                        Pat::Ident { name, type_: None }
                    }
                } else {
                    // Shorthand: { key }
                    Pat::Ident { name: key.clone(), type_: None }
                };
                
                props.push(ObjectPatProp::Init { key, value });
            }
            
            self.skip_ws_and_comments();
            if self.check(',') {
                self.advance();
                self.skip_ws_and_comments();
            }
        }
        
        self.expect('}')?;
        Ok(Pat::Object { props, rest: None })
    }

    /// Parse an array destructuring pattern [ first, ...rest ]
    fn parse_array_destructuring_pattern(&mut self) -> Result<Pat> {
        self.expect('[')?;
        self.skip_ws_and_comments();
        
        let mut elems = Vec::new();
        let mut rest: Option<Box<Pat>> = None;
        
        while !self.check(']') {
            self.skip_ws_and_comments();
            if self.check(']') { break; }
            
            if self.check_str("...") {
                self.advance_by(3);
                self.skip_ws_and_comments();
                let rest_name = self.parse_identifier()?;
                rest = Some(Box::new(Pat::Ident { name: rest_name, type_: None }));
                
                // Skip to end of array pattern
                while !self.check(']') && !self.is_at_end() {
                    self.advance();
                }
                break;
            }
            
            // Handle elision (skipping elements): [, , b] or [a, ,]
            let has_leading_comma = elems.is_empty() && self.check(',') && self.peek() != Some(']');
            
            let elem: Option<Pat> = if self.check('{') {
                Some(self.parse_destructuring_pattern()?)
            } else if self.check('[') {
                Some(self.parse_array_destructuring_pattern()?)
            } else if self.check(',') || has_leading_comma {
                // Elision: skip element
                None
            } else {
                let name = self.parse_identifier()?;
                Some(Pat::Ident { name, type_: None })
            };
            
            elems.push(elem);
            
            self.skip_ws_and_comments();
            if self.check(',') {
                self.advance();
                self.skip_ws_and_comments();
            }
        }
        
        self.expect(']')?;
        Ok(Pat::Array { elems, rest })
    }

    fn parse_type(&mut self) -> Result<Type> {
        self.skip_ws_and_comments();

        let base: Type = match self.current() {
            's' if self.check_word("string") => { self.advance_by(6); Type::String }
            'n' if self.check_word("number") => { self.advance_by(6); Type::Number }
            'b' if self.check_word("boolean") => { self.advance_by(7); Type::Boolean }
            'v' if self.check_word("void") => { self.advance_by(4); Type::Void }
            'n' if self.check_word("null") => { self.advance_by(4); Type::Null }
            'u' if self.check_word("undefined") => { self.advance_by(9); Type::Undefined }
            'n' if self.check_word("never") => { self.advance_by(5); Type::Never }
            'u' if self.check_word("unknown") => { self.advance_by(7); Type::Unknown }
            'a' if self.check_word("any") => { self.advance_by(3); Type::Any }
            '{' => { let members = self.parse_object_type_members()?; Type::Object { members } }
            '(' => {
                self.advance();
                let mut types = Vec::new();
                while !self.check(')') {
                    types.push(self.parse_type()?);
                    self.skip_ws_and_comments();
                    if self.check(',') { self.advance(); }
                }
                self.expect(')')?;
                Type::Tuple { types }
            }
            '<' => {
                self.advance();
                let mut generics = Vec::new();
                while !self.check('>') {
                    generics.push(self.parse_type()?);
                    self.skip_ws_and_comments();
                    if self.check(',') { self.advance(); }
                }
                self.expect('>')?;
                Type::Ref { name: "T".to_string(), generics }
            }
            _ => {
                let name = self.parse_identifier()?;
                let generics = if self.check('<') {
                    self.advance();
                    let mut args = Vec::new();
                    while !self.check('>') {
                        args.push(self.parse_type()?);
                        self.skip_ws_and_comments();
                        if self.check(',') { self.advance(); }
                    }
                    self.expect('>')?;
                    args
                } else {
                    vec![]
                };
                Type::Ref { name, generics }
            }
        };

        // Handle array type suffix: number[] -> Array<number>
        self.skip_ws_and_comments();
        let mut elem = base;
        while self.check('[') {
            self.advance();
            self.expect(']').ok();
            elem = Type::Array { elem: Box::new(elem) };
        }

        // Handle union and intersection types
        self.skip_ws_and_comments();
        let mut types = vec![elem];
        let mut is_union = false;
        let mut _is_intersection = false;
        loop {
            if self.check('|') && self.peek() != Some('|') {
                self.advance();
                self.skip_ws_and_comments();
                types.push(self.parse_type()?);
                is_union = true;
            } else if self.check('&') && self.peek() != Some('&') {
                self.advance();
                self.skip_ws_and_comments();
                types.push(self.parse_type()?);
                _is_intersection = true;
            } else {
                break;
            }
        }

        if types.len() == 1 {
            Ok(types.into_iter().next().unwrap())
        } else if is_union {
            Ok(Type::Union { types })
        } else {
            Ok(Type::Intersection { types })
        }
    }

    fn parse_object_type_members(&mut self) -> Result<Vec<ObjectMember>> {
        self.expect('{')?;
        self.skip_ws_and_comments();

        let mut members = Vec::new();

        while !self.check('}') {
            self.skip_ws_and_comments();

            let mut optional = false;
            let mut readonly = false;

            while self.current() == '?' || self.check_word("readonly") {
                if self.check('?') { self.advance(); optional = true; }
                if self.check_word("readonly") { self.advance_by(8); readonly = true; }
                self.skip_ws_and_comments();
            }

            let key = self.parse_identifier()?;
            self.skip_ws_and_comments();

            if self.check('?') { self.advance(); optional = true; }
            self.skip_ws_and_comments();
            self.expect(':')?;
            self.skip_ws_and_comments();

            let type_ = self.parse_type()?;

            members.push(ObjectMember {
                key,
                type_,
                optional,
                readonly,
            });

            self.skip_ws_and_comments();
            if self.check(';') || self.check(',') { self.advance(); }
            self.skip_ws_and_comments();
        }

        self.expect('}')?;
        Ok(members)
    }

    fn parse_generic_params(&mut self) -> Result<Vec<GenericParam>> {
        let mut params = Vec::new();
        if self.check('<') {
            self.advance();
            self.skip_ws_and_comments();
            while !self.check('>') {
                let name = self.parse_identifier()?;
                params.push(GenericParam {
                    name,
                    bound: None,
                    default: None,
                });
                self.skip_ws_and_comments();
                if self.check(',') { self.advance(); self.skip_ws_and_comments(); }
            }
            self.expect('>')?;
        }
        Ok(params)
    }

    /// Check if what follows < looks like type arguments.
    /// This helps distinguish <T> (type args) from < x (comparison).
    fn looks_like_type_arg(&self) -> bool {
        let after_lt = &self.source[self.pos..].trim_start();
        if after_lt.is_empty() {
            return false;
        }
        
        let first_char = after_lt.chars().next().unwrap();
        
        // Type keywords
        let type_keywords = [
            "number", "string", "boolean", "void", "null", "undefined", 
            "any", "never", "unknown", "symbol", "bigint",
            "interface", "type", "readonly", "keyof", "typeof",
        ];
        
        // Uppercase means type name
        if first_char.is_uppercase() {
            return true;
        }
        
        // Type keywords
        for kw in type_keywords {
            if after_lt.starts_with(kw) {
                let after_kw = &after_lt[kw.len()..];
                if after_kw.is_empty() || !after_kw.chars().next().unwrap().is_alphanumeric() {
                    return true;
                }
            }
        }
        
        // Structural starters
        if first_char == '{' || first_char == '[' || first_char == '(' {
            return true;
        }
        
        // Lowercase identifier (generic param like <T>)
        if first_char.is_lowercase() && first_char.is_alphabetic() {
            return true;
        }
        
        false
    }

    /// Parse type arguments for function calls (e.g., useState<T>)
    fn parse_type_args(&mut self) -> Result<Vec<Type>> {
        if !self.check('<') {
            return Ok(Vec::new());
        }
        
        let saved_pos = self.pos;
        self.advance();
        
        if !self.looks_like_type_arg() {
            self.pos = saved_pos;
            return Ok(Vec::new());
        }
        
        self.skip_ws_and_comments();
        
        let mut args = Vec::new();
        while !self.check('>') {
            if self.is_at_end() {
                break;
            }
            args.push(self.parse_type()?);
            self.skip_ws_and_comments();
            if self.check(',') { self.advance(); self.skip_ws_and_comments(); }
        }
        self.expect('>')?;
        Ok(args)
    }

    fn parse_params(&mut self) -> Result<Vec<Param>> {
        let mut params = Vec::new();

        while !self.check(')') {
            if self.check(',') { self.advance(); self.skip_ws_and_comments(); continue; }
            self.skip_ws_and_comments();

            // Handle destructuring patterns like { name } or [first, second]
            let (name, pattern) = if self.check('{') {
                // Object destructuring: { a, b = default } -> pattern contains the destructuring
                let pattern = self.parse_destructuring_pattern()?;
                // Use a placeholder name that we'll use as the source variable
                ("_props".to_string(), Some(pattern))
            } else if self.check('[') {
                // Array destructuring: [first, second]
                let pattern = self.parse_array_destructuring_pattern()?;
                ("_props".to_string(), Some(pattern))
            } else {
                let name = self.parse_identifier()?;
                (name, None)
            };
            self.skip_ws_and_comments();

            let optional = self.check('?');
            if optional { self.advance(); }

            let type_ = if self.check(':') {
                self.advance();
                self.skip_ws_and_comments();
                Some(self.parse_type()?)
            } else {
                None
            };

            let default = if self.check('=') {
                self.advance();
                self.skip_ws_and_comments();
                Some(self.parse_expression()?)
            } else {
                None
            };

            params.push(Param {
                name,
                type_,
                default,
                optional,
                pattern,
            });

            self.skip_ws_and_comments();
            if self.check(',') { self.advance(); }
        }

        Ok(params)
    }

    fn parse_block(&mut self) -> Result<Block> {
        self.expect('{')?;
        let mut stmts = Vec::new();

        loop {
            self.skip_ws_and_comments();
            if self.check('}') { break; }
            if let Some(stmt) = self.parse_statement()? {
                stmts.push(stmt);
            }
            self.skip_ws_and_comments();
        }

        self.expect('}')?;
        Ok(Block(stmts))
    }

    fn parse_statement(&mut self) -> Result<Option<Stmt>> {
        self.skip_ws_and_comments();

        if self.check_word("const") || self.check_word("let") || self.check_word("var") {
            return Ok(Some(Stmt::Variable {
                decl: self.parse_variable_decl()?.unwrap(),
            }));
        }

        if self.check_word("function") {
            return Ok(Some(Stmt::Function {
                decl: self.parse_function()?.unwrap(),
            }));
        }

        if self.check('{') {
            return Ok(Some(Stmt::Block(self.parse_block()?.0)));
        }

        if self.check_word("return") {
            self.advance_by(6);
            self.skip_ws_and_comments();
            let arg = if self.check(';') || self.check('}') { None } else { Some(self.parse_expression()?) };
            self.skip_ws_and_comments();
            if self.check(';') { self.advance(); }
            return Ok(Some(Stmt::Return { arg }));
        }

        if self.check_word("if") {
            self.advance_by(2);
            self.skip_ws_and_comments();
            self.expect('(')?;
            let test = self.parse_expression()?;
            self.expect(')')?;
            self.skip_ws_and_comments();
            let consequent = Box::new(self.parse_statement()?.unwrap_or(Stmt::Empty));
            self.skip_ws_and_comments();
            let alternate = if self.check_word("else") {
                self.advance_by(4);
                self.skip_ws_and_comments();
                Some(Box::new(self.parse_statement()?.unwrap_or(Stmt::Empty)))
            } else {
                None
            };
            return Ok(Some(Stmt::If { test, consequent, alternate }));
        }

        if self.check_word("for") {
            self.advance_by(3);
            self.skip_ws_and_comments();
            self.expect('(')?;
            self.skip_balanced('(', ')');
            self.skip_ws_and_comments();
            let body = Box::new(self.parse_statement()?.unwrap_or(Stmt::Empty));
            return Ok(Some(Stmt::For {
                init: None,
                test: None,
                update: None,
                body,
            }));
        }

        if self.check_word("while") {
            self.advance_by(5);
            self.skip_ws_and_comments();
            self.expect('(')?;
            let test = self.parse_expression()?;
            self.expect(')')?;
            self.skip_ws_and_comments();
            let body = Box::new(self.parse_statement()?.unwrap_or(Stmt::Empty));
            return Ok(Some(Stmt::While { test, body }));
        }

        if self.check(';') { self.advance(); return Ok(Some(Stmt::Empty)); }

        let expr = self.parse_expression()?;
        self.skip_ws_and_comments();
        if self.check(';') { self.advance(); }
        Ok(Some(Stmt::Expr { expr }))
    }

    fn parse_expression(&mut self) -> Result<Expr> {
        self.parse_assignment()
    }

    fn parse_assignment(&mut self) -> Result<Expr> {
        let left = self.parse_ternary()?;
        self.skip_ws_and_comments();

        if self.check_str("=>") {
            // Extract params from left side: ident or sequence expr
            let params = match left {
                Expr::Ident { ref name } => {
                    vec![Param { name: name.clone(), type_: None, default: None, optional: false, pattern: None }]
                }
                Expr::Seq { ref exprs } => {
                    exprs.iter().filter_map(|e| match e {
                        Expr::Ident { name } => Some(Param { name: name.clone(), type_: None, default: None, optional: false, pattern: None }),
                        _ => None,
                    }).collect()
                }
                _ => vec![],
            };
            return self.parse_arrow_function_with_params(params);
        }

        let op = if self.check('=') && !self.check_str("==") && !self.check_str("===") {
            Some(AssignOp::Assign)
        } else if self.check_str("+=") {
            Some(AssignOp::AddAssign)
        } else if self.check_str("-=") {
            Some(AssignOp::SubAssign)
        } else {
            None
        };

        if let Some(op) = op {
            self.advance_by(if op == AssignOp::Assign { 1 } else { 2 });
            self.skip_ws_and_comments();
            let right = Box::new(self.parse_expression()?);
            return Ok(Expr::Assign { op, left: Box::new(left), right });
        }

        Ok(left)
    }

    fn parse_arrow_function(&mut self) -> Result<Expr> {
        self.parse_arrow_function_with_params(vec![])
    }

    fn parse_arrow_function_with_params(&mut self, params: Vec<Param>) -> Result<Expr> {
        // We're at '=>', consume it
        self.expect_str("=>")?;
        self.skip_ws_and_comments();
        
        // Parse the body
        let body: Stmt = if self.check('{') {
            // Block body
            Stmt::Block(self.parse_block()?.0)
        } else {
            // Expression body - wrap in return statement
            let expr = self.parse_expression()?;
            Stmt::Return { arg: Some(expr) }
        };
        
        Ok(Expr::Arrow {
            params,
            body: Box::new(body),
            is_async: false,
        })
    }

    fn parse_ternary(&mut self) -> Result<Expr> {
        let test = self.parse_nullish_coalesce()?;
        self.skip_ws_and_comments();

        if self.check('?') {
            self.advance();
            self.skip_ws_and_comments();
            let consequent = Box::new(self.parse_expression()?);
            self.skip_ws_and_comments();
            self.expect(':')?;
            self.skip_ws_and_comments();
            let alternate = Box::new(self.parse_expression()?);
            return Ok(Expr::Cond { test: Box::new(test), consequent, alternate });
        }

        Ok(test)
    }

    fn parse_logical_or(&mut self) -> Result<Expr> {
        let left = self.parse_logical_and()?;
        self.skip_ws_and_comments();

        if self.check_str("||") {
            self.advance_by(2);
            self.skip_ws_and_comments();
            let right = Box::new(self.parse_logical_or()?);
            return Ok(Expr::Logical { op: LogicalOp::Or, left: Box::new(left), right });
        }

        Ok(left)
    }

    fn parse_nullish_coalesce(&mut self) -> Result<Expr> {
        let left = self.parse_logical_or()?;
        self.skip_ws_and_comments();

        if self.check_str("??") {
            self.advance_by(2);
            self.skip_ws_and_comments();
            let right = Box::new(self.parse_nullish_coalesce()?);
            return Ok(Expr::Logical { op: LogicalOp::NullishCoalesce, left: Box::new(left), right });
        }

        Ok(left)
    }

    fn parse_logical_and(&mut self) -> Result<Expr> {
        let left = self.parse_equality()?;
        self.skip_ws_and_comments();

        if self.check_str("&&") {
            self.advance_by(2);
            self.skip_ws_and_comments();
            let right = Box::new(self.parse_logical_and()?);
            return Ok(Expr::Logical { op: LogicalOp::And, left: Box::new(left), right });
        }

        Ok(left)
    }

    fn parse_equality(&mut self) -> Result<Expr> {
        let left = self.parse_comparison()?;
        self.skip_ws_and_comments();

        let op = if self.check_str("===") { BinaryOp::EqStrict }
        else if self.check_str("!==") { BinaryOp::NeStrict }
        else if self.check_str("==") { BinaryOp::Eq }
        else if self.check_str("!=") { BinaryOp::Ne }
        else { return Ok(left); };

        self.advance_by(if op == BinaryOp::Eq || op == BinaryOp::Ne { 2 } else { 3 });
        self.skip_ws_and_comments();
        let right = Box::new(self.parse_equality()?);
        Ok(Expr::Bin { op, left: Box::new(left), right })
    }

    fn parse_comparison(&mut self) -> Result<Expr> {
        let left = self.parse_additive()?;
        self.skip_ws_and_comments();

        let op = if self.check_str(">=") { BinaryOp::Ge }
        else if self.check_str("<=") { BinaryOp::Le }
        else if self.check_str(">") { BinaryOp::Gt }
        else if self.check_str("<") { BinaryOp::Lt }
        else { return Ok(left); };

        self.advance_by(match op { BinaryOp::Ge | BinaryOp::Le => 2, _ => 1 });
        self.skip_ws_and_comments();
        let right = Box::new(self.parse_comparison()?);
        Ok(Expr::Bin { op, left: Box::new(left), right })
    }

    fn parse_additive(&mut self) -> Result<Expr> {
        let left = self.parse_multiplicative()?;
        self.skip_ws_and_comments();

        let op = if self.check('+') && !self.check_str("++") { self.advance(); BinaryOp::Add }
        else if self.check('-') && !self.check_str("--") { self.advance(); BinaryOp::Sub }
        else { return Ok(left); };

        self.skip_ws_and_comments();
        let right = Box::new(self.parse_additive()?);
        Ok(Expr::Bin { op, left: Box::new(left), right })
    }

    fn parse_multiplicative(&mut self) -> Result<Expr> {
        let left = self.parse_unary()?;
        self.skip_ws_and_comments();

        let op = if self.check('*') { self.advance(); BinaryOp::Mul }
        else if self.check('/') { self.advance(); BinaryOp::Div }
        else if self.check('%') { self.advance(); BinaryOp::Mod }
        else { return Ok(left); };

        self.skip_ws_and_comments();
        let right = Box::new(self.parse_multiplicative()?);
        Ok(Expr::Bin { op, left: Box::new(left), right })
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        self.skip_ws_and_comments();

        let op = if self.check_str("typeof") { self.advance_by(6); Some(UnaryOp::TypeOf) }
        else if self.check('!') { self.advance(); Some(UnaryOp::Not) }
        else if self.check('-') { self.advance(); Some(UnaryOp::Minus) }
        else if self.check('+') { self.advance(); Some(UnaryOp::Plus) }
        else if self.check_word("new") {
            self.advance_by(3);
            self.skip_ws_and_comments();
            let mut callee = Box::new(self.parse_primary()?);
            loop {
                self.skip_ws_and_comments();
                if self.check('.') {
                    self.advance();
                    let prop = Box::new(self.parse_primary()?);
                    callee = Box::new(Expr::Member {
                        object: callee,
                        property: prop,
                        computed: false,
                        optional: false,
                    });
                } else {
                    break;
                }
            }
            let args = if self.check('(') {
                self.advance();
                let mut args = Vec::new();
                while !self.check(')') {
                    self.skip_ws_and_comments();
                    if self.check(')') { break; }
                    args.push(self.parse_expression()?);
                    self.skip_ws_and_comments();
                    if self.check(',') { self.advance(); }
                }
                self.expect(')')?;
                args
            } else {
                vec![]
            };
            return Ok(Expr::New { callee, args, type_args: vec![] });
        }
        else { return self.parse_call(); };

        self.skip_ws_and_comments();
        let arg = Box::new(self.parse_unary()?);
        Ok(Expr::Unary { op: op.unwrap(), arg, prefix: true })
    }

    fn parse_call(&mut self) -> Result<Expr> {
        let mut expr = self.parse_primary()?;

        loop {
            self.skip_ws_and_comments();

            // Optional chaining: ?.
            let is_optional = self.check_str("?.");
            
            // Check for type arguments BEFORE arguments (e.g., useState<T>)
            let type_args = self.parse_type_args()?;
            
            if self.check('(') {
                self.advance();
                let mut args = Vec::new();
                while !self.check(')') {
                    self.skip_ws_and_comments();
                    if self.check(')') { break; }
                    args.push(self.parse_expression()?);
                    self.skip_ws_and_comments();
                    if self.check(',') { self.advance(); }
                }
                self.expect(')')?;
                expr = Expr::Call { callee: Box::new(expr), args, type_args };
            } else if !type_args.is_empty() {
                // Type args without parens - wrap as type assertion
                expr = Expr::TSAs { expr: Box::new(expr), type_: Type::Ref { 
                    name: "Type".to_string(), 
                    generics: type_args 
                }};
            } else if self.check('.') || is_optional {
                if is_optional {
                    self.advance_by(2); // Skip ?.
                } else {
                    self.advance();
                }
                let property = Box::new(self.parse_primary()?);
                expr = Expr::Member { object: Box::new(expr), property, computed: false, optional: is_optional };
            } else if self.check('[') || (self.check('?') && self.peek() == Some('[')) {
                let opt_computed = self.check('?');
                if opt_computed {
                    self.advance_by(2); // Skip ?[
                } else {
                    self.advance();
                }
                let property = Box::new(self.parse_expression()?);
                self.expect(']')?;
                expr = Expr::Member { object: Box::new(expr), property, computed: true, optional: opt_computed };
            } else {
                break;
            }
        }

        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        self.skip_ws_and_comments();

        let c = self.current();

        if c == '"' || c == '\'' {
            return Ok(Expr::String(self.parse_string()?));
        }

        if c.is_ascii_digit() || (c == '.' && self.peek().map(|p| p.is_ascii_digit()).unwrap_or(false)) {
            return Ok(Expr::Number(self.parse_number()?));
        }

        if c == '`' {
            return self.parse_template_literal();
        }

        // Handle identifiers (including keywords used as identifiers)
        if self.is_ident_char(c) || c.is_alphabetic() {
            let name = self.parse_identifier()?;
            // Check for optional chaining after identifier: identifier?.prop
            let expr = Expr::Ident { name };
            return Ok(expr);
        }

        if c == '{' {
            // Check if this is an object literal by looking ahead for key: value patterns
            let inner_start = self.pos + 1;
            let inner_end = self.source[inner_start..].find('}').map(|p| inner_start + p);
            
            if let Some(end) = inner_end {
                let inner = &self.source[inner_start..end];
                let trimmed = inner.trim();
                
                // It's an object if:
                // 1. Contains a colon (key: value pattern)
                // 2. Or starts with spread operator (...)
                let is_object = trimmed.starts_with("...") || (trimmed.contains(':') && !trimmed.chars().all(|c| c.is_ascii_whitespace() || c.is_ascii_digit()));
                
                if is_object {
                    return self.parse_object_literal();
                }
            }
            
            // Otherwise, treat as parenthesized expression { expr }
            // Check for arrow function first: { () => ... } or { (x) => ... }
            self.advance();
            
            // Skip whitespace and check if this is an arrow function
            self.skip_ws_and_comments();
            
            // Check if we're at ( or an identifier (potential arrow function params)
            if self.check('(') || self.is_ident_char(self.current()) || self.current().is_alphabetic() {
                // This could be an arrow function parameter list
                // Try to find where the parameter list ends (before =>)
                let start_pos = self.pos;
                
                // Skip to find =>
                let mut found_arrow = false;
                while !self.is_at_end() && self.current() != '}' {
                    if self.current() == '=' && self.peek() == Some('>') {
                        found_arrow = true;
                        break;
                    }
                    self.advance();
                }
                
                
                if found_arrow {
                    // This is an arrow function, reset and parse from start
                    self.pos = start_pos;
                    let expr = self.parse_assignment();
                    if expr.is_err() {
                    }
                    let expr = expr?;
                    self.expect('}')?;
                    return Ok(expr);
                } else {
                    // Not an arrow function, reset and parse as normal expression
                    self.pos = start_pos;
                }
            }
            
            // Parse as normal expression
            let expr = self.parse_ternary()?;
            self.expect('}')?;
            return Ok(expr);
        }

        if c == '[' {
            self.advance();
            let mut elems = Vec::new();
            while !self.check(']') {
                self.skip_ws_and_comments();
                if self.check(']') { break; }
                
                // Handle spread operator: ...expr
                if self.check_str("...") {
                    self.advance_by(3);
                    self.skip_ws_and_comments();
                    let arg = Box::new(self.parse_expression()?);
                    elems.push(Some(Expr::Spread { arg }));
                } else {
                    elems.push(Some(self.parse_expression()?));
                }
                
                self.skip_ws_and_comments();
                if self.check(',') { self.advance(); }
            }
            self.expect(']')?;
            return Ok(Expr::Array { elems });
        }

        if c == '<' {
            return self.parse_jsx();
        }

        if self.is_ident_char(c) || c.is_alphabetic() {
            let name = self.parse_identifier()?;
            return Ok(Expr::Ident { name });
        }

        if self.check_word("true") { return Ok(Expr::Boolean(true)); }
        if self.check_word("false") { return Ok(Expr::Boolean(false)); }
        if self.check_word("null") { return Ok(Expr::Null); }
        if self.check_word("undefined") { return Ok(Expr::Undefined); }

        if c == '(' {
            let start_pos = self.pos;
            self.advance(); // past '('

            // Try to detect arrow function: (params) => body
            let mut is_arrow = false;
            let mut params = Vec::new();

            if self.check(')') {
                // Empty params: () => ...
                self.advance();
                self.skip_ws_and_comments();
                is_arrow = self.check_str("=>");
            } else {
                // Try to parse comma-separated identifiers as params
                let mut param_parse_ok = true;
                loop {
                    self.skip_ws_and_comments();
                    if self.check(')') { break; }

                    if !self.is_ident_char(self.current()) && !self.current().is_alphabetic() {
                        param_parse_ok = false;
                        break;
                    }
                    let name = self.parse_identifier()?;
                    params.push(Param { name, type_: None, default: None, optional: false, pattern: None });

                    self.skip_ws_and_comments();
                    if self.check(',') {
                        self.advance();
                    } else if self.check(')') {
                        break;
                    } else {
                        param_parse_ok = false;
                        break;
                    }
                }

                if param_parse_ok && self.check(')') {
                    self.advance();
                    self.skip_ws_and_comments();
                    is_arrow = self.check_str("=>");
                }
            }

            if is_arrow {
                self.advance_by(2); // past =>
                self.skip_ws_and_comments();
                let body = if self.check('{') {
                    Stmt::Block(self.parse_block()?.0)
                } else {
                    let expr = self.parse_expression()?;
                    Stmt::Return { arg: Some(expr) }
                };
                return Ok(Expr::Arrow { params, body: Box::new(body), is_async: false });
            } else {
                // Not an arrow function, reset and parse as parenthesized expression
                self.pos = start_pos;
                self.advance(); // past '('
                let expr = self.parse_expression()?;
                self.expect(')')?;
                return Ok(expr);
            }
        }

        Ok(Expr::Ident { name: String::new() })
    }

    fn parse_object_literal(&mut self) -> Result<Expr> {
        self.expect('{')?;
        let mut props = Vec::new();

        loop {
            self.skip_ws_and_comments();
            if self.check('}') { break; }

            if self.check_str("...") {
                self.advance_by(3);
                self.skip_ws_and_comments();
                let value = Box::new(self.parse_expression()?);
                props.push(ObjectProp::Spread { value: *value });
            } else {
                // Check for async method shorthand: async foo() { }
                let is_async = self.check_word("async");
                if is_async {
                    self.advance_by(5);
                    self.skip_ws_and_comments();
                }

                let key = self.parse_prop_key()?;
                self.skip_ws_and_comments();

                // Check for method shorthand: foo() { } or foo(): T { }
                if self.check('(') {
                    self.advance(); // past '('
                    let params = self.parse_params()?;
                    self.expect(')')?;
                    self.skip_ws_and_comments();
                    let return_type = if self.check(':') {
                        self.advance();
                        self.skip_ws_and_comments();
                        Some(self.parse_type()?)
                    } else {
                        None
                    };
                    self.skip_ws_and_comments();
                    let body = if self.check('{') {
                        Some(self.parse_block()?)
                    } else {
                        None
                    };
                    let decl = FunctionDecl {
                        name: key.clone(),
                        generics: vec![],
                        params,
                        return_type,
                        body,
                        is_async,
                        is_generator: false,
                        decorators: vec![],
                    };
                    props.push(ObjectProp::Method { key: PropKey::Ident(key), value: decl });
                } else if self.check(':') {
                    // Regular property: key: value
                    self.advance();
                    self.skip_ws_and_comments();
                    let value = self.parse_expression()?;
                    props.push(ObjectProp::Init { key: PropKey::Ident(key.to_string()), value });
                } else {
                    // Shorthand property: { foo } means { foo: foo }
                    let value = Expr::Ident { name: key.to_string() };
                    props.push(ObjectProp::Init { key: PropKey::Ident(key.to_string()), value });
                }
            }

            self.skip_ws_and_comments();
            if self.check(',') { 
                self.advance(); // consume comma
            } else if !self.check('}') { 
                self.advance(); // consume something else (shouldn't happen often)
            }  // else: we're at '}', don't advance
        }

        self.expect('}')?;
        Ok(Expr::Object { props })
    }

    fn parse_prop_key(&mut self) -> Result<String> {
        let c = self.current();
        if c == '"' || c == '\'' {
            Ok(self.parse_string()?)
        } else if c.is_ascii_digit() {
            Ok(self.parse_number()?.to_string())
        } else {
            self.parse_identifier()
        }
    }

    fn parse_jsx(&mut self) -> Result<Expr> {
        self.expect('<')?;
        self.skip_ws_and_comments();

        let name = self.parse_jsx_name()?;
        self.skip_ws_and_comments();

        let mut attrs = Vec::new();
        while !self.check('>') && !self.check_str("/>") {
            self.skip_ws_and_comments();
            if self.check('>') || self.check_str("/>") { break; }

            if self.check('{') {
                self.advance();
                let expr = self.parse_expression()?;
                self.expect('}')?;
                attrs.push(JSXAttr::Expr { name: None, expr });
            } else {
                let attr_name = self.parse_identifier()?;
                self.skip_ws_and_comments();

                let value = if self.check('=') {
                    self.advance();
                    self.skip_ws_and_comments();
                    if self.current() == '"' || self.current() == '\'' {
                        JSXAttrValue::String(self.parse_string()?)
                    } else if self.check('{') {
                        // JSX expression wrapper: attr={expr}
                        self.advance();
                        let expr_val = self.parse_expression()?;
                        self.expect('}')?;
                        JSXAttrValue::Expr(expr_val)
                    } else {
                        // Use parse_primary to avoid parse_assignment's /> skipping
                        let expr_val = self.parse_primary();
                        JSXAttrValue::Expr(expr_val?)
                    }
                } else {
                    JSXAttrValue::String("true".to_string())
                };

                attrs.push(JSXAttr::Attr { name: attr_name, value: Some(value) });
            }
            self.skip_ws_and_comments();
        }

        let self_closing = self.check_str("/>");
        if self_closing { 
            self.advance_by(2); 
        } else { self.expect('>')?; }

        let mut children = Vec::new();
        if !self_closing {
            loop {
                self.skip_ws_and_comments();
                if self.check('<') {
                    if self.peek().map(|c| c == '/').unwrap_or(false) { break; }
                    let jsx_expr = self.parse_jsx()?;
                    let jsx_expr = match jsx_expr {
                        Expr::JSX(jsx) => jsx,
                        _ => return Ok(Expr::JSX(JSXExpr { opening: JSXOpening { name: JSXName::Ident("div".to_string()), attrs: vec![], self_closing: false }, children: vec![], closing: None })),
                    };
                    children.push(JSXChild::JSX(jsx_expr));
                } else if self.current() == '{' {
                    self.advance();
                    self.skip_ws_and_comments();
                    let expr = self.parse_expression()?;
                    self.expect('}')?;
                    children.push(JSXChild::Expr(expr));
                } else if !self.is_at_end() && self.current() != '<' {
                    let text = self.parse_jsx_text();
                    if !text.trim().is_empty() {
                        children.push(JSXChild::Text(text));
                    }
                } else {
                    break;
                }
            }

            self.expect('<')?;
            self.expect('/')?;
            self.skip_ws_and_comments();
            let _close_name = self.parse_jsx_name()?;
            self.skip_ws_and_comments();
            self.expect('>')?;
        }

        let result = Expr::JSX(JSXExpr {
            opening: JSXOpening { name, attrs, self_closing },
            children,
            closing: None,
        });
        Ok(result)
    }

    fn parse_jsx_name(&mut self) -> Result<JSXName> {
        let name = self.parse_identifier()?;
        if self.check('.') {
            self.advance();
            let property = self.parse_identifier()?;
            return Ok(JSXName::Member { object: name, property });
        }
        Ok(JSXName::Ident(name))
    }

    fn parse_jsx_text(&mut self) -> String {
        let mut text = String::new();
        while !self.is_at_end() {
            let c = self.current();
            if c == '<' || c == '{' || c == '>' { break; }
            text.push(c);
            self.advance();
        }
        text.trim().to_string()
    }

    fn parse_string(&mut self) -> Result<String> {
        let quote = self.current();
        self.advance();
        let mut value = String::new();

        while !self.is_at_end() && self.current() != quote {
            let c = self.current();
            if c == '\\' {
                self.advance();
                match self.current() {
                    'n' => value.push('\n'),
                    't' => value.push('\t'),
                    'r' => value.push('\r'),
                    '\\' => value.push('\\'),
                    '\'' => value.push('\''),
                    '"' => value.push('"'),
                    '0' => value.push('\0'),
                    _ => value.push(self.current()),
                }
            } else {
                value.push(c);
            }
            self.advance();
        }

        self.expect(quote)?;
        Ok(value)
    }

    fn parse_number(&mut self) -> Result<f64> {
        let start = self.pos;
        while self.current().is_ascii_digit() { self.advance(); }
        if self.check('.') {
            self.advance();
            while self.current().is_ascii_digit() { self.advance(); }
        }
        let num_str = &self.source[start..self.pos];
        num_str.parse().map_err(|_| anyhow::anyhow!("Invalid number: {}", num_str))
    }

    /// Parse a template literal with ${} expressions
    fn parse_template_literal(&mut self) -> Result<Expr> {
        self.expect('`')?;
        
        let mut parts = Vec::new();
        let mut exprs = Vec::new();
        let mut current_string = String::new();
        
        while !self.is_at_end() && self.current() != '`' {
            let c = self.current();
            
            if c == '$' && self.peek() == Some('{') {
                // Save current string part
                if !current_string.is_empty() {
                    parts.push(TemplatePart::String(std::mem::take(&mut current_string)));
                }
                
                // Parse expression in ${}
                self.advance_by(2); // Skip ${'
                self.skip_ws_and_comments();
                
                let expr = self.parse_expression()?;
                exprs.push(expr);
                
                self.expect('}')?;
            } else if c == '\\' {
                // Escape sequence
                self.advance();
                let escaped = match self.current() {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '\\' => '\\',
                    '`' => '`',
                    '$' => '$',
                    '0' => '\0',
                    _ => self.current(),
                };
                current_string.push(escaped);
                self.advance();
            } else {
                current_string.push(c);
                self.advance();
            }
        }
        
        self.expect('`')?;
        
        // Save final string part
        if !current_string.is_empty() {
            parts.push(TemplatePart::String(current_string));
        }
        
        Ok(Expr::Template { parts, exprs })
    }

    fn parse_identifier(&mut self) -> Result<String> {
        if !self.is_ident_char(self.current()) && !self.current().is_alphabetic() {
            return Err(anyhow::anyhow!("Expected identifier at position {}", self.pos));
        }

        let start = self.pos;
        while self.is_ident_char(self.current()) {
            self.advance();
        }

        Ok(self.source[start..self.pos].to_string())
    }

    fn current(&self) -> char {
        self.source[self.pos..].chars().next().unwrap_or('\0')
    }

    fn peek(&self) -> Option<char> {
        self.source[self.pos..].chars().nth(1)
    }

    fn advance(&mut self) {
        let c = self.current();
        if c != '\0' {
            self.pos += c.len_utf8();
        }
    }

    fn advance_by(&mut self, n: usize) { self.pos += n; }

    fn check(&mut self, expected: char) -> bool {
        self.current() == expected
    }

    fn check_str(&self, expected: &str) -> bool {
        self.source[self.pos..].starts_with(expected)
    }

    fn skip_word(&mut self, word: &str) -> bool {
        if self.check_word(word) {
            self.advance_by(word.len());
            true
        } else {
            false
        }
    }

    fn check_word(&self, word: &str) -> bool {
        self.source[self.pos..].starts_with(word) &&
            !self.source.chars().nth(self.pos + word.len()).map(|c| self.is_ident_char(c)).unwrap_or(true)
    }

    fn expect(&mut self, expected: char) -> Result<()> {
        if self.current() != expected {
            return Err(anyhow::anyhow!(
                "Expected '{}' at position {}, found '{}'",
                expected,
                self.pos,
                self.current()
            ));
        }
        self.advance();
        Ok(())
    }

    fn expect_str(&mut self, expected: &str) -> Result<()> {
        if !self.source[self.pos..].starts_with(expected) {
            return Err(anyhow::anyhow!("Expected '{}' at position {}", expected, self.pos));
        }
        self.advance_by(expected.len());
        Ok(())
    }

    fn is_at_end(&self) -> bool {
        self.pos >= self.source.len() || self.current() == '\0'
    }

    fn is_ident_char(&self, c: char) -> bool {
        c.is_ascii_alphanumeric() || c == '_' || c == '$'
    }

    fn is_var_keyword(&self) -> bool {
        self.check_word("const") || self.check_word("let") || self.check_word("var")
    }

    fn skip_ws_and_comments(&mut self) {
        while !self.is_at_end() {
            let c = self.current();
            if c.is_whitespace() {
                self.advance();
            } else if c == '/' && self.peek() == Some('/') {
                self.advance_by(2);
                while !self.is_at_end() && self.current() != '\n' { self.advance(); }
            } else if c == '/' && self.peek() == Some('*') {
                self.advance_by(2);
                while !self.is_at_end() {
                    if self.current() == '*' && self.peek() == Some('/') {
                        self.advance_by(2);
                        break;
                    }
                    self.advance();
                }
            } else {
                break;
            }
        }
    }

    fn skip_statement(&mut self) {
        let mut depth = 0;
        while !self.is_at_end() {
            let c = self.current();
            if c == '{' || c == '(' || c == '[' { depth += 1; }
            else if c == '}' || c == ')' || c == ']' {
                if depth == 0 { break; }
                depth -= 1;
            } else if c == ';' && depth == 0 { self.advance(); break; }
            self.advance();
        }
    }

    fn skip_to_semicolon(&mut self) {
        while !self.is_at_end() && self.current() != ';' { self.advance(); }
        if self.check(';') { self.advance(); }
    }

    fn skip_balanced(&mut self, open: char, close: char) {
        let mut depth = 1;
        self.advance();
        while !self.is_at_end() && depth > 0 {
            let c = self.current();
            if c == open { depth += 1; }
            else if c == close { depth -= 1; }
            self.advance();
        }
    }
}
