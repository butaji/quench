//! A Wasm module's constant table.
use super::V128_BYTES;
use crate::bytecode::Constant;

/// A module's constant table: each scalar constant is held once however many operators
/// load it, as V8's `ConstantArrayBuilder` keeps one entry per Smi and number
/// (dotnet-aot-wasm: 16 MB of constants, nearly all repeats). A scalar's bits are its
/// identity, so `-0` and each NaN payload keep their own entries.
///
/// The index holds constant indices only (4 bytes an entry), hashed and compared by the
/// constants they name: the scalars live once, in the table, not again as keys.
pub(super) struct ConstantPool {
    constants: Vec<Constant>,
    index: hashbrown::HashTable<u32>,
    /// Constants the pool handed to its program (`settle`), indexed before `constants`.
    settled: usize,
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

fn hash_of(key: ScalarConstant) -> u64 {
    use std::hash::BuildHasher;
    rustc_hash::FxBuildHasher.hash_one(key)
}

impl ConstantPool {
    pub(super) fn with(initial: impl IntoIterator<Item = Constant>) -> Self {
        let mut pool = Self { constants: Vec::new(), index: Default::default(), settled: 0 };
        for constant in initial {
            pool.intern(&[], constant);
        }
        pool
    }

    /// The index of `constant`, appended unless an equal scalar is held; `None` once the
    /// table outgrows a constant index. `settled` is the table `settle` handed out.
    pub(super) fn intern(&mut self, settled: &[Constant], constant: Constant) -> Option<u32> {
        let Self { constants, index, settled: settled_count } = self;
        let settled_count = *settled_count;
        debug_assert_eq!(settled.len(), settled_count);
        fn at<'c>(settled: &'c [Constant], constants: &'c [Constant], position: u32) -> &'c Constant {
            let position = position as usize;
            if position < settled.len() { &settled[position] } else { &constants[position - settled.len()] }
        }
        let key = ScalarConstant::of(&constant);
        if let Some(key) = key {
            let hash = hash_of(key);
            if let Some(found) = index.find(hash, |&held| ScalarConstant::of(at(settled, constants, held)) == Some(key)) {
                return Some(*found);
            }
        }
        let position = u32::try_from(settled_count + constants.len()).ok()?;
        constants.push(constant);
        if let Some(key) = key {
            index.insert_unique(hash_of(key), position, |&held| {
                ScalarConstant::of(at(settled, constants, held)).map_or(0, hash_of)
            });
        }
        Some(position)
    }

    pub(super) fn into_constants(self) -> Vec<Constant> {
        self.constants
    }

    /// The constants, for the program, and the pool that still answers their indices: a
    /// module that lowers its functions on their first calls interns them again, against
    /// the program's table.
    pub(super) fn settle(mut self) -> (Vec<Constant>, Self) {
        // Nothing settled before: the pool's own table holds every indexed constant.
        debug_assert_eq!(self.settled, 0);
        let Self { constants, index, .. } = &mut self;
        index.shrink_to_fit(|&held| ScalarConstant::of(&constants[held as usize]).map_or(0, hash_of));
        let mut constants = std::mem::take(&mut self.constants);
        constants.shrink_to_fit();
        self.settled += constants.len();
        (constants, self)
    }

    pub(super) fn len(&self) -> usize {
        self.settled + self.constants.len()
    }
}
