//! Structured labels resolve to the shared residual's jump targets. This is
//! lowering state, not an executable Wasm IR or a second interpreter stack.

use super::*;
use wasmparser::BlockType;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Reachability {
    Live,
    Dead,
}

enum Kind {
    Function,
    Block,
    Loop {
        head: usize,
    },
    If(IfArm),
    TryTable {
        catches: Vec<CatchInfo>,
        start: u32,
    },
    LegacyTry {
        start: u32,
        end: Option<u32>,
        catch_register: Option<Register>,
    },
}

pub(super) struct DelegatedRegion {
    start: u32,
    end: u32,
    target_control_index: usize,
}

#[derive(Clone, Copy)]
struct CatchInfo {
    tag_index: Option<u32>,
    label: u32,
    catch_ref: bool,
}

enum IfArm {
    Then { false_jump: Option<usize> },
    Else,
}

pub(super) struct Control {
    kind: Kind,
    base: Register,
    param_count: u16,
    result_count: u16,
    branch_count: u16,
    entered: Reachability,
    exits: Vec<usize>,
}

impl Control {
    pub(super) fn function(result_count: u16) -> Self {
        Self {
            kind: Kind::Function,
            base: 0,
            param_count: 0,
            result_count,
            branch_count: result_count,
            entered: Reachability::Live,
            exits: Vec::new(),
        }
    }
}

