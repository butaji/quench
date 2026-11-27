//! The linear-memory access operators: `MemoryLoad` and `MemoryStore`, their discriminants in the
//! shared Wasm lists, and their pure rules. The interpreter's reads and vector results are the
//! extension trait in `glue.rs`.

use super::WasmType;

/// The `MemoryLoad` enum, its discriminants the list's order (`wasm_lists.rs`).
macro_rules! define_memory_loads {
    ($($name:ident $family:ident),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub enum MemoryLoad { $($name,)* }
        impl MemoryLoad {
            const ALL: &'static [Self] = &[$(Self::$name,)*];
        }
    };
}
memory_load_list!(define_memory_loads);

/// The loads' selectors, Wasm operators and widths. The vector loads' results are `vector_of_word`
/// in `glue.rs`, which names the same variants.
macro_rules! loads {
    (scalar { $($scalar:ident, $swidth:ty;)+ } vector { $($name:ident, $word:ty;)+ }) => {
        impl MemoryLoad {
            pub const fn from_tag(tag: u32) -> Option<Self> { if (tag as usize) < Self::ALL.len() { Some(Self::ALL[tag as usize]) } else { None } }
            pub fn from_wasm(op: &wasmparser::Operator<'_>) -> Option<(Self, wasmparser::MemArg)> {
                match op { $(wasmparser::Operator::$scalar { memarg } => Some((Self::$scalar, *memarg)),)+ $(wasmparser::Operator::$name { memarg } => Some((Self::$name, *memarg)),)+ _ => None }
            }
            #[inline(always)] pub const fn width(self) -> usize { match self { $(Self::$scalar => std::mem::size_of::<$swidth>(),)+ $(Self::$name => std::mem::size_of::<$word>(),)+ } }
        }
    }
}
loads! {
    scalar {
        I32Load, i32; I64Load, i64; F32Load, u32; F64Load, u64;
        I32Load8S, i8; I32Load8U, u8; I32Load16S, i16; I32Load16U, u16;
        I64Load8S, i8; I64Load8U, u8; I64Load16S, i16; I64Load16U, u16; I64Load32S, i32; I64Load32U, u32;
    }
    vector {
        V128Load, u128; V128Load8x8S, u64; V128Load8x8U, u64; V128Load16x4S, u64; V128Load16x4U, u64;
        V128Load32x2S, u64; V128Load32x2U, u64; V128Load8Splat, u8; V128Load16Splat, u16;
        V128Load32Splat, u32; V128Load64Splat, u64; V128Load32Zero, u32; V128Load64Zero, u64;
    }
}

/// The stores of the scalar rows run `staged::wasm_memory`'s access (`Access::of_store`); the
/// vector store is `staged::wasm_memory_vector::store_vector`.
macro_rules! stores {
    (scalar { $($scalar:ident, $swidth:ty, $variant:ident;)+ } vector { $($name:ident, $word:ty;)+ }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub enum MemoryStore { $($scalar,)+ $($name,)+ }
        impl MemoryStore {
            const ALL: &'static [Self] = &[$(Self::$scalar,)+ $(Self::$name,)+];
            pub const fn from_tag(tag: u32) -> Option<Self> { if (tag as usize) < Self::ALL.len() { Some(Self::ALL[tag as usize]) } else { None } }
            pub fn from_wasm(op: &wasmparser::Operator<'_>) -> Option<(Self, wasmparser::MemArg)> {
                match op { $(wasmparser::Operator::$scalar { memarg } => Some((Self::$scalar, *memarg)),)+ $(wasmparser::Operator::$name { memarg } => Some((Self::$name, *memarg)),)+ _ => None }
            }
            #[inline(always)] pub const fn value_type(self) -> WasmType { match self { $(Self::$scalar => WasmType::$variant,)+ $(Self::$name => WasmType::V128,)+ } }
            #[inline(always)] pub const fn width(self) -> usize { match self { $(Self::$scalar => std::mem::size_of::<$swidth>(),)+ $(Self::$name => std::mem::size_of::<$word>(),)+ } }
        }
    }
}
stores! {
    scalar {
        I32Store, i32, I32; I64Store, i64, I64; F32Store, u32, F32; F64Store, u64, F64;
        I32Store8, i8, I32; I32Store16, i16, I32; I64Store8, i8, I64; I64Store16, i16, I64; I64Store32, i32, I64;
    }
    vector { V128Store, u128; }
}

#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;
