//! Atomic access layouts derive from one table; byte rules remain in memory.rs.

use super::memory::{MemoryLoad, MemoryStorage, MemoryStore};
use super::{WasmTrap, WasmType, WasmValue};
use std::sync::{Mutex, MutexGuard};

// One order for atomic accesses across all linear memories and for size/growth.
// Take this before a backing lock; ordinary byte accesses need only that lock.
static MEMORY_ORDER: Mutex<()> = Mutex::new(());
pub(crate) fn memory_order() -> MutexGuard<'static, ()> {
    MEMORY_ORDER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) fn fence() {
    let _order = memory_order();
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
}

#[derive(Clone, Copy, Debug)]
#[repr(u16)]
pub(crate) enum AtomicInput {
    Memory,
    Address,
    Value,
    Auxiliary,
}

#[derive(Clone, Copy, Debug)]
enum AtomicKind {
    Load,
    Store,
    Modify(super::integer::I64BinaryOperator),
    Exchange,
    CompareExchange,
    Wait,
    Notify,
}

impl AtomicKind {
    fn input_count(self) -> u16 {
        (match self {
            Self::Load => AtomicInput::Address,
            Self::Store | Self::Modify(_) | Self::Exchange | Self::Notify => AtomicInput::Value,
            Self::CompareExchange | Self::Wait => AtomicInput::Auxiliary,
        }) as u16
            + 1
    }
    fn returns_value(self) -> bool {
        !matches!(self, Self::Store)
    }
}

#[derive(Clone, Copy)]
struct AtomicLayout {
    kind: AtomicKind,
    load: MemoryLoad,
    store: MemoryStore,
}

macro_rules! atomic_accesses {
    ($($name:ident, $kind:expr, $load:ident, $store:ident;)+) => {
        #[derive(Clone, Copy, Debug)]
        #[repr(u32)]
        pub(crate) enum AtomicOperator { $($name,)+ }
        impl AtomicOperator {
            const ALL: &'static [Self] = &[$(Self::$name,)+];
            pub(crate) fn from_tag(tag: u32) -> Option<Self> { Self::ALL.get(tag as usize).copied() }
            fn from_wasm(op: &wasmparser::Operator<'_>) -> Option<(Self, wasmparser::MemArg)> {
                match op { $(wasmparser::Operator::$name { memarg } => Some((Self::$name, *memarg)),)+ _ => None }
            }
            fn layout(self) -> AtomicLayout {
                match self { $(Self::$name => AtomicLayout { kind: $kind, load: MemoryLoad::$load, store: MemoryStore::$store },)+ }
            }
            pub(crate) fn input_count(self) -> u16 { self.layout().kind.input_count() }
            pub(crate) fn value_type(self) -> WasmType { self.layout().store.value_type() }
            pub(crate) fn returns_value(self) -> bool { self.layout().kind.returns_value() }
            pub(crate) fn auxiliary_type(self) -> WasmType {
                match self.layout().kind { AtomicKind::Wait => WasmType::I64, _ => self.value_type() }
            }
        }
    }
}

