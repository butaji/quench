//! Import matching: one table of rules, not a language.

use super::{Func, Global, InvokeError, Memory, ResolvedImport, Table};
use crate::hir::{FuncSig, HeapKind, HirMemory, HirTable, ImportKind, Kind, RefType, Ty};

pub fn match_import(
    kind: &ImportKind,
    types: &[FuncSig],
    export: ResolvedImport,
) -> Result<ResolvedImport, InvokeError> {
    match (kind, export) {
        (ImportKind::Func { type_idx, exact }, ResolvedImport::Func { func, sig }) => {
            let want = types
                .get(*type_idx as usize)
                .ok_or(InvokeError::Unlinkable("incompatible import type"))?;
            let got = sig_of(&func, &sig);
            let ok = if *exact {
                got == *want
            } else {
                got.assignable_to(want)
            };
            if !ok {
                return Err(InvokeError::Unlinkable("incompatible import type"));
            }
            Ok(ResolvedImport::Func { func, sig })
        }
        (ImportKind::Memory(want), ResolvedImport::Memory(got)) => {
            if !memory_ok(want, &got.borrow()) {
                return Err(InvokeError::Unlinkable("incompatible import type"));
            }
            Ok(ResolvedImport::Memory(got))
        }
        (ImportKind::Table(want), ResolvedImport::Table(got)) => {
            if !table_ok(want, &got.borrow()) {
                return Err(InvokeError::Unlinkable("incompatible import type"));
            }
            Ok(ResolvedImport::Table(got))
        }
        (ImportKind::Global { ty, refty, mutable }, ResolvedImport::Global(got)) => {
            if !global_ok(*ty, *refty, *mutable, &got.borrow()) {
                return Err(InvokeError::Unlinkable("incompatible import type"));
            }
            Ok(ResolvedImport::Global(got))
        }
        (ImportKind::Tag { type_idx }, ResolvedImport::Tag { sig, id }) => {
            let want = types
                .get(*type_idx as usize)
                .ok_or(InvokeError::Unlinkable("incompatible import type"))?;
            if want.params != sig.params
                || want.results != sig.results
                || want.rec_len != sig.rec_len
                || want.rec_index != sig.rec_index
            {
                return Err(InvokeError::Unlinkable("incompatible import type"));
            }
            Ok(ResolvedImport::Tag { sig, id })
        }
        _ => Err(InvokeError::Unlinkable("incompatible import type")),
    }
}

fn sig_of(_func: &Func, exported: &FuncSig) -> FuncSig {
    exported.clone()
}

fn memory_ok(want: &HirMemory, got: &Memory) -> bool {
    want.memory64 == got.memory64
        && want.shared == got.shared
        && got.page == (1u32 << want.page_size_log2)
        && limits_ok(got.pages(), got.max, want.initial, want.maximum)
}

fn table_ok(want: &HirTable, got: &Table) -> bool {
    want.table64 == got.table64
        && ref_eq(want.refty, got.refty)
        && limits_ok(got.elems.len() as u64, got.max, want.initial, want.maximum)
}

fn global_ok(ty: Ty, refty: Option<RefType>, mutable: bool, got: &Global) -> bool {
    if mutable != got.mutable || ty.kind != got.ty.kind {
        return false;
    }
    match (ty.kind, refty, got.refty) {
        (Kind::Ref, Some(want), Some(have)) if mutable => ref_eq(want, have),
        (Kind::Ref, Some(want), Some(have)) => is_subtype(have, want),
        (Kind::Ref, _, _) => false,
        _ => true,
    }
}

fn limits_ok(got_min: u64, got_max: Option<u64>, want_min: u64, want_max: Option<u64>) -> bool {
    if got_min < want_min {
        return false;
    }
    match (got_max, want_max) {
        (_, None) => true,
        (Some(got), Some(want)) => got <= want,
        (None, Some(_)) => false,
    }
}

fn ref_eq(a: RefType, b: RefType) -> bool {
    a.nullable == b.nullable && a.heap == b.heap
}

fn is_subtype(got: RefType, want: RefType) -> bool {
    if !want.nullable && got.nullable {
        return false;
    }
    heap_subtype(got.heap, want.heap)
}

fn heap_subtype(got: HeapKind, want: HeapKind) -> bool {
    got == want
        || matches!(
            (got, want),
            (HeapKind::Concrete | HeapKind::NoFunc, HeapKind::Func)
                | (HeapKind::NoFunc, HeapKind::Concrete)
                | (HeapKind::NoExtern, HeapKind::Extern)
                | (
                    HeapKind::None
                        | HeapKind::Eq
                        | HeapKind::I31
                        | HeapKind::Struct
                        | HeapKind::Array,
                    HeapKind::Any
                )
        )
}
