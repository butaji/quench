//! VM module load: validated Wasm bytes → store HIR.
//!
//! Format (parse/validate/wast) lives in `quench-wasm`. This is the VM
//! ingesting a binary into Native register HIR.

mod emit;
mod sections;

use crate::hir::{FuncSig, HirFunc, HirModule, Kind, Ty};
use wasmparser::{FunctionBody, ValType, WasmFeatures};

#[derive(Debug)]
pub enum LowerError {
    Parse(String),
    Unsupported,
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(message) => write!(f, "{message}"),
            Self::Unsupported => write!(f, "unimplemented"),
        }
    }
}

/// Load a validated module into the VM. Unsupported ops become `None` funcs.
pub fn load(bytes: &[u8], features: WasmFeatures) -> Result<HirModule, LowerError> {
    let raw = sections::read(bytes, features)?;
    let types: Vec<FuncSig> = raw
        .types
        .iter()
        .enumerate()
        .map(|(i, ty)| {
            let (rec_len, rec_index) = raw.rec_meta.get(i).copied().unwrap_or((1, 0));
            let (has_super, is_final, sub_depth) = match raw.gc_types.get(i) {
                Some(ty) => {
                    let (super_idx, is_final) = match ty {
                        crate::hir::GcType::Func {
                            super_idx,
                            is_final,
                            ..
                        }
                        | crate::hir::GcType::Struct {
                            super_idx,
                            is_final,
                            ..
                        }
                        | crate::hir::GcType::Array {
                            super_idx,
                            is_final,
                            ..
                        } => (*super_idx, *is_final),
                    };
                    (
                        super_idx.is_some(),
                        is_final,
                        super_depth(&raw.gc_types, super_idx),
                    )
                }
                None => (false, true, 0),
            };
            Ok(FuncSig {
                params: map_kinds(ty.params())?,
                results: map_kinds(ty.results())?,
                rec_len,
                rec_index,
                has_super,
                is_final,
                sub_depth,
                chain: type_chain(&raw.gc_types, &raw.rec_meta, i as u32),
            })
        })
        .collect::<Result<_, _>>()?;
    let tag_arities: Vec<usize> = {
        let mut arities = Vec::new();
        for im in raw.imports.iter() {
            if let crate::hir::ImportKind::Tag { type_idx } = im.kind {
                arities.push(
                    types
                        .get(type_idx as usize)
                        .map(|t| t.params.len())
                        .unwrap_or(0),
                );
            }
        }
        for idx in raw.tags.iter() {
            arities.push(
                types
                    .get(*idx as usize)
                    .map(|t| t.params.len())
                    .unwrap_or(0),
            );
        }
        arities
    };
    let import_funcs = raw
        .imports
        .iter()
        .filter(|im| matches!(im.kind, crate::hir::ImportKind::Func { .. }))
        .count();
    let mut funcs = Vec::with_capacity(raw.bodies.len());
    for (index, body) in raw.bodies.iter().enumerate() {
        let ty_index = raw.func_types.get(import_funcs + index).copied();
        funcs.push(
            lower_func(
                &types,
                ty_index,
                &raw.func_types,
                &raw.gc_types,
                &tag_arities,
                body,
            )
            .ok(),
        );
    }
    Ok(HirModule {
        types: types.into_boxed_slice(),
        funcs: funcs.into_boxed_slice(),
        func_types: raw.func_types.into_boxed_slice(),
        imports: raw.imports.into_boxed_slice(),
        exports: raw.exports.into_boxed_slice(),
        memories: raw.memories.into_boxed_slice(),
        tables: raw.tables.into_boxed_slice(),
        globals: raw.globals.into_boxed_slice(),
        datas: raw.datas.into_boxed_slice(),
        elems: raw.elems.into_boxed_slice(),
        tags: raw.tags.into_boxed_slice(),
        gc_types: raw.gc_types.into_boxed_slice(),
        start: raw.start,
    })
}

fn lower_func(
    types: &[FuncSig],
    ty_index: Option<u32>,
    func_types: &[u32],
    gc_types: &[crate::hir::GcType],
    tag_arities: &[usize],
    body: &FunctionBody<'_>,
) -> Result<HirFunc, LowerError> {
    let ty_index = ty_index.ok_or(LowerError::Unsupported)?;
    let ty = types
        .get(ty_index as usize)
        .ok_or(LowerError::Unsupported)?;
    let params: Box<[Ty]> = ty.params.iter().copied().map(Ty::native).collect();
    let results: Box<[Ty]> = ty.results.iter().copied().map(Ty::native).collect();
    let extra = extra_locals(body)?;
    let local_count = params.len() + extra.len();
    let mut ctx = emit::Context::new(local_count as u16, types, func_types, gc_types, tag_arities);
    emit::body(&mut ctx, body, &results)?;
    Ok(HirFunc {
        params,
        results,
        locals: extra,
        nregs: ctx.nregs(),
        code: ctx.finish(),
    })
}

