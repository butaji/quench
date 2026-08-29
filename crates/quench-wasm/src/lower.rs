//! Wasm bytes → common HIR. Decode is third-party; execute is not.

mod emit;
mod sections;

use quench_runtime::hir::{FuncSig, HirFunc, HirModule, Kind, Ty};
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

/// Translate a validated module into HIR. Unsupported ops become `None` funcs.
pub fn lower_binary(bytes: &[u8], features: WasmFeatures) -> Result<HirModule, LowerError> {
    let raw = sections::read(bytes, features)?;
    let types: Vec<FuncSig> = raw
        .types
        .iter()
        .enumerate()
        .map(|(i, ty)| {
            let (rec_len, rec_index) = raw.rec_meta.get(i).copied().unwrap_or((1, 0));
            Ok(FuncSig {
                params: map_kinds(ty.params())?,
                results: map_kinds(ty.results())?,
                rec_len,
                rec_index,
            })
        })
        .collect::<Result<_, _>>()?;
    let tag_arities: Vec<usize> = {
        let mut arities = Vec::new();
        for im in raw.imports.iter() {
            if let quench_runtime::hir::ImportKind::Tag { type_idx } = im.kind {
                arities.push(types.get(type_idx as usize).map(|t| t.params.len()).unwrap_or(0));
            }
        }
        for idx in raw.tags.iter() {
            arities.push(types.get(*idx as usize).map(|t| t.params.len()).unwrap_or(0));
        }
        arities
    };
    let import_funcs = raw
        .imports
        .iter()
        .filter(|im| matches!(im.kind, quench_runtime::hir::ImportKind::Func { .. }))
        .count();
    let mut funcs = Vec::with_capacity(raw.bodies.len());
    for (index, body) in raw.bodies.iter().enumerate() {
        let ty_index = raw.func_types.get(import_funcs + index).copied();
        funcs.push(
            lower_func(&types, ty_index, &raw.func_types, &raw.gc_types, &tag_arities, body).ok(),
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
    gc_types: &[quench_runtime::hir::GcType],
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