use super::integer::I64BinaryOperator;
atomic_accesses! {
    I32AtomicLoad, AtomicKind::Load, I32Load, I32Store;
    I64AtomicLoad, AtomicKind::Load, I64Load, I64Store;
    I32AtomicLoad8U, AtomicKind::Load, I32Load8U, I32Store8;
    I32AtomicLoad16U, AtomicKind::Load, I32Load16U, I32Store16;
    I64AtomicLoad8U, AtomicKind::Load, I64Load8U, I64Store8;
    I64AtomicLoad16U, AtomicKind::Load, I64Load16U, I64Store16;
    I64AtomicLoad32U, AtomicKind::Load, I64Load32U, I64Store32;
    I32AtomicStore, AtomicKind::Store, I32Load, I32Store;
    I64AtomicStore, AtomicKind::Store, I64Load, I64Store;
    I32AtomicStore8, AtomicKind::Store, I32Load8U, I32Store8;
    I32AtomicStore16, AtomicKind::Store, I32Load16U, I32Store16;
    I64AtomicStore8, AtomicKind::Store, I64Load8U, I64Store8;
    I64AtomicStore16, AtomicKind::Store, I64Load16U, I64Store16;
    I64AtomicStore32, AtomicKind::Store, I64Load32U, I64Store32;
    I32AtomicRmwAdd, AtomicKind::Modify(I64BinaryOperator::Add), I32Load, I32Store;
    I64AtomicRmwAdd, AtomicKind::Modify(I64BinaryOperator::Add), I64Load, I64Store;
    I32AtomicRmw8AddU, AtomicKind::Modify(I64BinaryOperator::Add), I32Load8U, I32Store8;
    I32AtomicRmw16AddU, AtomicKind::Modify(I64BinaryOperator::Add), I32Load16U, I32Store16;
    I64AtomicRmw8AddU, AtomicKind::Modify(I64BinaryOperator::Add), I64Load8U, I64Store8;
    I64AtomicRmw16AddU, AtomicKind::Modify(I64BinaryOperator::Add), I64Load16U, I64Store16;
    I64AtomicRmw32AddU, AtomicKind::Modify(I64BinaryOperator::Add), I64Load32U, I64Store32;
    I32AtomicRmwSub, AtomicKind::Modify(I64BinaryOperator::Subtract), I32Load, I32Store;
    I64AtomicRmwSub, AtomicKind::Modify(I64BinaryOperator::Subtract), I64Load, I64Store;
    I32AtomicRmw8SubU, AtomicKind::Modify(I64BinaryOperator::Subtract), I32Load8U, I32Store8;
    I32AtomicRmw16SubU, AtomicKind::Modify(I64BinaryOperator::Subtract), I32Load16U, I32Store16;
    I64AtomicRmw8SubU, AtomicKind::Modify(I64BinaryOperator::Subtract), I64Load8U, I64Store8;
    I64AtomicRmw16SubU, AtomicKind::Modify(I64BinaryOperator::Subtract), I64Load16U, I64Store16;
    I64AtomicRmw32SubU, AtomicKind::Modify(I64BinaryOperator::Subtract), I64Load32U, I64Store32;
    I32AtomicRmwAnd, AtomicKind::Modify(I64BinaryOperator::And), I32Load, I32Store;
    I64AtomicRmwAnd, AtomicKind::Modify(I64BinaryOperator::And), I64Load, I64Store;
    I32AtomicRmw8AndU, AtomicKind::Modify(I64BinaryOperator::And), I32Load8U, I32Store8;
    I32AtomicRmw16AndU, AtomicKind::Modify(I64BinaryOperator::And), I32Load16U, I32Store16;
    I64AtomicRmw8AndU, AtomicKind::Modify(I64BinaryOperator::And), I64Load8U, I64Store8;
    I64AtomicRmw16AndU, AtomicKind::Modify(I64BinaryOperator::And), I64Load16U, I64Store16;
    I64AtomicRmw32AndU, AtomicKind::Modify(I64BinaryOperator::And), I64Load32U, I64Store32;
    I32AtomicRmwOr, AtomicKind::Modify(I64BinaryOperator::Or), I32Load, I32Store;
    I64AtomicRmwOr, AtomicKind::Modify(I64BinaryOperator::Or), I64Load, I64Store;
    I32AtomicRmw8OrU, AtomicKind::Modify(I64BinaryOperator::Or), I32Load8U, I32Store8;
    I32AtomicRmw16OrU, AtomicKind::Modify(I64BinaryOperator::Or), I32Load16U, I32Store16;
    I64AtomicRmw8OrU, AtomicKind::Modify(I64BinaryOperator::Or), I64Load8U, I64Store8;
    I64AtomicRmw16OrU, AtomicKind::Modify(I64BinaryOperator::Or), I64Load16U, I64Store16;
    I64AtomicRmw32OrU, AtomicKind::Modify(I64BinaryOperator::Or), I64Load32U, I64Store32;
    I32AtomicRmwXor, AtomicKind::Modify(I64BinaryOperator::Xor), I32Load, I32Store;
    I64AtomicRmwXor, AtomicKind::Modify(I64BinaryOperator::Xor), I64Load, I64Store;
    I32AtomicRmw8XorU, AtomicKind::Modify(I64BinaryOperator::Xor), I32Load8U, I32Store8;
    I32AtomicRmw16XorU, AtomicKind::Modify(I64BinaryOperator::Xor), I32Load16U, I32Store16;
    I64AtomicRmw8XorU, AtomicKind::Modify(I64BinaryOperator::Xor), I64Load8U, I64Store8;
    I64AtomicRmw16XorU, AtomicKind::Modify(I64BinaryOperator::Xor), I64Load16U, I64Store16;
    I64AtomicRmw32XorU, AtomicKind::Modify(I64BinaryOperator::Xor), I64Load32U, I64Store32;
    I32AtomicRmwXchg, AtomicKind::Exchange, I32Load, I32Store;
    I64AtomicRmwXchg, AtomicKind::Exchange, I64Load, I64Store;
    I32AtomicRmw8XchgU, AtomicKind::Exchange, I32Load8U, I32Store8;
    I32AtomicRmw16XchgU, AtomicKind::Exchange, I32Load16U, I32Store16;
    I64AtomicRmw8XchgU, AtomicKind::Exchange, I64Load8U, I64Store8;
    I64AtomicRmw16XchgU, AtomicKind::Exchange, I64Load16U, I64Store16;
    I64AtomicRmw32XchgU, AtomicKind::Exchange, I64Load32U, I64Store32;
    I32AtomicRmwCmpxchg, AtomicKind::CompareExchange, I32Load, I32Store;
    I64AtomicRmwCmpxchg, AtomicKind::CompareExchange, I64Load, I64Store;
    I32AtomicRmw8CmpxchgU, AtomicKind::CompareExchange, I32Load8U, I32Store8;
    I32AtomicRmw16CmpxchgU, AtomicKind::CompareExchange, I32Load16U, I32Store16;
    I64AtomicRmw8CmpxchgU, AtomicKind::CompareExchange, I64Load8U, I64Store8;
    I64AtomicRmw16CmpxchgU, AtomicKind::CompareExchange, I64Load16U, I64Store16;
    I64AtomicRmw32CmpxchgU, AtomicKind::CompareExchange, I64Load32U, I64Store32;
    MemoryAtomicNotify, AtomicKind::Notify, I32Load, I32Store;
    MemoryAtomicWait32, AtomicKind::Wait, I32Load, I32Store;
    MemoryAtomicWait64, AtomicKind::Wait, I64Load, I64Store;
}

