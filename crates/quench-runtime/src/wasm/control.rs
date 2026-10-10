//! Structured labels resolve to the shared residual's jump targets. This is
//! lowering state, not an executable Wasm IR or a second interpreter stack.

use super::*;
use wasmparser::{BlockType, Catch, TryTable};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Reachability {
    Live,
    Dead,
}

enum Kind {
    Function,
    Block,
    Loop { head: usize },
    If(IfArm),
    Try(TryRegion),
    LegacyTry(LegacyTry),
}

struct TryRegion {
    start: usize,
    slot: u16,
    catches: Vec<Catch>,
}

struct LegacyTry {
    slot: u16,
    arm: LegacyArm,
}

enum LegacyArm {
    Body { start: usize },
    Catch { next: Option<usize> },
}

fn catch_parts(catch: Catch) -> (Option<u32>, u32, bool) {
    match catch {
        Catch::One { tag, label } => (Some(tag), label, false),
        Catch::OneRef { tag, label } => (Some(tag), label, true),
        Catch::All { label } => (None, label, false),
        Catch::AllRef { label } => (None, label, true),
    }
}

enum IfArm {
    Then { false_jump: Option<usize> },
    Else,
}

/// An i32 branch condition: a register, or a comparison fused into the jump.
#[derive(Clone, Copy)]
pub(super) enum Condition {
    Value(Register),
    Comparison(I32BinaryOperator, Register, Register),
    /// A comparison with a constant that fits the jump's signed field.
    ConstantComparison(I32BinaryOperator, Register, i16),
    Zero(Register),
}

/// A branch label resolved against the current operand stack.
#[derive(Clone, Copy)]
struct BranchTarget {
    index: usize,
    base: Register,
    arity: Register,
    head: Option<usize>,
    values: Register,
}

pub(super) struct Control {
    kind: Kind,
    base: Register,
    params: Register,
    results: Register,
    saved_inputs: Option<Register>,
    delegation: Option<Delegation>,
    entered: Reachability,
    exits: Vec<usize>,
}

// Forward references owned by the lexical destination, discarded after lowering.
// The original exception is written directly into this destination's frame slot.
struct Delegation {
    slot: u16,
    jumps: Vec<usize>,
}

impl Control {
    /// The function's operand stack starts above its prologue binding registers.
    pub(super) fn function(base: Register, results: Register) -> Self {
        Self {
            kind: Kind::Function,
            base,
            params: 0,
            results,
            saved_inputs: None,
            delegation: None,
            entered: Reachability::Live,
            exits: Vec::new(),
        }
    }
}

