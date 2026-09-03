//! Collect module sections into one data record.

use crate::hir::{
    ConstExpr, ConstOp, Export, GcStorage, GcType, HeapKind, HirData, HirElem, HirGlobal,
    HirImport, HirMemory, HirTable, ImportKind, RefType as HirRef,
};
use wasmparser::{
    AbstractHeapType, CompositeInnerType, DataKind, ElementItems, ElementKind, ExternalKind,
    FuncType, HeapType, Operator, Parser, Payload, TypeRef, ValType, WasmFeatures,
};

use super::{native_ty, parse_err, LowerError};

pub struct RawModule<'a> {
    pub types: Vec<FuncType>,
    pub func_types: Vec<u32>,
    pub exports: Vec<(Box<str>, Export)>,
    pub bodies: Vec<wasmparser::FunctionBody<'a>>,
    pub imports: Vec<HirImport>,
    pub memories: Vec<HirMemory>,
    pub tables: Vec<HirTable>,
    pub globals: Vec<HirGlobal>,
    pub datas: Vec<HirData>,
    pub elems: Vec<HirElem>,
    pub tags: Vec<u32>,
    pub gc_types: Vec<GcType>,
    pub rec_meta: Vec<(u32, u32)>,
    pub start: Option<u32>,
}

pub fn read<'a>(bytes: &'a [u8], features: WasmFeatures) -> Result<RawModule<'a>, LowerError> {
    let mut parser = Parser::new(0);
    parser.set_features(features);
    let mut raw = RawModule {
        types: Vec::new(),
        func_types: Vec::new(),
        exports: Vec::new(),
        bodies: Vec::new(),
        imports: Vec::new(),
        memories: Vec::new(),
        tables: Vec::new(),
        globals: Vec::new(),
        datas: Vec::new(),
        elems: Vec::new(),
        tags: Vec::new(),
        gc_types: Vec::new(),
        rec_meta: Vec::new(),
        start: None,
    };
    for payload in parser.parse_all(bytes) {
        take(&mut raw, payload.map_err(parse_err)?)?;
    }
    Ok(raw)
}

fn take<'a>(raw: &mut RawModule<'a>, payload: Payload<'a>) -> Result<(), LowerError> {
    match payload {
        Payload::TypeSection(reader) => {
            for rec in reader {
                let rec = rec.map_err(parse_err)?;
                let rec_types: Vec<_> = rec.types().collect();
                let rec_len = rec_types.len() as u32;
                let rec_start = raw.gc_types.len() as u32;
                for (rec_index, sub) in rec_types.into_iter().enumerate() {
                    raw.rec_meta.push((rec_len, rec_index as u32));
                    let super_idx = packed_idx(sub.supertype_idx, rec_start);
                    let descriptor_idx = packed_idx(sub.composite_type.descriptor_idx, rec_start);
                    let describes_idx = packed_idx(sub.composite_type.describes_idx, rec_start);
                    let is_final = sub.is_final;
                    match &sub.composite_type.inner {
                        CompositeInnerType::Func(func) => {
                            raw.types.push(func.clone());
                            raw.gc_types.push(GcType::Func {
                                super_idx,
                                descriptor_idx,
                                describes_idx,
                                is_final,
                            });
                        }
                        CompositeInnerType::Struct(st) => {
                            raw.types.push(wasmparser::FuncType::new([], []));
                            raw.gc_types.push(GcType::Struct {
                                fields: st
                                    .fields
                                    .iter()
                                    .map(|f| gc_storage(f, rec_start))
                                    .collect(),
                                super_idx,
                                descriptor_idx,
                                describes_idx,
                                is_final,
                            });
                        }
                        CompositeInnerType::Array(arr) => {
                            raw.types.push(wasmparser::FuncType::new([], []));
                            raw.gc_types.push(GcType::Array {
                                elem: gc_storage(&arr.0, rec_start),
                                super_idx,
                                descriptor_idx,
                                describes_idx,
                                is_final,
                            });
                        }
                        _ => {
                            raw.types.push(wasmparser::FuncType::new([], []));
                            raw.gc_types.push(GcType::Func {
                                super_idx,
                                descriptor_idx,
                                describes_idx,
                                is_final,
                            });
                        }
                    }
                }
            }
        }
        Payload::FunctionSection(reader) => {
            for ty in reader {
                raw.func_types.push(ty.map_err(parse_err)?);
            }
        }
        Payload::ExportSection(reader) => {
            for export in reader {
                let export = export.map_err(parse_err)?;
                if let Some(kind) = export_kind(export.kind, export.index) {
                    raw.exports
                        .push((export.name.to_string().into_boxed_str(), kind));
                }
            }
        }
        Payload::CodeSectionEntry(body) => raw.bodies.push(body),
        Payload::ImportSection(reader) => {
            for group in reader {
                take_imports(raw, group.map_err(parse_err)?)?;
            }
        }
        Payload::MemorySection(reader) => {
            for memory in reader {
                let memory = memory.map_err(parse_err)?;
                raw.memories.push(hir_memory(memory));
            }
        }
        Payload::TableSection(reader) => {
            for table in reader {
                let table = table.map_err(parse_err)?;
                raw.tables.push(hir_table_init(table)?);
            }
        }
        Payload::GlobalSection(reader) => {
            for global in reader {
                let global = global.map_err(parse_err)?;
                raw.globals.push(HirGlobal {
                    ty: native_ty(global.ty.content_type)?,
                    refty: val_refty(global.ty.content_type),
                    mutable: global.ty.mutable,
                    init: const_expr(&global.init_expr)?,
                });
            }
        }
        Payload::DataSection(reader) => {
            for data in reader {
                raw.datas.push(hir_data(data.map_err(parse_err)?)?);
            }
        }
        Payload::ElementSection(reader) => {
            for elem in reader {
                raw.elems.push(hir_elem(elem.map_err(parse_err)?)?);
            }
        }
        Payload::StartSection { func, .. } => raw.start = Some(func),
        Payload::TagSection(reader) => {
            for tag in reader {
                raw.tags.push(tag.map_err(parse_err)?.func_type_idx);
            }
        }
        _ => {}
    }
    Ok(())
}