fn extra_locals(body: &FunctionBody<'_>) -> Result<Box<[Ty]>, LowerError> {
    let mut locals = Vec::new();
    let reader = body
        .get_locals_reader()
        .map_err(|e| LowerError::Parse(e.to_string()))?;
    for local in reader {
        let (count, ty) = local.map_err(|e| LowerError::Parse(e.to_string()))?;
        let ty = native_ty(ty)?;
        for _ in 0..count {
            locals.push(ty);
        }
    }
    Ok(locals.into_boxed_slice())
}

fn type_chain(
    types: &[crate::hir::GcType],
    rec_meta: &[(u32, u32)],
    mut idx: u32,
) -> Box<[(u64, u32)]> {
    let mut out = Vec::new();
    for _ in 0..64 {
        let Some(&(len, rec_index)) = rec_meta.get(idx as usize) else {
            break;
        };
        let start = idx.saturating_sub(rec_index);
        out.push((rec_fp(types, rec_meta, start, len), rec_index));
        let super_idx = match types.get(idx as usize) {
            Some(crate::hir::GcType::Func { super_idx, .. })
            | Some(crate::hir::GcType::Struct { super_idx, .. })
            | Some(crate::hir::GcType::Array { super_idx, .. }) => *super_idx,
            None => break,
        };
        match super_idx {
            Some(s) => idx = s,
            None => break,
        }
    }
    out.into_boxed_slice()
}

fn rec_fp(types: &[crate::hir::GcType], rec_meta: &[(u32, u32)], start: u32, len: u32) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for i in 0..len {
        member_fp(types, rec_meta, start, start + i, len).hash(&mut hasher);
    }
    hasher.finish()
}

fn member_fp(
    types: &[crate::hir::GcType],
    rec_meta: &[(u32, u32)],
    start: u32,
    idx: u32,
    len: u32,
) -> String {
    match types.get(idx as usize) {
        Some(crate::hir::GcType::Func {
            super_idx,
            is_final,
            ..
        }) => format!(
            "f{is_final}{}",
            rel(*super_idx, start, len, types, rec_meta)
        ),
        Some(crate::hir::GcType::Struct {
            fields,
            super_idx,
            is_final,
            ..
        }) => format!(
            "s{is_final}{}{}",
            rel(*super_idx, start, len, types, rec_meta),
            fields
                .iter()
                .map(|f| storage_fp(f, start, len, types, rec_meta))
                .collect::<String>()
        ),
        Some(crate::hir::GcType::Array {
            elem,
            super_idx,
            is_final,
            ..
        }) => format!(
            "a{is_final}{}{}",
            rel(*super_idx, start, len, types, rec_meta),
            storage_fp(elem, start, len, types, rec_meta)
        ),
        None => "?".into(),
    }
}

fn storage_fp(
    s: &crate::hir::GcStorage,
    start: u32,
    len: u32,
    types: &[crate::hir::GcType],
    rec_meta: &[(u32, u32)],
) -> String {
    match s {
        crate::hir::GcStorage::I8 => "i8".into(),
        crate::hir::GcStorage::I16 => "i16".into(),
        crate::hir::GcStorage::Val(k) => format!("{k:?}"),
        crate::hir::GcStorage::Ref { type_idx } => {
            format!("r{}", rel(*type_idx, start, len, types, rec_meta))
        }
    }
}

fn rel(
    idx: Option<u32>,
    start: u32,
    len: u32,
    types: &[crate::hir::GcType],
    rec_meta: &[(u32, u32)],
) -> String {
    let Some(idx) = idx else {
        return "n".into();
    };
    if idx >= start && idx < start + len {
        return format!("l{}", idx - start);
    }
    let Some(&(tlen, tindex)) = rec_meta.get(idx as usize) else {
        return format!("g{idx}");
    };
    let tstart = idx.saturating_sub(tindex);
    if tstart == start && tlen == len {
        return format!("l{tindex}");
    }
    format!("x{}:{tindex}", rec_fp(types, rec_meta, tstart, tlen))
}

fn super_depth(types: &[crate::hir::GcType], start: Option<u32>) -> u32 {
    let mut n = 0u32;
    let mut cur = start;
    while let Some(idx) = cur {
        n += 1;
        cur = match types.get(idx as usize) {
            Some(crate::hir::GcType::Func { super_idx, .. })
            | Some(crate::hir::GcType::Struct { super_idx, .. })
            | Some(crate::hir::GcType::Array { super_idx, .. }) => *super_idx,
            None => break,
        };
        if n > 64 {
            break;
        }
    }
    n
}

fn map_kinds(types: &[ValType]) -> Result<Box<[Kind]>, LowerError> {
    types.iter().copied().map(kind).collect()
}

pub(crate) fn native_ty(ty: ValType) -> Result<Ty, LowerError> {
    Ok(Ty::native(kind(ty)?))
}

pub(crate) fn kind(ty: ValType) -> Result<Kind, LowerError> {
    match ty {
        ValType::I32 => Ok(Kind::I32),
        ValType::I64 => Ok(Kind::I64),
        ValType::F32 => Ok(Kind::F32),
        ValType::F64 => Ok(Kind::F64),
        ValType::V128 => Ok(Kind::V128),
        ValType::Ref(_) => Ok(Kind::Ref),
    }
}

pub(crate) fn parse_err(error: impl ToString) -> LowerError {
    LowerError::Parse(error.to_string())
}
