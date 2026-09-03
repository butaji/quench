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

#[cfg(test)]
mod tests {
    use super::match_import;
    use crate::hir::{FuncSig, HeapKind, HirMemory, HirTable, ImportKind, Kind, RefType, Ty};
    use crate::instance::{Func, Global, Memory, ResolvedImport, Table};
    use crate::native::{Native, RefVal};
    use crate::slot::Slot;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn sig(params: &[Kind], results: &[Kind]) -> FuncSig {
        FuncSig {
            params: params.to_vec().into_boxed_slice(),
            results: results.to_vec().into_boxed_slice(),
            rec_len: 1,
            rec_index: 0,
            has_super: false,
            is_final: true,
            sub_depth: 0,
            chain: Box::new([]),
        }
    }

    fn memory(initial: u64, maximum: Option<u64>) -> Rc<RefCell<Memory>> {
        Rc::new(RefCell::new(Memory {
            data: vec![0; initial as usize * 65_536],
            min: initial,
            max: maximum,
            page: 65_536,
            memory64: false,
            shared: false,
        }))
    }

    fn table(initial: u64, maximum: Option<u64>) -> Rc<RefCell<Table>> {
        Rc::new(RefCell::new(Table {
            elems: vec![RefVal::Null; initial as usize],
            min: initial,
            max: maximum,
            table64: false,
            refty: RefType {
                nullable: true,
                heap: HeapKind::Func,
            },
        }))
    }

    #[test]
    fn imported_limits_are_subtype_checked() {
        let want = ImportKind::Memory(HirMemory {
            memory64: false,
            shared: false,
            initial: 1,
            maximum: Some(3),
            page_size_log2: 16,
        });
        let types = [];
        assert!(match_import(&want, &types, ResolvedImport::Memory(memory(2, Some(3)))).is_ok());
        assert!(match_import(&want, &types, ResolvedImport::Memory(memory(1, Some(4)))).is_err());
        assert!(match_import(&want, &types, ResolvedImport::Memory(memory(1, None))).is_err());
    }

    #[test]
    fn imported_table_limits_and_reference_type_must_match() {
        let want = ImportKind::Table(HirTable {
            table64: false,
            initial: 1,
            maximum: Some(4),
            refty: RefType {
                nullable: true,
                heap: HeapKind::Func,
            },
            init: None,
        });
        let types = [];
        assert!(match_import(&want, &types, ResolvedImport::Table(table(2, Some(4)))).is_ok());
        let wrong_ref = Rc::new(RefCell::new(Table {
            elems: vec![RefVal::Null],
            min: 1,
            max: Some(4),
            table64: false,
            refty: RefType {
                nullable: true,
                heap: HeapKind::Extern,
            },
        }));
        assert!(match_import(&want, &types, ResolvedImport::Table(wrong_ref)).is_err());
    }

    #[test]
    fn mutable_reference_globals_require_invariant_types() {
        let ty = Ty::native(Kind::Ref);
        let want = ImportKind::Global {
            ty,
            refty: Some(RefType {
                nullable: false,
                heap: HeapKind::Func,
            }),
            mutable: true,
        };
        let types = [];
        let matching = Rc::new(RefCell::new(Global {
            value: Slot::Native(Native::Ref(RefVal::Null)),
            mutable: true,
            ty,
            refty: Some(RefType {
                nullable: false,
                heap: HeapKind::Func,
            }),
        }));
        assert!(match_import(&want, &types, ResolvedImport::Global(matching)).is_ok());
        let nullable = Rc::new(RefCell::new(Global {
            value: Slot::Native(Native::Ref(RefVal::Null)),
            mutable: true,
            ty,
            refty: Some(RefType {
                nullable: true,
                heap: HeapKind::Func,
            }),
        }));
        assert!(match_import(&want, &types, ResolvedImport::Global(nullable)).is_err());
    }

    #[test]
    fn function_imports_use_exact_signature_for_exact_types() {
        let want = sig(&[Kind::I32], &[Kind::I32]);
        let got = sig(&[Kind::I32], &[Kind::I32]);
        let types = [want.clone()];
        let result = match_import(
            &ImportKind::Func {
                type_idx: 0,
                exact: true,
            },
            &types,
            ResolvedImport::Func {
                func: Func::Host(got.clone()),
                sig: got,
            },
        );
        assert!(result.is_ok());
    }
}