impl Lowering<'_> {
    pub(super) fn control_operator(&mut self, operator: &Operator<'_>) -> Result<bool, Diagnostic> {
        match operator {
            Operator::TryTable { try_table } => self.begin_try(try_table)?,
            Operator::Try { blockty } => {
                let slot = self.exception_slot()?;
                self.begin(
                    Kind::LegacyTry(LegacyTry {
                        slot,
                        arm: LegacyArm::Body {
                            start: self.code.len(),
                        },
                    }),
                    *blockty,
                )?;
            }
            Operator::Catch { tag_index } => self.legacy_catch(Some(*tag_index))?,
            Operator::CatchAll => self.legacy_catch(None)?,
            Operator::Rethrow { relative_depth } => self.legacy_rethrow(*relative_depth)?,
            Operator::Delegate { relative_depth } => self.legacy_delegate(*relative_depth)?,
            Operator::Throw { tag_index } => self.throw_tag(*tag_index)?,
            Operator::ThrowRef => {
                if self.path == Reachability::Live {
                    let exception = self.pop()?;
                    self.emit(Op::WasmThrowRef, exception, 0, 0, 0)?;
                    self.make_dead();
                }
            }
            Operator::Block { blockty } => self.begin(Kind::Block, *blockty)?,
            Operator::Loop { blockty } => self.begin(
                Kind::Loop {
                    head: self.code.len(),
                },
                *blockty,
            )?,
            Operator::If { blockty } => {
                let condition = if self.path == Reachability::Live {
                    Some(self.pop_condition()?)
                } else {
                    None
                };
                self.materialize_aliases()?;
                self.begin(Kind::If(IfArm::Then { false_jump: None }), *blockty)?;
                let false_jump = condition
                    .map(|condition| self.jump_on(condition, false))
                    .transpose()?;
                self.controls.last_mut().unwrap().kind = Kind::If(IfArm::Then { false_jump });
            }
            Operator::Else => self.else_arm()?,
            Operator::End => self.end()?,
            Operator::Br { relative_depth } => {
                if self.path == Reachability::Live {
                    self.branch(*relative_depth)?;
                    self.make_dead();
                }
            }
            Operator::BrIf { relative_depth } => {
                if self.path == Reachability::Live {
                    let condition = self.pop_condition()?;
                    self.materialize_aliases()?;
                    self.branch_if(*relative_depth, condition)?;
                }
            }
            Operator::BrOnNull { relative_depth } | Operator::BrOnNonNull { relative_depth } => {
                if self.path == Reachability::Live {
                    let reference = self.pop()?;
                    let without_reference = self.depth;
                    // Reserve the reference while allocating the null-test scratch slot.
                    self.push()?;
                    let condition = self.push()?;
                    self.depth = without_reference;
                    self.emit(Op::WasmRefIsNull, condition, reference, 0, 0)?;
                    let carry_reference = matches!(operator, Operator::BrOnNonNull { .. });
                    if carry_reference {
                        self.emit(
                            Op::WasmI32Unary,
                            condition,
                            condition,
                            0,
                            I32UnaryOperator::EqualZero as u32,
                        )?;
                        self.push()?;
                    }
                    self.branch_if(*relative_depth, Condition::Value(condition))?;
                    self.depth = without_reference;
                    if !carry_reference {
                        self.push()?;
                    }
                }
            }
            Operator::BrTable { targets } => {
                if self.path == Reachability::Live {
                    let index = self.pop()?;
                    self.materialize_aliases()?;
                    self.branch_table(index, &targets)?;
                    self.make_dead();
                }
            }
            Operator::Return => {
                if self.path == Reachability::Live {
                    self.branch((self.controls.len() - 1) as u32)?;
                    self.make_dead();
                }
            }
            Operator::Unreachable => {
                if self.path == Reachability::Live {
                    self.emit(Op::WasmUnreachable, 0, 0, 0, 0)?;
                    self.make_dead();
                }
            }
            Operator::Select | Operator::TypedSelect { .. } => {
                if self.path == Reachability::Live {
                    let condition = self.pop()?;
                    let right = self.pop()?;
                    let left = self.pop()?;
                    let result = self.push()?;
                    self.emit(
                        Op::WasmSelect,
                        result,
                        left,
                        right,
                        crate::bytecode::ImmediateLayout::register_pair_immediate(
                            condition, condition,
                        ),
                    )?;
                    self.produced(result);
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn tag_params(&self, index: u32) -> Result<u16, Diagnostic> {
        let tag = self
            .tags
            .get(index as usize)
            .ok_or_else(|| self.control_error("Wasm tag index out of bounds"))?;
        let ty = self
            .signatures
            .declarations
            .function(self.name, tag.ty as usize)?;
        u16::try_from(ty.params().len())
            .map_err(|_| self.control_error("too many Wasm exception values"))
    }

    fn tag_binding(&mut self, index: u32) -> Result<Register, Diagnostic> {
        self.tag_params(index)?;
        self.instance_binding(
            self.globals.len()
                + self.memories.len()
                + self.data.len()
                + self.tables.len()
                + self.elements.len()
                + index as usize,
        )
    }

    fn begin_try(&mut self, table: &TryTable) -> Result<(), Diagnostic> {
        for &catch in &table.catches {
            let (tag, label, reference) = catch_parts(catch);
            let params = tag
                .map(|tag| self.tag_params(tag))
                .transpose()?
                .unwrap_or(0);
            let arity = params
                .checked_add(u16::from(reference))
                .ok_or_else(|| self.control_error("too many Wasm catch values"))?;
            let target = self
                .controls
                .len()
                .checked_sub(label as usize + 1)
                .and_then(|index| self.controls.get(index))
                .ok_or_else(|| self.control_error("Wasm catch label out of bounds"))?;
            let expected = match target.kind {
                Kind::Loop { .. } => target.params,
                _ => target.results,
            };
            if expected != arity {
                return Err(self.control_error("Wasm catch label arity mismatch"));
            }
        }
        if table.catches.is_empty() || self.path == Reachability::Dead {
            return self.begin(Kind::Block, table.ty);
        }
        let slot = self.exception_slot()?;
        self.begin(
            Kind::Try(TryRegion {
                start: self.code.len(),
                slot,
                catches: table.catches.clone(),
            }),
            table.ty,
        )
    }

    fn exception_slot(&mut self) -> Result<u16, Diagnostic> {
        let slot = self
            .temporary_base
            .checked_add(self.temporary_locals)
            .ok_or_else(|| self.control_error("too many Wasm exception locals"))?;
        self.temporary_locals = self
            .temporary_locals
            .checked_add(1)
            .filter(|temporary| self.temporary_base.checked_add(*temporary).is_some())
            .ok_or_else(|| self.control_error("too many Wasm exception locals"))?;
        Ok(slot)
    }

    fn legacy_catch(&mut self, tag: Option<u32>) -> Result<(), Diagnostic> {
        let control = self
            .controls
            .last()
            .ok_or_else(|| self.control_error("catch outside Wasm try"))?;
        let Kind::LegacyTry(region) = &control.kind else {
            return Err(self.control_error("catch outside Wasm try"));
        };
        self.check_result(control)?;
        let (base, slot, entered) = (control.base, region.slot, control.entered);
        let (body, next) = match region.arm {
            LegacyArm::Body { start } => (Some(start), None),
            LegacyArm::Catch { next: Some(next) } => (None, Some(next)),
            LegacyArm::Catch { next: None } => {
                return Err(self.control_error("Wasm catch after catch_all"));
            }
        };
        self.finish_delegation(self.controls.len() - 1)?;
        let end = self.code.len();
        if self.path == Reachability::Live {
            let exit = self.jump(Op::Jump, 0)?;
            self.controls.last_mut().unwrap().exits.push(exit);
        }
        let target = self.code.len();
        if let Some(next) = next {
            self.patch_jump(next, target)?;
        }
        if let Some(start) = body {
            self.protect_exception_region(start, end, target, slot)?;
        }
        let next = self.catch_values(slot, base, tag, false)?;
        self.controls.last_mut().unwrap().kind = Kind::LegacyTry(LegacyTry {
            slot,
            arm: LegacyArm::Catch { next },
        });
        self.path = entered;
        Ok(())
    }

    fn legacy_rethrow(&mut self, depth: u32) -> Result<(), Diagnostic> {
        let control = self
            .controls
            .len()
            .checked_sub(depth as usize + 1)
            .and_then(|index| self.controls.get(index))
            .ok_or_else(|| self.control_error("Wasm rethrow label out of bounds"))?;
        let Kind::LegacyTry(LegacyTry {
            slot,
            arm: LegacyArm::Catch { .. },
        }) = control.kind
        else {
            return Err(self.control_error("Wasm rethrow label is not a catch"));
        };
        if self.path == Reachability::Live {
            let exception = self.push()?;
            self.emit(Op::LoadLocal, exception, 0, 0, u32::from(slot))?;
            self.emit(Op::WasmThrowRef, exception, 0, 0, 0)?;
            self.make_dead();
        }
        Ok(())
    }

    fn legacy_delegate(&mut self, depth: u32) -> Result<(), Diagnostic> {
        let current = self
            .controls
            .len()
            .checked_sub(1)
            .ok_or_else(|| self.control_error("delegate outside Wasm try"))?;
        let destination = current
            .checked_sub(depth as usize + 1)
            .ok_or_else(|| self.control_error("Wasm delegate label out of bounds"))?;
        let Kind::LegacyTry(LegacyTry {
            arm: LegacyArm::Body { start },
            ..
        }) = self.controls[current].kind
        else {
            return Err(self.control_error("delegate outside Wasm try body"));
        };
        self.check_result(&self.controls[current])?;
        // Delegations targeting this try must remain inside its outgoing region.
        self.finish_delegation(current)?;
        let end = self.code.len();
        if start != end {
            let slot = match &self.controls[destination].delegation {
                Some(delegation) => delegation.slot,
                None => {
                    let slot = self.exception_slot()?;
                    self.controls[destination].delegation = Some(Delegation {
                        slot,
                        jumps: vec![],
                    });
                    slot
                }
            };
            if self.path == Reachability::Live {
                let exit = self.jump(Op::Jump, 0)?;
                self.controls[current].exits.push(exit);
            }
            let target = self.code.len();
            let jump = self.jump(Op::Jump, 0)?;
            self.controls[destination]
                .delegation
                .as_mut()
                .unwrap()
                .jumps
                .push(jump);
            self.protect_exception_region(start, end, target, slot)?;
        }
        self.end()
    }

    fn finish_delegation(&mut self, destination: usize) -> Result<(), Diagnostic> {
        let (slot, jumps) = match self.controls[destination].delegation.as_mut() {
            Some(delegation) if !delegation.jumps.is_empty() => {
                (delegation.slot, std::mem::take(&mut delegation.jumps))
            }
            _ => return Ok(()),
        };
        // Normal completion bypasses the exceptional continuation. Placement at
        // the destination boundary skips closed inner handlers, while preserving
        // its own enclosing handler (or propagating outside a catch/function).
        if self.path == Reachability::Live {
            let exit = self.jump(Op::Jump, 0)?;
            self.controls[destination].exits.push(exit);
        }
        let target = self.code.len();
        for jump in jumps {
            self.patch_jump(jump, target)?;
        }
        let depth = self.depth;
        let exception = self.push()?;
        self.emit(Op::LoadLocal, exception, 0, 0, u32::from(slot))?;
        self.emit(Op::WasmThrowRef, exception, 0, 0, 0)?;
        self.depth = depth;
        Ok(())
    }

    fn finish_legacy_try(&mut self, region: &LegacyTry, base: Register) -> Result<(), Diagnostic> {
        let LegacyArm::Catch { next: Some(next) } = region.arm else {
            return Ok(());
        };
        let normal = if self.path == Reachability::Live {
            Some(self.jump(Op::Jump, 0)?)
        } else {
            None
        };
        self.patch_jump(next, self.code.len())?;
        self.depth = base;
        let exception = self.push()?;
        self.emit(Op::LoadLocal, exception, 0, 0, u32::from(region.slot))?;
        self.emit(Op::WasmThrowRef, exception, 0, 0, 0)?;
        if let Some(normal) = normal {
            self.patch_jump(normal, self.code.len())?;
        }
        Ok(())
    }

    fn throw_tag(&mut self, index: u32) -> Result<(), Diagnostic> {
        let params = self.tag_params(index)?;
        if self.path == Reachability::Dead {
            return Ok(());
        }
        let base = self
            .depth
            .checked_sub(params)
            .filter(|base| *base >= self.control_base())
            .ok_or_else(|| self.control_error("missing Wasm exception values"))?;
        let count = params
            .checked_add(super::tag::ExceptionInput::MIN_COUNT)
            .ok_or_else(|| self.control_error("too many Wasm exception values"))?;
        self.push()?;
        // Shift upward in reverse before inserting the tag at the window's head.
        for offset in (0..params).rev() {
            self.emit(
                Op::Move,
                base + offset + super::tag::ExceptionInput::Payload as u16,
                base + offset,
                0,
                0,
            )?;
        }
        let tag = self.tag_binding(index)?;
        self.emit(
            Op::Move,
            base + super::tag::ExceptionInput::Tag as u16,
            tag,
            0,
            0,
        )?;
        self.emit(Op::WasmExceptionNew, base, base, count, 0)?;
        self.emit(Op::WasmThrowRef, base, 0, 0, 0)?;
        self.make_dead();
        Ok(())
    }

    fn finish_try(&mut self, region: &TryRegion, base: Register) -> Result<(), Diagnostic> {
        let end = self.code.len();
        if region.start == end {
            return Ok(());
        }
        let normal = if self.path == Reachability::Live {
            Some(self.jump(Op::Jump, 0)?)
        } else {
            None
        };
        let target = self.code.len();
        for &catch in &region.catches {
            let (tag, label, reference) = catch_parts(catch);
            let next = self.catch_values(region.slot, base, tag, reference)?;
            self.branch(label)?;
            if let Some(next) = next {
                self.patch_jump(next, self.code.len())?;
            }
        }
        self.depth = base;
        let exception = self.push()?;
        self.emit(Op::LoadLocal, exception, 0, 0, u32::from(region.slot))?;
        self.emit(Op::WasmThrowRef, exception, 0, 0, 0)?;
        if let Some(normal) = normal {
            self.patch_jump(normal, self.code.len())?;
        }
        self.protect_exception_region(region.start, end, target, region.slot)
    }

    fn catch_values(
        &mut self,
        slot: u16,
        base: Register,
        tag: Option<u32>,
        reference: bool,
    ) -> Result<Option<usize>, Diagnostic> {
        let params = tag
            .map(|tag| self.tag_params(tag))
            .transpose()?
            .unwrap_or(0);
        let count = params
            .checked_add(u16::from(reference))
            .ok_or_else(|| self.control_error("too many Wasm catch values"))?;
        self.depth = base
            .checked_add(count)
            .ok_or_else(|| self.control_error("too many Wasm catch registers"))?;
        let exception = self.push()?;
        self.emit(Op::LoadLocal, exception, 0, 0, u32::from(slot))?;
        let next = if let Some(tag) = tag {
            let tag = self.tag_binding(tag)?;
            let condition = self.push()?;
            self.emit(Op::WasmExceptionMatch, condition, exception, tag, 0)?;
            Some(self.jump_on(Condition::Value(condition), false)?)
        } else {
            None
        };
        for offset in 0..params {
            self.emit(
                Op::WasmExceptionPayload,
                base + offset,
                exception,
                0,
                u32::from(offset),
            )?;
        }
        if reference {
            self.emit(Op::Move, base + params, exception, 0, 0)?;
        }
        self.depth = base + count;
        Ok(next)
    }

    fn protect_exception_region(
        &mut self,
        start: usize,
        end: usize,
        target: usize,
        slot: u16,
    ) -> Result<(), Diagnostic> {
        if start == end {
            return Ok(());
        }
        self.handlers.push(crate::bytecode::Handler {
            start: u32::try_from(start)
                .map_err(|_| self.control_error("Wasm handler start out of bounds"))?,
            end: u32::try_from(end)
                .map_err(|_| self.control_error("Wasm handler end out of bounds"))?,
            target: u32::try_from(target)
                .map_err(|_| self.control_error("Wasm handler target out of bounds"))?,
            slot: Some(slot),
            return_target: None,
            return_slot: None,
            with_depth: 0,
        });
        Ok(())
    }

    fn control_error(&self, message: &str) -> Diagnostic {
        Diagnostic::unsupported(self.name, message)
    }

    fn begin(&mut self, kind: Kind, block_type: BlockType) -> Result<(), Diagnostic> {
        let (params, results) = match block_type {
            BlockType::Empty => (0, 0),
            BlockType::Type(_) => (0, 1),
            BlockType::FuncType(index) => {
                let signature = self
                    .signatures
                    .declarations
                    .function_shape(self.name, index as usize)?;
                let count = |n| {
                    u16::try_from(n)
                        .map_err(|_| self.control_error("Wasm block arity exceeds register layout"))
                };
                (
                    count(signature.params().len())?,
                    count(signature.results().len())?,
                )
            }
        };
        let base = if self.path == Reachability::Live {
            self.depth
                .checked_sub(params)
                .filter(|base| *base >= self.control_base())
                .ok_or_else(|| self.control_error("missing Wasm block parameters"))?
        } else {
            self.depth
        };
        let input_end = base
            .checked_add(params)
            .ok_or_else(|| self.control_error("Wasm block arity exceeds register layout"))?;
        let result_end = base
            .checked_add(results)
            .ok_or_else(|| self.control_error("Wasm block arity exceeds register layout"))?;
        if input_end.max(result_end) > crate::bytecode::REGISTER_MASK + 1 {
            return Err(self.control_error("Wasm block arity exceeds register layout"));
        }
        // Then and else consume the same input values, even when then rewrites
        // their operand registers. Hidden frame locals preserve those inputs.
        let saved_inputs =
            if matches!(kind, Kind::If(_)) && params != 0 && self.path == Reachability::Live {
                let first = self.temporary_base + self.temporary_locals;
                first
                    .checked_add(params)
                    .ok_or_else(|| self.control_error("too many Wasm locals"))?;
                self.temporary_locals += params;
                for offset in 0..params {
                    self.emit(
                        Op::StoreLocal,
                        base + offset,
                        0,
                        0,
                        u32::from(first + offset),
                    )?;
                }
                Some(first)
            } else {
                None
            };
        self.depth = input_end;
        self.registers = self.registers.max(input_end.max(result_end));
        self.controls.push(Control {
            kind,
            base,
            params,
            results,
            saved_inputs,
            delegation: None,
            entered: self.path,
            exits: Vec::new(),
        });
        Ok(())
    }

    fn else_arm(&mut self) -> Result<(), Diagnostic> {
        let control = self
            .controls
            .last()
            .ok_or_else(|| self.control_error("else outside Wasm if"))?;
        let Kind::If(IfArm::Then { false_jump }) = control.kind else {
            return Err(self.control_error("else outside Wasm if"));
        };
        self.check_result(control)?;
        if self.path == Reachability::Live {
            let end = self.jump(Op::Jump, 0)?;
            self.controls.last_mut().unwrap().exits.push(end);
        }
        if let Some(false_jump) = false_jump {
            self.patch_jump(false_jump, self.code.len())?;
        }
        let control = self.controls.last_mut().unwrap();
        control.kind = Kind::If(IfArm::Else);
        self.depth = control.base + control.params;
        self.path = control.entered;
        let (base, params, saved_inputs) = (control.base, control.params, control.saved_inputs);
        if let Some(first) = saved_inputs {
            for offset in 0..params {
                self.emit(
                    Op::LoadLocal,
                    base + offset,
                    0,
                    0,
                    u32::from(first + offset),
                )?;
            }
        }
        Ok(())
    }

    fn check_result(&self, control: &Control) -> Result<(), Diagnostic> {
        if self.path == Reachability::Live && self.depth != control.base + control.results {
            return Err(self.control_error("invalid Wasm result stack"));
        }
        Ok(())
    }

    fn end(&mut self) -> Result<(), Diagnostic> {
        let current = self
            .controls
            .len()
            .checked_sub(1)
            .ok_or_else(|| self.control_error("extra Wasm end"))?;
        self.check_result(&self.controls[current])?;
        self.finish_delegation(current)?;
        let control = self
            .controls
            .pop()
            .ok_or_else(|| self.control_error("extra Wasm end"))?;
        let mut live = self.path == Reachability::Live || !control.exits.is_empty();
        if let Kind::If(IfArm::Then { false_jump }) = control.kind {
            if control.params != control.results {
                return Err(self.control_error("Wasm if without else requires matching arities"));
            }
            if let Some(false_jump) = false_jump {
                self.patch_jump(false_jump, self.code.len())?;
                live = true;
            }
        }
        if let Kind::Try(ref region) = control.kind {
            self.finish_try(region, control.base)?;
        }
        if let Kind::LegacyTry(ref region) = control.kind {
            self.finish_legacy_try(region, control.base)?;
        }
        for exit in control.exits {
            self.patch_jump(exit, self.code.len())?;
        }
        self.depth = control.base + control.results;
        self.registers = self.registers.max(self.depth);
        self.path = if live {
            Reachability::Live
        } else {
            Reachability::Dead
        };
        if matches!(control.kind, Kind::Function) {
            if control.results == 0 {
                self.emit(Op::LoadConst, control.base, 0, 0, VOID_RESULT_CONSTANT)?;
            }
            let result = if usize::from(control.results) > SCALAR_RETURN_ARITY {
                let bundle = self.push()?;
                self.emit(Op::MakeArray, bundle, 0, 0, u32::from(control.results))?;
                for offset in 0..control.results {
                    self.emit(
                        Op::DefineArrayElement,
                        control.base + offset,
                        bundle,
                        0,
                        u32::from(offset),
                    )?;
                }
                bundle
            } else {
                control.base
            };
            self.emit(Op::Return, result, 0, 0, 0)?;
        }
        Ok(())
    }

    pub(super) fn branch_if(
        &mut self,
        relative_depth: u32,
        condition: Condition,
    ) -> Result<(), Diagnostic> {
        let target = self.branch_target(relative_depth)?;
        if target.base == target.values || target.arity == 0 {
            let jump = self.jump_on(condition, true)?;
            return self.bind_branch(target, jump);
        }
        let fallthrough = self.jump_on(condition, false)?;
        self.branch(relative_depth)?;
        self.patch_jump(fallthrough, self.code.len())
    }

    /// Pop an i32 branch condition. A comparison that the previous operator
    /// emitted only for this condition is withdrawn and becomes the jump.
    /// Its operands stay intact: they sit in locals or popped positions,
    /// which later materialization never writes.
    pub(super) fn pop_condition(&mut self) -> Result<Condition, Diagnostic> {
        let condition = self.pop()?;
        let Some(pc) = self.previous_producer(condition) else {
            return Ok(Condition::Value(condition));
        };
        let instruction = self.instruction(pc);
        if let Some(comparison) = I32BinaryOperator::from_register_op(instruction.op())
            && comparison.jump_op().is_some()
        {
            self.code.pop();
            return Ok(Condition::Comparison(
                comparison,
                instruction.b(),
                instruction.c(),
            ));
        }
        if let Some(comparison) = I32BinaryOperator::from_immediate_op(instruction.op())
            && comparison.immediate_jump_op().is_some()
            && let Ok(constant) = i16::try_from(instruction.imm() as i32)
        {
            self.code.pop();
            return Ok(Condition::ConstantComparison(
                comparison,
                instruction.b(),
                constant,
            ));
        }
        if instruction.op() == Op::WasmI32Unary
            && I32UnaryOperator::from_tag(instruction.imm()) == Some(I32UnaryOperator::EqualZero)
        {
            self.code.pop();
            return Ok(Condition::Zero(instruction.b()));
        }
        Ok(Condition::Value(condition))
    }

    /// Emit a jump taken when `condition` holds (`when`) or fails.
    pub(super) fn jump_on(
        &mut self,
        condition: Condition,
        when: bool,
    ) -> Result<usize, Diagnostic> {
        let pc = self.code.len();
        match condition {
            Condition::Comparison(comparison, left, right) => {
                let comparison = if when {
                    comparison
                } else {
                    comparison
                        .negated_comparison()
                        .expect("fusable comparison has a negation")
                };
                let op = comparison.jump_op().expect("fusable comparison has a jump");
                self.emit(op, left, right, 0, 0)?;
            }
            Condition::ConstantComparison(comparison, left, constant) => {
                let comparison = if when {
                    comparison
                } else {
                    comparison
                        .negated_comparison()
                        .expect("fusable comparison has a negation")
                };
                let op = comparison
                    .immediate_jump_op()
                    .expect("fusable comparison has a constant jump");
                self.emit(op, left, constant as u16, 0, 0)?;
            }
            Condition::Zero(value) => {
                let op = if when {
                    Op::WasmJumpI32Zero
                } else {
                    Op::WasmJumpI32NonZero
                };
                self.emit(op, value, 0, 0, 0)?;
            }
            Condition::Value(value) => {
                let op = if when {
                    Op::WasmJumpI32NonZero
                } else {
                    Op::WasmJumpI32Zero
                };
                self.emit(op, value, 0, 0, 0)?;
            }
        }
        Ok(pc)
    }

    fn branch_target(&self, relative_depth: u32) -> Result<BranchTarget, Diagnostic> {
        let distance = usize::try_from(relative_depth)
            .ok()
            .and_then(|depth| depth.checked_add(1))
            .ok_or_else(|| self.control_error("Wasm branch label out of bounds"))?;
        let index = self
            .controls
            .len()
            .checked_sub(distance)
            .ok_or_else(|| self.control_error("Wasm branch label out of bounds"))?;
        let control = &self.controls[index];
        let (base, arity, head) = match control.kind {
            Kind::Loop { head } => (control.base, control.params, Some(head)),
            _ => (control.base, control.results, None),
        };
        let values = self
            .depth
            .checked_sub(arity)
            .filter(|values| *values >= self.control_base())
            .ok_or_else(|| self.control_error("missing Wasm branch values"))?;
        Ok(BranchTarget {
            index,
            base,
            arity,
            head,
            values,
        })
    }

    /// Point a jump at its label: a loop head now, a block end when it closes.
    fn bind_branch(&mut self, target: BranchTarget, jump: usize) -> Result<(), Diagnostic> {
        if let Some(head) = target.head {
            self.patch_jump(jump, head)
        } else {
            self.controls[target.index].exits.push(jump);
            Ok(())
        }
    }

    /// One `Jump` entry per case and a final default entry follow the table.
    /// An entry whose label needs value moves jumps to a pad after the
    /// entries that moves them and branches.
    fn branch_table(
        &mut self,
        index: Register,
        targets: &wasmparser::BrTable<'_>,
    ) -> Result<(), Diagnostic> {
        let labels = targets
            .targets()
            .chain(std::iter::once(Ok(targets.default())))
            .collect::<Result<Vec<u32>, _>>()
            .map_err(|e| Diagnostic::unsupported(self.name, e.to_string()))?;
        let cases = u32::try_from(labels.len() - 1)
            .map_err(|_| self.control_error("Wasm branch table too large"))?;
        self.emit(Op::WasmBranchTable, index, 0, 0, cases)?;
        let entries = labels
            .iter()
            .map(|_| self.jump(Op::Jump, 0))
            .collect::<Result<Vec<usize>, _>>()?;
        for (entry, label) in entries.into_iter().zip(labels) {
            let target = self.branch_target(label)?;
            if target.base == target.values || target.arity == 0 {
                self.bind_branch(target, entry)?;
            } else {
                self.patch_jump(entry, self.code.len())?;
                self.branch(label)?;
            }
        }
        Ok(())
    }

    fn branch(&mut self, relative_depth: u32) -> Result<(), Diagnostic> {
        let target = self.branch_target(relative_depth)?;
        // Destinations are below sources, so ascending moves preserve overlap.
        for offset in 0..target.arity {
            if target.base != target.values {
                self.emit(Op::Move, target.base + offset, target.values + offset, 0, 0)?;
            }
        }
        let jump = self.jump(Op::Jump, 0)?;
        self.bind_branch(target, jump)
    }

    pub(super) fn control_base(&self) -> Register {
        self.controls.last().map_or(0, |control| control.base)
    }

    pub(super) fn make_dead(&mut self) {
        self.path = Reachability::Dead;
        self.depth = self.control_base();
    }

    fn jump(&mut self, op: Op, condition: Register) -> Result<usize, Diagnostic> {
        let pc = self.code.len();
        self.emit(op, condition, 0, 0, 0)?;
        Ok(pc)
    }

    fn patch_jump(&mut self, pc: usize, target: usize) -> Result<(), Diagnostic> {
        let target =
            u32::try_from(target).map_err(|_| self.control_error("Wasm jump target too large"))?;
        if self.code[pc].is_wide() {
            let index = self.code[pc].wide_index();
            self.wide[index].set_jump_target(target);
        } else if !self.code[pc].try_set_jump_target(target) {
            let mut instruction = self.code[pc].as_wide();
            instruction.set_jump_target(target);
            let marker = Instr::wide(self.wide.len())
                .ok_or_else(|| self.control_error("Wasm residual too large"))?;
            self.wide.push(instruction);
            self.code[pc] = marker;
        }
        Ok(())
    }
}