fn integer_bits(value: WasmValue) -> u64 {
    match value {
        WasmValue::I32(value) => u64::from(value as u32),
        WasmValue::I64(value) => value as u64,
        _ => unreachable!("validated atomic integer type"),
    }
}

impl AtomicOperator {
    pub(crate) fn apply(
        self,
        memory: &MemoryStorage,
        address: u64,
        value: Option<WasmValue>,
        replacement: Option<WasmValue>,
    ) -> Result<Option<WasmValue>, WasmTrap> {
        let layout = self.layout();
        match layout.kind {
            AtomicKind::Notify => {
                return memory
                    .notify(address, integer_bits(value.expect("notify count")) as u32)
                    .map(|count| Some(WasmValue::I32(count as i32)));
            }
            AtomicKind::Wait => {
                let WasmValue::I64(timeout) = replacement.expect("wait timeout") else {
                    unreachable!("validated timeout type")
                };
                return memory
                    .wait(
                        layout.load,
                        address,
                        value.expect("wait expected value"),
                        timeout,
                    )
                    .map(|result| Some(WasmValue::I32(result as i32)));
            }
            _ => {}
        }
        let width = layout.load.width();
        let _order = memory_order();
        let mut bytes = memory.lock();
        let loaded = layout.load.read_atomic(&bytes, address)?;
        let mask = u64::MAX >> ((std::mem::size_of::<u64>() - width) * u8::BITS as usize);
        let updated = match layout.kind {
            AtomicKind::Wait | AtomicKind::Notify => unreachable!("handled before byte mutation"),
            AtomicKind::Load => None,
            AtomicKind::Store | AtomicKind::Exchange => value,
            AtomicKind::Modify(op) => Some(op.apply(
                integer_bits(loaded) as i64,
                integer_bits(value.expect("atomic value operand")) as i64,
            )?),
            AtomicKind::CompareExchange => (integer_bits(loaded)
                == (integer_bits(value.expect("atomic expected operand")) & mask))
                .then_some(replacement.expect("atomic replacement operand")),
        };
        if let Some(updated) = updated {
            let bits = integer_bits(updated);
            let value = match layout.store.value_type() {
                WasmType::I32 => WasmValue::I32(bits as i32),
                WasmType::I64 => WasmValue::I64(bits as i64),
                _ => unreachable!("atomic store type"),
            };
            layout.store.write(&mut bytes, address, value)?;
        }
        Ok(layout.kind.returns_value().then_some(loaded))
    }
}

