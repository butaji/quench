use super::WasmType;
use wasmparser::Operator;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AtomicRmw {
    Add,
    Sub,
    And,
    Or,
    Xor,
    Exchange,
    CompareExchange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WasmAtomicAction {
    Load,
    Store,
    Rmw(AtomicRmw),
    Notify,
    Wait,
    Fence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WasmAtomicSite {
    pub action: WasmAtomicAction,
    pub value_type: Option<WasmType>,
    pub access_width: u8,
    pub memory_index: u32,
    pub offset: u64,
}

impl WasmAtomicSite {
    pub(crate) fn from_operator(operator: &Operator<'_>) -> Option<Self> {
        use AtomicRmw as Rmw;
        use Operator::*;
        use WasmAtomicAction as Action;

        if matches!(operator, AtomicFence) {
            return Some(Self {
                action: Action::Fence,
                value_type: None,
                access_width: 0,
                memory_index: 0,
                offset: 0,
            });
        }

        let (action, value_type, access_width, memarg) = match operator {
            MemoryAtomicNotify { memarg } => (Action::Notify, None, 4, memarg),
            MemoryAtomicWait32 { memarg } => (Action::Wait, Some(WasmType::I32), 4, memarg),
            MemoryAtomicWait64 { memarg } => (Action::Wait, Some(WasmType::I64), 8, memarg),
            I32AtomicLoad { memarg } => (Action::Load, Some(WasmType::I32), 4, memarg),
            I64AtomicLoad { memarg } => (Action::Load, Some(WasmType::I64), 8, memarg),
            I32AtomicLoad8U { memarg } => (Action::Load, Some(WasmType::I32), 1, memarg),
            I32AtomicLoad16U { memarg } => (Action::Load, Some(WasmType::I32), 2, memarg),
            I64AtomicLoad8U { memarg } => (Action::Load, Some(WasmType::I64), 1, memarg),
            I64AtomicLoad16U { memarg } => (Action::Load, Some(WasmType::I64), 2, memarg),
            I64AtomicLoad32U { memarg } => (Action::Load, Some(WasmType::I64), 4, memarg),
            I32AtomicStore { memarg } => (Action::Store, Some(WasmType::I32), 4, memarg),
            I64AtomicStore { memarg } => (Action::Store, Some(WasmType::I64), 8, memarg),
            I32AtomicStore8 { memarg } => (Action::Store, Some(WasmType::I32), 1, memarg),
            I32AtomicStore16 { memarg } => (Action::Store, Some(WasmType::I32), 2, memarg),
            I64AtomicStore8 { memarg } => (Action::Store, Some(WasmType::I64), 1, memarg),
            I64AtomicStore16 { memarg } => (Action::Store, Some(WasmType::I64), 2, memarg),
            I64AtomicStore32 { memarg } => (Action::Store, Some(WasmType::I64), 4, memarg),
            I32AtomicRmwAdd { memarg } => (Action::Rmw(Rmw::Add), Some(WasmType::I32), 4, memarg),
            I64AtomicRmwAdd { memarg } => (Action::Rmw(Rmw::Add), Some(WasmType::I64), 8, memarg),
            I32AtomicRmw8AddU { memarg } => (Action::Rmw(Rmw::Add), Some(WasmType::I32), 1, memarg),
            I32AtomicRmw16AddU { memarg } => {
                (Action::Rmw(Rmw::Add), Some(WasmType::I32), 2, memarg)
            }
            I64AtomicRmw8AddU { memarg } => (Action::Rmw(Rmw::Add), Some(WasmType::I64), 1, memarg),
            I64AtomicRmw16AddU { memarg } => {
                (Action::Rmw(Rmw::Add), Some(WasmType::I64), 2, memarg)
            }
            I64AtomicRmw32AddU { memarg } => {
                (Action::Rmw(Rmw::Add), Some(WasmType::I64), 4, memarg)
            }
            I32AtomicRmwSub { memarg } => (Action::Rmw(Rmw::Sub), Some(WasmType::I32), 4, memarg),
            I64AtomicRmwSub { memarg } => (Action::Rmw(Rmw::Sub), Some(WasmType::I64), 8, memarg),
            I32AtomicRmw8SubU { memarg } => (Action::Rmw(Rmw::Sub), Some(WasmType::I32), 1, memarg),
            I32AtomicRmw16SubU { memarg } => {
                (Action::Rmw(Rmw::Sub), Some(WasmType::I32), 2, memarg)
            }
            I64AtomicRmw8SubU { memarg } => (Action::Rmw(Rmw::Sub), Some(WasmType::I64), 1, memarg),
            I64AtomicRmw16SubU { memarg } => {
                (Action::Rmw(Rmw::Sub), Some(WasmType::I64), 2, memarg)
            }
            I64AtomicRmw32SubU { memarg } => {
                (Action::Rmw(Rmw::Sub), Some(WasmType::I64), 4, memarg)
            }
            I32AtomicRmwAnd { memarg } => (Action::Rmw(Rmw::And), Some(WasmType::I32), 4, memarg),
            I64AtomicRmwAnd { memarg } => (Action::Rmw(Rmw::And), Some(WasmType::I64), 8, memarg),
            I32AtomicRmw8AndU { memarg } => (Action::Rmw(Rmw::And), Some(WasmType::I32), 1, memarg),
            I32AtomicRmw16AndU { memarg } => {
                (Action::Rmw(Rmw::And), Some(WasmType::I32), 2, memarg)
            }
            I64AtomicRmw8AndU { memarg } => (Action::Rmw(Rmw::And), Some(WasmType::I64), 1, memarg),
            I64AtomicRmw16AndU { memarg } => {
                (Action::Rmw(Rmw::And), Some(WasmType::I64), 2, memarg)
            }
            I64AtomicRmw32AndU { memarg } => {
                (Action::Rmw(Rmw::And), Some(WasmType::I64), 4, memarg)
            }
            I32AtomicRmwOr { memarg } => (Action::Rmw(Rmw::Or), Some(WasmType::I32), 4, memarg),
            I64AtomicRmwOr { memarg } => (Action::Rmw(Rmw::Or), Some(WasmType::I64), 8, memarg),
            I32AtomicRmw8OrU { memarg } => (Action::Rmw(Rmw::Or), Some(WasmType::I32), 1, memarg),
            I32AtomicRmw16OrU { memarg } => (Action::Rmw(Rmw::Or), Some(WasmType::I32), 2, memarg),
            I64AtomicRmw8OrU { memarg } => (Action::Rmw(Rmw::Or), Some(WasmType::I64), 1, memarg),
            I64AtomicRmw16OrU { memarg } => (Action::Rmw(Rmw::Or), Some(WasmType::I64), 2, memarg),
            I64AtomicRmw32OrU { memarg } => (Action::Rmw(Rmw::Or), Some(WasmType::I64), 4, memarg),
            I32AtomicRmwXor { memarg } => (Action::Rmw(Rmw::Xor), Some(WasmType::I32), 4, memarg),
            I64AtomicRmwXor { memarg } => (Action::Rmw(Rmw::Xor), Some(WasmType::I64), 8, memarg),
            I32AtomicRmw8XorU { memarg } => (Action::Rmw(Rmw::Xor), Some(WasmType::I32), 1, memarg),
            I32AtomicRmw16XorU { memarg } => {
                (Action::Rmw(Rmw::Xor), Some(WasmType::I32), 2, memarg)
            }
            I64AtomicRmw8XorU { memarg } => (Action::Rmw(Rmw::Xor), Some(WasmType::I64), 1, memarg),
            I64AtomicRmw16XorU { memarg } => {
                (Action::Rmw(Rmw::Xor), Some(WasmType::I64), 2, memarg)
            }
            I64AtomicRmw32XorU { memarg } => {
                (Action::Rmw(Rmw::Xor), Some(WasmType::I64), 4, memarg)
            }
            I32AtomicRmwXchg { memarg } => {
                (Action::Rmw(Rmw::Exchange), Some(WasmType::I32), 4, memarg)
            }
            I64AtomicRmwXchg { memarg } => {
                (Action::Rmw(Rmw::Exchange), Some(WasmType::I64), 8, memarg)
            }
            I32AtomicRmw8XchgU { memarg } => {
                (Action::Rmw(Rmw::Exchange), Some(WasmType::I32), 1, memarg)
            }
            I32AtomicRmw16XchgU { memarg } => {
                (Action::Rmw(Rmw::Exchange), Some(WasmType::I32), 2, memarg)
            }
            I64AtomicRmw8XchgU { memarg } => {
                (Action::Rmw(Rmw::Exchange), Some(WasmType::I64), 1, memarg)
            }
            I64AtomicRmw16XchgU { memarg } => {
                (Action::Rmw(Rmw::Exchange), Some(WasmType::I64), 2, memarg)
            }
            I64AtomicRmw32XchgU { memarg } => {
                (Action::Rmw(Rmw::Exchange), Some(WasmType::I64), 4, memarg)
            }
            I32AtomicRmwCmpxchg { memarg } => (
                Action::Rmw(Rmw::CompareExchange),
                Some(WasmType::I32),
                4,
                memarg,
            ),
            I64AtomicRmwCmpxchg { memarg } => (
                Action::Rmw(Rmw::CompareExchange),
                Some(WasmType::I64),
                8,
                memarg,
            ),
            I32AtomicRmw8CmpxchgU { memarg } => (
                Action::Rmw(Rmw::CompareExchange),
                Some(WasmType::I32),
                1,
                memarg,
            ),
            I32AtomicRmw16CmpxchgU { memarg } => (
                Action::Rmw(Rmw::CompareExchange),
                Some(WasmType::I32),
                2,
                memarg,
            ),
            I64AtomicRmw8CmpxchgU { memarg } => (
                Action::Rmw(Rmw::CompareExchange),
                Some(WasmType::I64),
                1,
                memarg,
            ),
            I64AtomicRmw16CmpxchgU { memarg } => (
                Action::Rmw(Rmw::CompareExchange),
                Some(WasmType::I64),
                2,
                memarg,
            ),
            I64AtomicRmw32CmpxchgU { memarg } => (
                Action::Rmw(Rmw::CompareExchange),
                Some(WasmType::I64),
                4,
                memarg,
            ),
            _ => return None,
        };
        Some(Self {
            action,
            value_type,
            access_width,
            memory_index: memarg.memory,
            offset: memarg.offset,
        })
    }
}
