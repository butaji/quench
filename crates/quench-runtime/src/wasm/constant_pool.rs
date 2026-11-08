//! A Wasm module's constant table.
use super::V128_BYTES;
use crate::bytecode::Constant;

/// A module's constant table: each scalar constant is held once however many operators
/// load it, as V8's `ConstantArrayBuilder` keeps one entry per Smi and number
/// (dotnet-aot-wasm: 16 MB of constants, nearly all repeats). A scalar's bits are its
/// identity, so `-0` and each NaN payload keep their own entries.
pub(super) struct ConstantPool {
    constants: Vec<Constant>,
    index: rustc_hash::FxHashMap<ScalarConstant, u32>,
}

/// The exact bits of a scalar constant, by kind.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ScalarConstant {
    Number(u64),
    Bits64(u64),
    V128([u8; V128_BYTES]),
    Boolean(bool),
    Null,
    Undefined,
}

impl ScalarConstant {
    fn of(constant: &Constant) -> Option<Self> {
        Some(match *constant {
            Constant::Number(value) => Self::Number(value.to_bits()),
            Constant::WasmBits64(bits) => Self::Bits64(bits),
            Constant::WasmV128(bytes) => Self::V128(bytes),
            Constant::Boolean(value) => Self::Boolean(value),
            Constant::Null => Self::Null,
            Constant::Undefined => Self::Undefined,
            Constant::String(_) | Constant::StringUnits(_) | Constant::BigInt(_) => return None,
        })
    }
}

impl ConstantPool {
    pub(super) fn with(initial: impl IntoIterator<Item = Constant>) -> Self {
        let mut pool = Self { constants: Vec::new(), index: Default::default() };
        for constant in initial {
            pool.intern(constant);
        }
        pool
    }

    /// The index of `constant`, appended unless an equal scalar is held; `None` once the
    /// table outgrows a constant index.
    pub(super) fn intern(&mut self, constant: Constant) -> Option<u32> {
        let key = ScalarConstant::of(&constant);
        if let Some(index) = key.and_then(|key| self.index.get(&key)) {
            return Some(*index);
        }
        let index = u32::try_from(self.constants.len()).ok()?;
        self.constants.push(constant);
        if let Some(key) = key {
            self.index.insert(key, index);
        }
        Some(index)
    }

    pub(super) fn into_constants(self) -> Vec<Constant> {
        self.constants
    }
}