fn take_imports(raw: &mut RawModule<'_>, group: wasmparser::Imports<'_>) -> Result<(), LowerError> {
    match group {
        wasmparser::Imports::Single(_, import) => push_import(raw, import),
        wasmparser::Imports::Compact1 { module, items } => {
            for item in items {
                let item = item.map_err(parse_err)?;
                push_import(
                    raw,
                    wasmparser::Import {
                        module,
                        name: item.name,
                        ty: item.ty,
                    },
                )?;
            }
            Ok(())
        }
        wasmparser::Imports::Compact2 { module, ty, names } => {
            for name in names {
                let name = name.map_err(parse_err)?;
                push_import(raw, wasmparser::Import { module, name, ty })?;
            }
            Ok(())
        }
    }
}

fn push_import(raw: &mut RawModule<'_>, import: wasmparser::Import<'_>) -> Result<(), LowerError> {
    if let TypeRef::Func(ty) | TypeRef::FuncExact(ty) = import.ty {
        raw.func_types.push(ty);
    }
    raw.imports.push(hir_import(import)?);
    Ok(())
}

fn export_kind(kind: ExternalKind, index: u32) -> Option<Export> {
    match kind {
        ExternalKind::Func | ExternalKind::FuncExact => Some(Export::Func(index)),
        ExternalKind::Table => Some(Export::Table(index)),
        ExternalKind::Memory => Some(Export::Memory(index)),
        ExternalKind::Global => Some(Export::Global(index)),
        ExternalKind::Tag => Some(Export::Tag(index)),
    }
}

fn hir_import(import: wasmparser::Import<'_>) -> Result<HirImport, LowerError> {
    let kind = match import.ty {
        TypeRef::Func(type_idx) => ImportKind::Func {
            type_idx,
            exact: false,
        },
        TypeRef::FuncExact(type_idx) => ImportKind::Func {
            type_idx,
            exact: true,
        },
        TypeRef::Table(ty) => ImportKind::Table(hir_table(ty)),
        TypeRef::Memory(ty) => ImportKind::Memory(hir_memory(ty)),
        TypeRef::Global(ty) => ImportKind::Global {
            ty: native_ty(ty.content_type)?,
            refty: val_refty(ty.content_type),
            mutable: ty.mutable,
        },
        TypeRef::Tag(ty) => ImportKind::Tag {
            type_idx: ty.func_type_idx,
        },
    };
    Ok(HirImport {
        module: import.module.to_string().into_boxed_str(),
        name: import.name.to_string().into_boxed_str(),
        kind,
    })
}

