//! Shared top-level facts for functions containing one ordinary counted loop.
//!
//! This is a disposable view of canonical residual operations. It owns no
//! JavaScript semantics and deliberately leaves each loop-body cover explicit.

use crate::{ir::Opcode, machine::CodeView};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
pub(crate) struct InitialNumber {
    pub(crate) slot: u16,
    pub(crate) value: f64,
}

#[derive(Debug)]
pub(crate) struct CountedFunctionFacts<T> {
    pub(crate) initials: Vec<InitialNumber>,
    pub(crate) loop_cover: T,
    pub(crate) returned: ReturnedLocal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ReturnedLocal {
    pub(crate) slot: u16,
    pub(crate) representation: ReturnedRepresentation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReturnedRepresentation {
    Direct,
    U32,
}

pub(crate) fn select<T>(
    code: CodeView<'_>,
    cover: impl FnOnce(crate::stencil_counted_loop::CountedLoop, CodeView<'_>, &[u16]) -> Option<T>,
) -> Option<CountedFunctionFacts<T>> {
    let mut state = FunctionState::new(cover);
    for pc in 0..code.len() {
        let instruction = code.instruction(pc)?;
        state.push(code, pc, instruction)?;
    }
    state.finish()
}

struct FunctionState<F, T> {
    constants: BTreeMap<u16, f64>,
    locals: BTreeMap<u16, ReturnedLocal>,
    initials: Vec<InitialNumber>,
    cover: Option<F>,
    loop_cover: Option<T>,
    returned: Option<ReturnedLocal>,
}

impl<F, T> FunctionState<F, T>
where
    F: FnOnce(crate::stencil_counted_loop::CountedLoop, CodeView<'_>, &[u16]) -> Option<T>,
{
    fn new(cover: F) -> Self {
        Self {
            constants: BTreeMap::new(),
            locals: BTreeMap::new(),
            initials: Vec::new(),
            cover: Some(cover),
            loop_cover: None,
            returned: None,
        }
    }

    fn push(
        &mut self,
        code: CodeView<'_>,
        pc: usize,
        instruction: crate::ir::Instruction,
    ) -> Option<()> {
        match instruction.opcode {
            Opcode::LoadConst => self.load_constant(code, instruction),
            Opcode::InitLocal => self.initialize(instruction),
            Opcode::LoadLocal | Opcode::LoadLocalChecked => {
                self.locals.insert(
                    instruction.a,
                    ReturnedLocal {
                        slot: instruction.b,
                        representation: ReturnedRepresentation::Direct,
                    },
                );
                Some(())
            }
            Opcode::Binary => self.binary(instruction),
            Opcode::Return => {
                if let Some(returned) = self.locals.get(&instruction.a) {
                    self.returned = Some(*returned);
                }
                Some(())
            }
            Opcode::Slow => self.slow(code, pc),
            _ => None,
        }
    }

    fn binary(&mut self, instruction: crate::ir::Instruction) -> Option<()> {
        let operator = crate::ir::compact_binary_operator(instruction.flags)?;
        (operator == crate::ops::BinaryOp::ShiftRightZeroFill).then_some(())?;
        let source = self.locals.get(&instruction.b).copied()?;
        let shift = self.constants.get(&instruction.c)?;
        (shift.to_bits() == 0.0f64.to_bits()).then_some(())?;
        self.locals.insert(
            instruction.a,
            ReturnedLocal {
                slot: source.slot,
                representation: ReturnedRepresentation::U32,
            },
        );
        Some(())
    }

    fn load_constant(
        &mut self,
        code: CodeView<'_>,
        instruction: crate::ir::Instruction,
    ) -> Option<()> {
        match code.constant(instruction.b)? {
            crate::ops::Constant::Number(value) => {
                self.constants.insert(instruction.a, *value);
            }
            crate::ops::Constant::Undefined => {
                self.constants.remove(&instruction.a);
            }
            _ => return None,
        }
        Some(())
    }

    fn initialize(&mut self, instruction: crate::ir::Instruction) -> Option<()> {
        let value = *self.constants.get(&instruction.b)?;
        self.initials.push(InitialNumber {
            slot: instruction.a,
            value,
        });
        Some(())
    }

    fn slow(&mut self, code: CodeView<'_>, pc: usize) -> Option<()> {
        match code.cold_at(pc)? {
            crate::ops::Op::MarkUninitialized { .. } | crate::ops::Op::MarkImmutable { .. } => {
                Some(())
            }
            crate::ops::Op::Loop {
                label: None,
                init,
                test,
                body,
                update,
                post_test: false,
                per_iteration,
                ..
            } if self.loop_cover.is_none() => {
                let counted = crate::stencil_counted_loop::select(
                    init.code()?,
                    test.code()?,
                    update.code()?,
                )?;
                let cover = self.cover.take()?(counted, body.code()?, per_iteration)?;
                self.loop_cover = Some(cover);
                Some(())
            }
            _ => None,
        }
    }

    fn finish(self) -> Option<CountedFunctionFacts<T>> {
        Some(CountedFunctionFacts {
            initials: self.initials,
            loop_cover: self.loop_cover?,
            returned: self.returned?,
        })
    }
}

pub(crate) fn initial_f64(initials: &[InitialNumber], slot: u16) -> Option<f64> {
    initials
        .iter()
        .find_map(|initial| (initial.slot == slot).then_some(initial.value))
}

pub(crate) fn initial_i64(initials: &[InitialNumber], slot: u16) -> Option<i64> {
    let value = initial_f64(initials, slot)?;
    (value.is_finite()
        && value.fract() == 0.0
        && value >= i64::MIN as f64
        && value <= i64::MAX as f64)
        .then_some(value as i64)
}