impl super::Lowering<'_> {
    pub(super) fn atomic_operator(
        &mut self,
        operator: &wasmparser::Operator<'_>,
    ) -> Result<bool, crate::Diagnostic> {
        use crate::bytecode::Op;
        if matches!(operator, wasmparser::Operator::AtomicFence) {
            if self.path != super::Reachability::Dead {
                self.emit(Op::WasmAtomicFence, 0, 0, 0, 0)?;
            }
            return Ok(true);
        }
        let Some((operator, arg)) = AtomicOperator::from_wasm(operator) else {
            return Ok(false);
        };
        self.memories.get(arg.memory as usize).ok_or_else(|| {
            crate::Diagnostic::unsupported(self.name, "Wasm memory out of bounds")
        })?;
        if self.path == super::Reachability::Dead {
            return Ok(true);
        }
        let guest_count = operator.input_count() - 1;
        for _ in 0..guest_count {
            self.pop()?;
        }
        let result = self.depth;
        // Preserve guest inputs until the complete window has read them.
        self.depth = result + guest_count;
        let memory = self.memory_binding(arg.memory)?;
        let address = self.push()?;
        let offset = self.scalar_constant(WasmValue::I64(arg.offset as i64))?;
        self.emit(Op::WasmMemoryAddress, address, result, memory, offset)?;
        for index in 1..guest_count {
            let target = self.push()?;
            self.emit(Op::Move, target, result + index, 0, 0)?;
        }
        self.emit(
            Op::WasmAtomicAccess,
            result,
            memory,
            operator.input_count(),
            operator as u32,
        )?;
        self.depth = result;
        if operator.returns_value() {
            self.push()?;
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wasm::memory::SharedMemory;
    use std::sync::Arc;

    #[test]
    fn atomic_accesses_preserve_critical_sections_and_cross_memory_order() {
        let shared = Arc::new(SharedMemory::new(vec![0; std::mem::size_of::<i32>()]));
        let memory = MemoryStorage::Shared(shared.clone());
        let mut observed = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|_| {
                    let memory = MemoryStorage::Shared(shared.clone());
                    scope.spawn(move || {
                        (0..100)
                            .map(|_| {
                                AtomicOperator::I32AtomicRmwAdd
                                    .apply(&memory, 0, Some(WasmValue::I32(1)), None)
                                    .unwrap()
                                    .unwrap()
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|handle| handle.join().unwrap())
                .map(integer_bits)
                .collect::<Vec<_>>()
        });
        observed.sort_unstable();
        assert_eq!(observed, (0..400).collect::<Vec<_>>());
        assert_eq!(
            AtomicOperator::I32AtomicLoad
                .apply(&memory, 0, None, None)
                .unwrap(),
            Some(WasmValue::I32(400))
        );
        let first = Arc::new(SharedMemory::new(vec![0; std::mem::size_of::<i32>()]));
        let second = Arc::new(SharedMemory::new(vec![0; std::mem::size_of::<i32>()]));
        let epoch = std::sync::Barrier::new(3);
        let results = std::thread::scope(|scope| {
            let handles: Vec<_> = [(&first, &second), (&second, &first)]
                .into_iter()
                .map(|(own, other)| {
                    let epoch = &epoch;
                    let own = MemoryStorage::Shared(own.clone());
                    let other = MemoryStorage::Shared(other.clone());
                    scope.spawn(move || {
                        let mut reads = Vec::new();
                        for _ in 0..100 {
                            epoch.wait();
                            AtomicOperator::I32AtomicStore
                                .apply(&own, 0, Some(WasmValue::I32(1)), None)
                                .unwrap();
                            reads.push(
                                AtomicOperator::I32AtomicLoad
                                    .apply(&other, 0, None, None)
                                    .unwrap(),
                            );
                            epoch.wait();
                        }
                        reads
                    })
                })
                .collect();
            for _ in 0..100 {
                for memory in [&first, &second] {
                    AtomicOperator::I32AtomicStore
                        .apply(
                            &MemoryStorage::Shared(memory.clone()),
                            0,
                            Some(WasmValue::I32(0)),
                            None,
                        )
                        .unwrap();
                }
                epoch.wait();
                epoch.wait();
            }
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect::<Vec<_>>()
        });
        for (first, second) in results[0].iter().zip(&results[1]) {
            assert_ne!(
                (first, second),
                (&Some(WasmValue::I32(0)), &Some(WasmValue::I32(0)))
            );
        }
    }
}