fn hir_memory(ty: wasmparser::MemoryType) -> HirMemory {
    HirMemory {
        memory64: ty.memory64,
        shared: ty.shared,
        initial: ty.initial,
        maximum: ty.maximum,
        page_size_log2: ty.page_size_log2(),
    }
}

fn packed_idx(idx: Option<wasmparser::PackedIndex>, rec_start: u32) -> Option<u32> {
    idx.and_then(|i| {
        i.as_module_index()
            .or_else(|| i.as_rec_group_index().map(|j| rec_start + j))
    })
}

fn gc_storage(field: &wasmparser::FieldType, rec_start: u32) -> GcStorage {
    match field.element_type {
        wasmparser::StorageType::I8 => GcStorage::I8,
        wasmparser::StorageType::I16 => GcStorage::I16,
        wasmparser::StorageType::Val(wasmparser::ValType::Ref(rt)) => {
            let type_idx = match rt.heap_type() {
                wasmparser::HeapType::Concrete(idx) | wasmparser::HeapType::Exact(idx) => idx
                    .as_module_index()
                    .or_else(|| idx.as_rec_group_index().map(|j| rec_start + j)),
                _ => None,
            };
            GcStorage::Ref { type_idx }
        }
        wasmparser::StorageType::Val(ty) => {
            GcStorage::Val(super::kind(ty).unwrap_or(crate::hir::Kind::Ref))
        }
    }
}

fn hir_table(ty: wasmparser::TableType) -> HirTable {
    HirTable {
        table64: ty.table64,
        initial: ty.initial,
        maximum: ty.maximum,
        refty: hir_refty(ty.element_type),
        init: None,
    }
}

fn hir_table_init(table: wasmparser::Table<'_>) -> Result<HirTable, LowerError> {
    let mut hir = hir_table(table.ty);
    hir.init = match table.init {
        wasmparser::TableInit::RefNull => None,
        wasmparser::TableInit::Expr(expr) => Some(const_expr(&expr)?),
    };
    Ok(hir)
}

fn val_refty(ty: ValType) -> Option<HirRef> {
    match ty {
        ValType::Ref(ty) => Some(hir_refty(ty)),
        _ => None,
    }
}

fn hir_refty(ty: wasmparser::RefType) -> HirRef {
    HirRef {
        nullable: ty.is_nullable(),
        heap: match ty.heap_type() {
            HeapType::Abstract {
                ty: AbstractHeapType::Func,
                ..
            } => HeapKind::Func,
            HeapType::Abstract {
                ty: AbstractHeapType::Extern,
                ..
            } => HeapKind::Extern,
            HeapType::Abstract {
                ty: AbstractHeapType::Any,
                ..
            } => HeapKind::Any,
            HeapType::Abstract {
                ty: AbstractHeapType::Eq,
                ..
            } => HeapKind::Eq,
            HeapType::Abstract {
                ty: AbstractHeapType::I31,
                ..
            } => HeapKind::I31,
            HeapType::Abstract {
                ty: AbstractHeapType::Struct,
                ..
            } => HeapKind::Struct,
            HeapType::Abstract {
                ty: AbstractHeapType::Array,
                ..
            } => HeapKind::Array,
            HeapType::Abstract {
                ty: AbstractHeapType::None,
                ..
            } => HeapKind::None,
            HeapType::Abstract {
                ty: AbstractHeapType::NoFunc,
                ..
            } => HeapKind::NoFunc,
            HeapType::Abstract {
                ty: AbstractHeapType::NoExtern,
                ..
            } => HeapKind::NoExtern,
            HeapType::Concrete(_) | HeapType::Exact(_) => HeapKind::Concrete,
            _ => HeapKind::Other,
        },
    }
}

fn hir_data(data: wasmparser::Data<'_>) -> Result<HirData, LowerError> {
    match data.kind {
        DataKind::Passive => Ok(HirData {
            mem: 0,
            offset: None,
            bytes: data.data.to_vec().into_boxed_slice(),
        }),
        DataKind::Active {
            memory_index,
            offset_expr,
        } => Ok(HirData {
            mem: memory_index,
            offset: Some(const_expr(&offset_expr)?),
            bytes: data.data.to_vec().into_boxed_slice(),
        }),
    }
}

