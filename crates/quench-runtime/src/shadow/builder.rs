//! Shadow tree builder and TypeScript type inference.
//!
//! Helper to build shadow trees from source and TypeScript type map construction.

use std::collections::HashMap;

use bumpalo::Bump;

use crate::interner::Symbol;
use crate::nanbox::JSValue;
use crate::shape::ShapeInterner;
use swc_ecma_ast::{Decl, Pat, Stmt, TsEntityName, TsKeywordTypeKind, TsLit, TsType, TsTypeRef};

use super::types::{Binding, ExecType, ObjectLayout, TypeMap};

/// Helper to build shadow trees from source inside `Context::eval_shadow`.
pub struct ShadowBuilder<'a> {
    pub bump: &'a Bump,
    pub interner: &'a mut crate::interner::StringInterner,
    pub shapes: &'a ShapeInterner,
    pub locals: HashMap<String, u16>,
    pub next_local: u16,
    pub bindings: HashMap<String, Binding>,
    pub object_layouts: HashMap<String, ObjectLayout>,
    pub type_map: TypeMap,
}

impl<'a> ShadowBuilder<'a> {
    pub fn new(
        bump: &'a Bump,
        interner: &'a mut crate::interner::StringInterner,
        shapes: &'a ShapeInterner,
    ) -> Self {
        ShadowBuilder {
            bump,
            interner,
            shapes,
            locals: HashMap::new(),
            next_local: 0,
            bindings: HashMap::new(),
            object_layouts: HashMap::new(),
            type_map: TypeMap::new(),
        }
    }

    pub fn collect_type_map(&mut self, script: &swc_ecma_ast::Script) {
        self.type_map = build_type_map(script, self.shapes, self.interner);
    }

    pub fn alloc<T>(&self, value: T) -> &'a T {
        self.bump.alloc(value)
    }

    pub fn local_for(&mut self, name: &str) -> u16 {
        if let Some(&idx) = self.locals.get(name) {
            return idx;
        }
        let idx = self.next_local;
        self.next_local += 1;
        self.locals.insert(name.to_string(), idx);
        idx
    }

    pub fn intern(&mut self, s: &str) -> Symbol {
        self.interner.intern(s)
    }
}

/// Build a `TypeMap` by scanning `var`/`let`/`const` declarations with TypeScript
/// type annotations in the script body.
pub fn build_type_map(
    script: &swc_ecma_ast::Script,
    shapes: &ShapeInterner,
    interner: &mut crate::interner::StringInterner,
) -> TypeMap {
    let mut class_shapes: HashMap<String, crate::shape::ShapeId> = HashMap::new();

    // First pass: collect class declarations so type references can resolve them.
    for stmt in &script.body {
        if let Stmt::Decl(Decl::Class(class)) = stmt {
            let shape = shapes.root();
            class_shapes.insert(class.ident.sym.to_string(), shape.id);
        }
    }

    let mut map = TypeMap::new();
    for stmt in &script.body {
        let Stmt::Decl(Decl::Var(var_decl)) = stmt else { continue };
        for decl in &var_decl.decls {
            let Pat::Ident(ident) = &decl.name else { continue };
            let Some(type_ann) = &ident.type_ann else { continue };
            let ty =
                ts_type_to_exec_type(&type_ann.type_ann, shapes, interner, &class_shapes);
            let sym = interner.intern(ident.id.sym.as_ref());
            map.insert(sym, ty);
        }
    }
    map
}

fn ts_type_to_exec_type(
    ts: &TsType,
    _shapes: &ShapeInterner,
    interner: &mut crate::interner::StringInterner,
    class_shapes: &HashMap<String, crate::shape::ShapeId>,
) -> ExecType {
    match ts {
        TsType::TsKeywordType(kw) => match kw.kind {
            TsKeywordTypeKind::TsNumberKeyword => ExecType::Int32,
            TsKeywordTypeKind::TsStringKeyword => ExecType::String,
            TsKeywordTypeKind::TsBooleanKeyword => ExecType::Boolean,
            TsKeywordTypeKind::TsVoidKeyword => ExecType::Void,
            TsKeywordTypeKind::TsBigIntKeyword => ExecType::BigInt,
            TsKeywordTypeKind::TsSymbolKeyword => ExecType::Symbol,
            _ => ExecType::Unknown,
        },
        TsType::TsLitType(lit) => ts_lit_to_exec_type(&lit.lit, interner),
        TsType::TsTypeRef(TsTypeRef { type_name, .. }) => {
            if let TsEntityName::Ident(ident) = type_name {
                if let Some(&shape_id) = class_shapes.get(&ident.sym.to_string()) {
                    return ExecType::Object(shape_id);
                }
            }
            ExecType::Unknown
        }
        _ => ExecType::Unknown,
    }
}

fn ts_lit_to_exec_type(
    lit: &TsLit,
    interner: &mut crate::interner::StringInterner,
) -> ExecType {
    match lit {
        TsLit::Number(n) => {
            let v = n.value;
            if v.fract() == 0.0 && v >= i32::MIN as f64 && v <= i32::MAX as f64 {
                ExecType::Literal(JSValue::int32(v as i32))
            } else {
                ExecType::Literal(JSValue::double(v))
            }
        }
        TsLit::Str(s) => {
            let sym = interner.intern(s.value.to_string_lossy());
            ExecType::Literal(JSValue::string(sym))
        }
        TsLit::Bool(b) => ExecType::Literal(JSValue::bool(b.value)),
        _ => ExecType::Unknown,
    }
}