impl Lowering<'_> {
    pub(super) fn control_operator(&mut self, operator: &Operator<'_>) -> Result<bool, Diagnostic> {
        match operator {
            Operator::Try { blockty } => {
                let start = u32::try_from(self.code.len())
                    .map_err(|_| self.control_error("Wasm try body is too large"))?;
                self.begin(
                    Kind::LegacyTry {
                        start,
                        end: None,
                        catch_register: None,
                    },
                    *blockty,
                )?;
            }
            Operator::Catch { tag_index } => self.legacy_catch(Some(*tag_index))?,
            Operator::CatchAll => self.legacy_catch(None)?,
            Operator::Delegate { relative_depth } => self.legacy_delegate(*relative_depth)?,
            Operator::Rethrow { relative_depth } => {
                if self.path == Reachability::Live {
                    let distance = usize::try_from(*relative_depth)
                        .ok()
                        .and_then(|depth| depth.checked_add(1))
                        .ok_or_else(|| self.control_error("invalid Wasm rethrow label"))?;
                    let target = self
                        .controls
                        .len()
                        .checked_sub(distance)
                        .ok_or_else(|| self.control_error("invalid Wasm rethrow label"))?;
                    let Some(exception) =
                        self.controls
                            .get(target)
                            .and_then(|control| match &control.kind {
                                Kind::LegacyTry {
                                    catch_register: Some(exception),
                                    ..
                                } => Some(*exception),
                                _ => None,
                            })
                    else {
                        return Err(self.control_error("invalid Wasm rethrow label"));
                    };
                    let site = self.code.len();
                    self.emit(Op::WasmThrowRef, 0, exception, 0, 0)?;
                    self.rethrow_sites.push((site, exception));
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
                let false_jump = if self.path == Reachability::Live {
                    let condition = self.pop()?;
                    Some(self.jump(Op::JumpFalse, condition)?)
                } else {
                    None
                };
                self.begin(Kind::If(IfArm::Then { false_jump }), *blockty)?;
            }
            Operator::TryTable { try_table } => {
                let catches = try_table
                    .catches
                    .iter()
                    .map(|catch| match *catch {
                        wasmparser::Catch::One { tag, label } => CatchInfo {
                            tag_index: Some(tag),
                            label,
                            catch_ref: false,
                        },
                        wasmparser::Catch::OneRef { tag, label } => CatchInfo {
                            tag_index: Some(tag),
                            label,
                            catch_ref: true,
                        },
                        wasmparser::Catch::All { label } => CatchInfo {
                            tag_index: None,
                            label,
                            catch_ref: false,
                        },
                        wasmparser::Catch::AllRef { label } => CatchInfo {
                            tag_index: None,
                            label,
                            catch_ref: true,
                        },
                    })
                    .collect();
                let start = u32::try_from(self.code.len())
                    .map_err(|_| self.control_error("Wasm try_table body is too large"))?;
                self.begin(Kind::TryTable { catches, start }, try_table.ty)?;
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
                    let condition = self.pop()?;
                    let fallthrough = self.jump(Op::JumpFalse, condition)?;
                    self.branch(*relative_depth)?;
                    self.patch_jump(fallthrough, self.code.len())?;
                }
            }
            Operator::BrOnNull { relative_depth } | Operator::BrOnNonNull { relative_depth } => {
                if self.path == Reachability::Live {
                    let branch_on_non_null = matches!(operator, Operator::BrOnNonNull { .. });
                    let reference = self.pop()?;
                    let condition = self.alloc_register()?;
                    self.emit(Op::WasmRefIsNull, condition, reference, 0, 0)?;
                    if branch_on_non_null {
                        self.emit(
                            Op::WasmI32Unary,
                            condition,
                            condition,
                            0,
                            I32UnaryOperator::EqualZero as u32,
                        )?;
                    }
                    let fallthrough = self.jump(Op::JumpFalse, condition)?;
                    if branch_on_non_null {
                        self.push()?;
                    }
                    self.branch(*relative_depth)?;
                    self.patch_jump(fallthrough, self.code.len())?;
                    if !branch_on_non_null {
                        self.push()?;
                    } else {
                        self.depth = self
                            .depth
                            .checked_sub(1)
                            .ok_or_else(|| self.control_error("Wasm operand stack underflow"))?;
                    }
                }
            }
            Operator::BrOnCast {
                relative_depth,
                to_ref_type,
                ..
            }
            | Operator::BrOnCastFail {
                relative_depth,
                to_ref_type,
                ..
            } => {
                if self.path == Reachability::Live {
                    let branch_on_cast_fail = matches!(operator, Operator::BrOnCastFail { .. });
                    let reference = self.pop()?;
                    let condition = self.alloc_register()?;
                    let type_code = reference_heap_code(to_ref_type.heap_type())?
                        | (u32::from(to_ref_type.is_nullable()) << 30);
                    self.emit(Op::WasmRefTest, condition, reference, 0, type_code)?;
                    if branch_on_cast_fail {
                        self.emit(
                            Op::WasmI32Unary,
                            condition,
                            condition,
                            0,
                            I32UnaryOperator::EqualZero as u32,
                        )?;
                    }
                    let fallthrough = self.jump(Op::JumpFalse, condition)?;
                    self.push()?;
                    self.branch(*relative_depth)?;
                    self.patch_jump(fallthrough, self.code.len())?;
                    self.depth = self
                        .depth
                        .checked_sub(1)
                        .ok_or_else(|| self.control_error("Wasm operand stack underflow"))?;
                    self.push()?;
                }
            }
            Operator::BrOnCastDescEq {
                relative_depth,
                to_ref_type,
                ..
            }
            | Operator::BrOnCastDescEqFail {
                relative_depth,
                to_ref_type,
                ..
            } => {
                if self.path == Reachability::Live {
                    let branch_on_cast_fail =
                        matches!(operator, Operator::BrOnCastDescEqFail { .. });
                    let descriptor = self.pop()?;
                    let reference = self.pop()?;
                    let condition = self.alloc_register()?;
                    let type_code = reference_heap_code(to_ref_type.heap_type())?
                        | (u32::from(to_ref_type.is_nullable()) << 30);
                    self.emit(
                        Op::WasmRefTestDescEq,
                        condition,
                        reference,
                        descriptor,
                        type_code,
                    )?;
                    if branch_on_cast_fail {
                        self.emit(
                            Op::WasmI32Unary,
                            condition,
                            condition,
                            0,
                            I32UnaryOperator::EqualZero as u32,
                        )?;
                    }
                    let fallthrough = self.jump(Op::JumpFalse, condition)?;
                    self.push()?;
                    self.branch(*relative_depth)?;
                    self.patch_jump(fallthrough, self.code.len())?;
                    self.depth = self
                        .depth
                        .checked_sub(1)
                        .ok_or_else(|| self.control_error("Wasm operand stack underflow"))?;
                    self.push()?;
                }
            }
            Operator::BrTable { targets } => {
                if self.path == Reachability::Live {
                    let index = self.pop()?;
                    let depth = self.depth;
                    self.push()?; // Keep the selector live while allocating scratch slots.
                    let constant = self.push()?;
                    let condition = self.push()?;
                    self.depth = depth;
                    for (ordinal, target) in targets.targets().enumerate() {
                        let target = target
                            .map_err(|e| Diagnostic::unsupported(self.name, e.to_string()))?;
                        let ordinal = u32::try_from(ordinal)
                            .map_err(|_| self.control_error("Wasm branch table too large"))?;
                        self.load_i32(constant, ordinal as i32)?;
                        self.emit(
                            Op::WasmI32Binary,
                            condition,
                            index,
                            constant,
                            I32BinaryOperator::Equal as u32,
                        )?;
                        let next = self.jump(Op::JumpFalse, condition)?;
                        self.branch(target)?;
                        self.patch_jump(next, self.code.len())?;
                    }
                    self.branch(targets.default())?;
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
                    debug_assert_eq!(result, left);
                    let false_jump = self.jump(Op::JumpFalse, condition)?;
                    let end = self.jump(Op::Jump, 0)?;
                    self.patch_jump(false_jump, self.code.len())?;
                    self.emit(Op::Move, result, right, 0, 0)?;
                    self.patch_jump(end, self.code.len())?;
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn control_error(&self, message: &str) -> Diagnostic {
        Diagnostic::unsupported(self.name, message)
    }

    fn legacy_catch(&mut self, tag_index: Option<u32>) -> Result<(), Diagnostic> {
        let control_index = self
            .controls
            .len()
            .checked_sub(1)
            .ok_or_else(|| self.control_error("Wasm catch outside try"))?;
        let (base, start, end) = {
            let control = &self.controls[control_index];
            let Kind::LegacyTry { start, end, .. } = &control.kind else {
                return Err(self.control_error("Wasm catch outside try"));
            };
            (control.base, *start, *end)
        };
        self.check_result(&self.controls[control_index])?;
        if self.path == Reachability::Live {
            let exit = self.jump(Op::Jump, 0)?;
            self.controls[control_index].exits.push(exit);
        }
        let try_end = end.unwrap_or(
            u32::try_from(self.code.len())
                .map_err(|_| self.control_error("Wasm try body is too large"))?,
        );
        let payload_count = tag_index
            .map(|tag| {
                self.tag_signatures
                    .get(tag as usize)
                    .map(|signature| signature.params.len())
                    .ok_or_else(|| self.control_error("Wasm catch tag out of bounds"))
            })
            .transpose()?
            .unwrap_or(0);
        let payload_count = u16::try_from(payload_count)
            .map_err(|_| self.control_error("Wasm catch payload is too large"))?;
        let exception_register = self.alloc_register()?;
        self.exception_registers.push(exception_register);
        for (range_start, range_end) in self.legacy_handler_ranges(start, try_end, control_index) {
            let target = u32::try_from(self.code.len())
                .map_err(|_| self.control_error("Wasm catch body is too large"))?;
            self.exception_handlers.push(super::WasmExceptionHandler {
                start: range_start,
                end: range_end,
                tag_index,
                target,
                payload_register: base,
                payload_count,
                catch_ref: false,
                exception_register: Some(exception_register),
            });
        }
        self.controls[control_index].kind = Kind::LegacyTry {
            start,
            end: Some(try_end),
            catch_register: Some(exception_register),
        };
        self.depth = base
            .checked_add(payload_count)
            .ok_or_else(|| self.control_error("Wasm catch stack is too large"))?;
        self.registers = self.registers.max(self.depth);
        self.path = Reachability::Live;
        Ok(())
    }

    fn legacy_delegate(&mut self, relative_depth: u32) -> Result<(), Diagnostic> {
        let control_index = self
            .controls
            .len()
            .checked_sub(1)
            .ok_or_else(|| self.control_error("Wasm delegate outside try"))?;
        let (start, has_catch) = match &self.controls[control_index].kind {
            Kind::LegacyTry {
                start,
                catch_register,
                ..
            } => (*start, catch_register.is_some()),
            _ => return Err(self.control_error("Wasm delegate outside try")),
        };
        if has_catch {
            return Err(self.control_error("Wasm delegate after catch"));
        }
        self.check_result(&self.controls[control_index])?;
        let target_control_index = control_index
            .checked_sub(1)
            .and_then(|last_outer| {
                usize::try_from(relative_depth)
                    .ok()
                    .and_then(|depth| last_outer.checked_sub(depth))
            })
            .ok_or_else(|| self.control_error("invalid Wasm delegate label"))?;
        let end = u32::try_from(self.code.len())
            .map_err(|_| self.control_error("Wasm delegate body is too large"))?;
        self.delegated_regions.push(DelegatedRegion {
            start,
            end,
            target_control_index,
        });
        self.controls[control_index].kind = Kind::LegacyTry {
            start,
            end: Some(end),
            catch_register: None,
        };
        self.end()?;
        Ok(())
    }

    fn legacy_handler_ranges(&self, start: u32, end: u32, control_index: usize) -> Vec<(u32, u32)> {
        let mut ranges = vec![(start, end)];
        for delegated in &self.delegated_regions {
            if delegated.target_control_index >= control_index
                || delegated.end <= start
                || delegated.start >= end
            {
                continue;
            }
            let mut remaining = Vec::new();
            for (range_start, range_end) in ranges {
                if delegated.end <= range_start || delegated.start >= range_end {
                    remaining.push((range_start, range_end));
                    continue;
                }
                if range_start < delegated.start {
                    remaining.push((range_start, delegated.start.min(range_end)));
                }
                if delegated.end < range_end {
                    remaining.push((delegated.end.max(range_start), range_end));
                }
            }
            ranges = remaining;
        }
        ranges
    }

    fn begin(&mut self, kind: Kind, block_type: BlockType) -> Result<(), Diagnostic> {
        let (param_count, result_count) = match block_type {
            BlockType::Empty => (0, 0),
            BlockType::Type(ty) if WasmType::from_wasm(ty).is_some() => (0, 1),
            BlockType::FuncType(type_index) => {
                let signature = self
                    .type_signatures
                    .get(type_index as usize)
                    .ok_or_else(|| self.control_error("Wasm block type index out of bounds"))?;
                (
                    u16::try_from(signature.params.len())
                        .map_err(|_| self.control_error("too many Wasm block parameters"))?,
                    u16::try_from(signature.result_count())
                        .map_err(|_| self.control_error("too many Wasm block results"))?,
                )
            }
            _ => return Err(self.control_error("unsupported Wasm block signature")),
        };
        let base = self
            .depth
            .checked_sub(param_count)
            .filter(|base| *base >= self.control_base())
            .ok_or_else(|| self.control_error("missing Wasm block parameters"))?;
        let branch_count = if matches!(&kind, Kind::Loop { .. }) {
            param_count
        } else {
            result_count
        };
        self.controls.push(Control {
            kind,
            base,
            param_count,
            result_count,
            branch_count,
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
        self.depth = control.base + control.param_count;
        self.path = control.entered;
        Ok(())
    }

    fn check_result(&self, control: &Control) -> Result<(), Diagnostic> {
        if self.path == Reachability::Live && self.depth != control.base + control.result_count {
            return Err(self.control_error("invalid Wasm result stack"));
        }
        Ok(())
    }

    fn end(&mut self) -> Result<(), Diagnostic> {
        let control = self
            .controls
            .pop()
            .ok_or_else(|| self.control_error("extra Wasm end"))?;
        self.check_result(&control)?;
        let mut live = self.path == Reachability::Live || !control.exits.is_empty();
        if let Kind::If(IfArm::Then { false_jump }) = &control.kind {
            if control.param_count != control.result_count {
                return Err(self.control_error("Wasm result if requires else"));
            }
            if let Some(false_jump) = *false_jump {
                self.patch_jump(false_jump, self.code.len())?;
                live = true;
            }
        }
        let try_table = match &control.kind {
            Kind::TryTable { catches, start } => Some((catches.clone(), *start)),
            _ => None,
        };
        let normal_exit = if try_table.is_some() && self.path == Reachability::Live {
            Some(self.jump(Op::Jump, 0)?)
        } else {
            None
        };
        let try_end = u32::try_from(self.code.len())
            .map_err(|_| self.control_error("Wasm try_table body is too large"))?;
        if let Some((catches, start)) = try_table {
            for catch in catches {
                let label_distance = usize::try_from(catch.label)
                    .ok()
                    .and_then(|depth| depth.checked_add(1))
                    .ok_or_else(|| self.control_error("Wasm catch label out of bounds"))?;
                let label_index = self
                    .controls
                    .len()
                    .checked_sub(label_distance)
                    .ok_or_else(|| self.control_error("Wasm catch label out of bounds"))?;
                let label_base = self.controls[label_index].base;
                let payload_count = catch
                    .tag_index
                    .map(|tag| {
                        self.tag_signatures
                            .get(tag as usize)
                            .map(|signature| signature.params.len())
                            .ok_or_else(|| self.control_error("Wasm catch tag out of bounds"))
                    })
                    .transpose()?
                    .unwrap_or(0)
                    .checked_add(usize::from(catch.catch_ref))
                    .ok_or_else(|| self.control_error("Wasm catch payload is too large"))?;
                let payload_count = u16::try_from(payload_count)
                    .map_err(|_| self.control_error("Wasm catch payload is too large"))?;
                let target = u32::try_from(self.code.len())
                    .map_err(|_| self.control_error("Wasm catch body is too large"))?;
                self.exception_handlers.push(super::WasmExceptionHandler {
                    start,
                    end: try_end,
                    tag_index: catch.tag_index,
                    target,
                    payload_register: label_base,
                    payload_count,
                    catch_ref: catch.catch_ref,
                    exception_register: None,
                });
                self.path = Reachability::Live;
                self.depth = label_base
                    .checked_add(payload_count)
                    .ok_or_else(|| self.control_error("Wasm catch stack is too large"))?;
                self.registers = self.registers.max(self.depth);
                self.branch(catch.label)?;
                self.make_dead();
            }
        }
        let end_target = self.code.len();
        if let Some(normal_exit) = normal_exit {
            self.patch_jump(normal_exit, end_target)?;
        }
        for exit in control.exits {
            self.patch_jump(exit, end_target)?;
        }
        self.depth = control.base + control.result_count;
        self.registers = self.registers.max(self.depth);
        self.path = if live {
            Reachability::Live
        } else {
            Reachability::Dead
        };
        if matches!(control.kind, Kind::Function) {
            if control.result_count > 1 {
                self.emit(
                    Op::WasmMultiValuePack,
                    0,
                    control.base,
                    0,
                    u32::from(control.result_count),
                )?;
            } else if control.result_count == 0 {
                self.emit(Op::LoadConst, 0, 0, 0, VOID_RESULT_CONSTANT)?;
            }
            self.emit(Op::Return, 0, 0, 0, 0)?;
        }
        Ok(())
    }

    fn branch(&mut self, relative_depth: u32) -> Result<(), Diagnostic> {
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
        let (base, result_count, head) = match control.kind {
            Kind::Loop { head } => (control.base, control.branch_count, Some(head)),
            _ => (control.base, control.result_count, None),
        };
        if result_count != 0 {
            let first_value = self
                .depth
                .checked_sub(result_count)
                .filter(|value| *value >= self.control_base())
                .ok_or_else(|| self.control_error("missing Wasm branch result"))?;
            for offset in 0..result_count {
                let destination = base + offset;
                let source = first_value + offset;
                if destination != source {
                    self.emit(Op::Move, destination, source, 0, 0)?;
                }
            }
        }
        let jump = self.jump(Op::Jump, 0)?;
        if let Some(head) = head {
            self.patch_jump(jump, head)?;
        } else {
            self.controls[index].exits.push(jump);
        }
        Ok(())
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