fn hir_elem(elem: wasmparser::Element<'_>) -> Result<HirElem, LowerError> {
    let items = read_elem_items(elem.items)?;
    match elem.kind {
        ElementKind::Passive => Ok(HirElem {
            table: 0,
            offset: None,
            declared: false,
            items,
        }),
        ElementKind::Declared => Ok(HirElem {
            table: 0,
            offset: None,
            declared: true,
            items,
        }),
        ElementKind::Active {
            table_index,
            offset_expr,
        } => Ok(HirElem {
            table: table_index.unwrap_or(0),
            offset: Some(const_expr(&offset_expr)?),
            declared: false,
            items,
        }),
    }
}

fn read_elem_items(items: ElementItems<'_>) -> Result<Box<[ConstExpr]>, LowerError> {
    match items {
        ElementItems::Functions(reader) => reader
            .into_iter()
            .map(|i| {
                i.map(|func| ConstExpr {
                    ops: Box::new([ConstOp::RefFunc(func)]),
                })
                .map_err(parse_err)
            })
            .collect(),
        ElementItems::Expressions(_ty, reader) => {
            let mut out = Vec::new();
            for expr in reader {
                out.push(const_expr(&expr.map_err(parse_err)?)?);
            }
            Ok(out.into_boxed_slice())
        }
    }
}

fn const_expr(expr: &wasmparser::ConstExpr<'_>) -> Result<ConstExpr, LowerError> {
    let mut ops = Vec::new();
    for op in expr.get_operators_reader() {
        match op.map_err(parse_err)? {
            Operator::End => {}
            other => ops.push(const_op(other)?),
        }
    }
    Ok(ConstExpr {
        ops: ops.into_boxed_slice(),
    })
}

fn const_op(op: Operator<'_>) -> Result<ConstOp, LowerError> {
    Ok(match op {
        Operator::I32Const { value } => ConstOp::I32(value),
        Operator::I64Const { value } => ConstOp::I64(value),
        Operator::F32Const { value } => ConstOp::F32(value.bits()),
        Operator::F64Const { value } => ConstOp::F64(value.bits()),
        Operator::V128Const { value } => ConstOp::V128(u128::from(value)),
        Operator::RefNull { .. } => ConstOp::RefNull,
        Operator::RefFunc { function_index } => ConstOp::RefFunc(function_index),
        Operator::GlobalGet { global_index } => ConstOp::GlobalGet(global_index),
        Operator::I32Add => ConstOp::I32Add,
        Operator::I32Sub => ConstOp::I32Sub,
        Operator::I32Mul => ConstOp::I32Mul,
        Operator::I32And => ConstOp::I32And,
        Operator::I32Or => ConstOp::I32Or,
        Operator::I32Xor => ConstOp::I32Xor,
        Operator::I64Add => ConstOp::I64Add,
        Operator::I64Sub => ConstOp::I64Sub,
        Operator::I64Mul => ConstOp::I64Mul,
        Operator::I64And => ConstOp::I64And,
        Operator::I64Or => ConstOp::I64Or,
        Operator::I64Xor => ConstOp::I64Xor,
        Operator::ArrayNewDefault { array_type_index } => {
            ConstOp::ArrayNewDefault(array_type_index)
        }
        Operator::RefI31 => ConstOp::RefI31,
        Operator::ArrayNew { array_type_index } => ConstOp::ArrayNew(array_type_index),
        Operator::StructNewDefault { struct_type_index } => {
            ConstOp::StructNewDefault(struct_type_index)
        }
        Operator::StructNew { struct_type_index } => ConstOp::StructNew(struct_type_index),
        Operator::StructNewDesc { struct_type_index } => ConstOp::StructNewDesc(struct_type_index),
        Operator::StructNewDefaultDesc { struct_type_index } => {
            ConstOp::StructNewDefaultDesc(struct_type_index)
        }
        Operator::ArrayNewFixed {
            array_type_index,
            array_size,
        } => ConstOp::ArrayNewFixed {
            type_idx: array_type_index,
            n: array_size,
        },
        Operator::AnyConvertExtern => ConstOp::AnyConvertExtern,
        Operator::ExternConvertAny => ConstOp::ExternConvertAny,
        _ => return Err(LowerError::Unsupported),
    })
}
