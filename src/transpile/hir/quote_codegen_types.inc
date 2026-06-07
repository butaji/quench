impl QuoteCodegen {
    fn gen_type_helper(&self, ty: &Type) -> TokenStream {
        use super::Type as T;
        match ty {
            T::Array { elem } => self.gen_array_type(elem),
            T::Ref { name: _, generics: _ } => self.gen_type_helper(&Type::Array { elem: Box::new(Type::Unknown) }),
            T::Object { .. } | T::Union { .. } | T::Intersection { .. } | T::Literal { .. } | T::Template { .. } => self.gen_obj_union_literal_template(ty),
            T::Function { .. } | T::Record { .. } | T::KeyOf { .. } => self.gen_fn_record_keyof(ty),
            T::Index { obj, index } => self.gen_index_type(obj, index),
            T::Mapped { from, to } => self.gen_mapped_type(from, to),
            T::Conditional { check, extends, true_type, false_type } => {
                self.gen_conditional_type(check, extends, true_type, false_type)
            }
            T::Partial { .. } | T::Required { .. } | T::Pick { .. } | T::Omit { .. } | T::ReturnType { .. } | T::Parameters { .. } => self.gen_type_with_inner(ty),
            _ => quote! { Value },
        }
    }

    fn gen_type_with_inner(&self, ty: &Type) -> TokenStream {
        use super::Type as T;
        match ty {
            T::Partial { inner } | T::Required { inner } => self.gen_partial_type(inner),
            T::Pick { inner, keys } | T::Omit { inner, keys } => self.gen_pick_type(inner, keys),
            T::ReturnType { inner } | T::Parameters { inner } => self.gen_return_type(inner),
            _ => quote! { Value },
        }
    }

    fn gen_obj_union_literal_template(&self, ty: &Type) -> TokenStream {
        use super::Type as T;
        match ty {
            T::Object { members } => self.gen_object_type(members),
            T::Union { types } | T::Intersection { types } => self.gen_union_type(types),
            T::Literal { kind, value } => self.gen_literal_type(kind, value),
            T::Template { parts, values } => self.gen_template_type(parts, values),
            _ => quote! { Value },
        }
    }

    fn gen_fn_record_keyof(&self, ty: &Type) -> TokenStream {
        use super::Type as T;
        match ty {
            T::Function { params, ret } => self.gen_fn_type(params, ret),
            T::Record { key, value } => self.gen_record_type(key, value),
            T::KeyOf { inner } => self.gen_keyof_type(inner),
            _ => quote! { Value },
        }
    }

    fn gen_tuple_type(&self, elements: &[super::TupleElement]) -> TokenStream {
        let types: Vec<_> = elements.iter().map(|e| self.gen_type(&e.type_)).collect();
        quote! { (#(#types),*) }
    }

    fn gen_index_type(&self, obj: &Box<Type>, index: &Box<Type>) -> TokenStream {
        let obj_t = self.gen_type(obj);
        let index_t = self.gen_type(index);
        quote! { std::collections::HashMap<#obj_t, #index_t> }
    }

    fn gen_mapped_type(&self, from: &Box<Type>, to: &Box<Type>) -> TokenStream {
        let from_t = self.gen_type(from);
        let to_t = self.gen_type(to);
        quote! { std::collections::HashMap<#from_t, #to_t> }
    }

    fn gen_conditional_type(&self, check: &Box<Type>, extends: &Box<Type>, true_type: &Box<Type>, false_type: &Box<Type>) -> TokenStream {
        let check_t = self.gen_type(check);
        let extends_t = self.gen_type(extends);
        let true_t = self.gen_type(true_type);
        let false_t = self.gen_type(false_type);
        quote! { if #check_t: #extends_t { #true_t } else { #false_t } }
    }

    fn gen_record_type(&self, key: &Box<Type>, value: &Box<Type>) -> TokenStream {
        let key_t = self.gen_type(key);
        let value_t = self.gen_type(value);
        quote! { std::collections::HashMap<#key_t, #value_t> }
    }

    fn gen_partial_type(&self, inner: &Box<Type>) -> TokenStream {
        use super::Type as T;
        match inner.as_ref() {
            T::Object { members } => {
                let fields: Vec<_> = members.iter()
                    .map(|m| {
                        let name = syn::Ident::new(&m.key, proc_macro2::Span::call_site());
                        let ty = self.gen_type(&m.type_);
                        quote! { pub #name: Option<#ty> }
                    })
                    .collect();
                quote! { { #(#fields);* } }
            }
            _ => quote! { Value },
        }
    }

    fn gen_pick_type(&self, inner: &Box<Type>, keys: &[String]) -> TokenStream {
        use super::Type as T;
        match inner.as_ref() {
            T::Object { members } => {
                let fields: Vec<_> = members.iter()
                    .filter(|m| keys.contains(&m.key))
                    .map(|m| {
                        let name = syn::Ident::new(&m.key, proc_macro2::Span::call_site());
                        let ty = self.gen_type(&m.type_);
                        quote! { pub #name: #ty }
                    })
                    .collect();
                if fields.is_empty() {
                    quote! { Value }
                } else {
                    quote! { { #(#fields);* } }
                }
            }
            _ => quote! { Value },
        }
    }

    fn gen_keyof_type(&self, inner: &Box<Type>) -> TokenStream {
        use super::Type as T;
        match inner.as_ref() {
            T::Object { members } => {
                let variants: Vec<_> = members.iter()
                    .map(|m| {
                        let variant = syn::Ident::new(&m.key, proc_macro2::Span::call_site());
                        quote! { #variant }
                    })
                    .collect();
                quote! { enum { #(#variants),* } }
            }
            _ => quote! { Value },
        }
    }

    fn gen_return_type(&self, inner: &Box<Type>) -> TokenStream {
        use super::Type as T;
        match inner.as_ref() {
            T::Function { params: _, ret } => self.gen_type(ret),
            _ => quote! { Value },
        }
    }

    fn gen_union_type(&self, types: &[Type]) -> TokenStream {
        if types.is_empty() {
            return quote! { Value };
        }
        let variants: Vec<TokenStream> = types.iter()
            .enumerate()
            .map(|(i, t)| self.gen_union_variant(i, t))
            .collect();
        quote! { enum { #(#variants),* } }
    }

    fn gen_union_variant(&self, index: usize, ty: &Type) -> TokenStream {
        use super::Type as T;
        match ty {
            T::Ref { name, generics } => {
                let name_id = syn::Ident::new(name, proc_macro2::Span::call_site());
                if generics.is_empty() {
                    quote! { #name_id }
                } else {
                    let gs: Vec<_> = generics.iter().map(|g| self.gen_type(g)).collect();
                    quote! { #name_id<#(#gs),*> }
                }
            }
            T::Object { members } => {
                let fields: Vec<_> = members.iter()
                    .map(|m| {
                        let key = syn::Ident::new(&m.key, proc_macro2::Span::call_site());
                        let ty = self.gen_type(&m.type_);
                        quote! { #key: #ty }
                    })
                    .collect();
                let variant_ident = syn::Ident::new(&format!("Variant{}", index), proc_macro2::Span::call_site());
                quote! { #variant_ident { #(#fields),* } }
            }
            T::Literal { kind, value } => {
                let variant_name = syn::Ident::new(&format!("{:?}{}", kind, value), proc_macro2::Span::call_site());
                quote! { #variant_name }
            }
            _ => {
                let variant_ident = syn::Ident::new(&format!("Variant{}", index), proc_macro2::Span::call_site());
                quote! { #variant_ident }
            }
        }
    }

    fn gen_literal_type(&self, kind: &super::LiteralKind, value: &str) -> TokenStream {
        match kind {
            super::LiteralKind::String => {
                let s = value.to_string();
                quote! { #s }
            }
            super::LiteralKind::Number => {
                if let Ok(n) = value.parse::<f64>() {
                    quote! { #n }
                } else {
                    quote! { Value }
                }
            }
            super::LiteralKind::Boolean => {
                if value == "true" {
                    quote! { true }
                } else {
                    quote! { false }
                }
            }
            super::LiteralKind::BigInt => {
                if let Ok(n) = value.parse::<i64>() {
                    quote! { #n }
                } else {
                    quote! { Value }
                }
            }
        }
    }

    fn gen_template_type(&self, parts: &[super::TemplatePart], values: &[Type]) -> TokenStream {
        let mut tokens = TokenStream::new();
        for (i, part) in parts.iter().enumerate() {
            match part {
                super::TemplatePart::String { value: s } => {
                    let s = s.to_string();
                    tokens.extend(quote! { #s });
                }
                super::TemplatePart::Type { value: t } => {
                    tokens.extend(self.gen_type(t));
                }
            }
            if i < values.len() {
                tokens.extend(self.gen_type(&values[i]));
            }
        }
        if tokens.is_empty() {
            quote! { String::new() }
        } else {
            tokens
        }
    }

    fn gen_fn_type(&self, params: &[Type], ret: &Box<Type>) -> TokenStream {
        let ps: Vec<_> = params.iter().map(|p| self.gen_type(p)).collect();
        let r = self.gen_type(ret);
        quote! { fn(#(#ps),*) -> #r }
    }
    
    fn gen_prim_type(&self, ty: &Type) -> TokenStream {
        use super::Type as T;
        match ty {
            T::String => quote! { String },
            T::Number => quote! { f64 },
            T::Boolean => quote! { bool },
            _ => quote! { Value },
        }
    }
    
    fn gen_meta_type(&self, ty: &Type) -> TokenStream {
        use super::Type as T;
        match ty {
            T::Void | T::Never => quote! { () },
            T::Undefined | T::Null | T::Unknown | T::Any => quote! { Value },
            T::BigInt => quote! { i64 },
            _ => quote! { Value },
        }
    }
    
    fn gen_array_type(&self, elem: &Box<Type>) -> TokenStream {
        let inner = self.gen_type(elem);
        quote! { Vec<#inner> }
    }
    
    fn gen_object_type(&self, members: &[super::TypeMember]) -> TokenStream {
        if members.is_empty() {
            return quote! { serde_json::Value };
        }
        let fields: Vec<_> = members.iter()
            .map(|m| {
                let name = syn::Ident::new(&m.key, proc_macro2::Span::call_site());
                let ty = self.gen_type(&m.type_);
                quote! { pub #name: #ty }
            })
            .collect();
        quote! { { #(#fields);* } }
    }
    
}
