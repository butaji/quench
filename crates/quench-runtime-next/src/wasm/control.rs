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
    Loop { head: usize },
    If(IfArm),
}

enum IfArm {
    Then { false_jump: Option<usize> },
    Else,
}

pub(super) struct Control {
    kind: Kind,
    base: Register,
    has_result: bool,
    entered: Reachability,
    exits: Vec<usize>,
}

impl Control {
    pub(super) fn function(has_result: bool) -> Self {
        Self {
            kind: Kind::Function,
            base: 0,
            has_result,
            entered: Reachability::Live,
            exits: Vec::new(),
        }
    }
}

impl Lowering<'_> {
    pub(super) fn control_operator(&mut self, operator: &Operator<'_>) -> Result<bool, Diagnostic> {
        match operator {
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
            Operator::Select => {
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

    fn begin(&mut self, kind: Kind, block_type: BlockType) -> Result<(), Diagnostic> {
        let has_result = match block_type {
            BlockType::Empty => false,
            BlockType::Type(ty) if WasmType::from_wasm(ty).is_some() => true,
            _ => return Err(self.control_error("unsupported Wasm block signature")),
        };
        self.controls.push(Control {
            kind,
            base: self.depth,
            has_result,
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
        self.depth = control.base;
        self.path = control.entered;
        Ok(())
    }

    fn check_result(&self, control: &Control) -> Result<(), Diagnostic> {
        if self.path == Reachability::Live
            && self.depth != control.base + u16::from(control.has_result)
        {
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
        if let Kind::If(IfArm::Then { false_jump }) = control.kind {
            if control.has_result {
                return Err(self.control_error("Wasm result if requires else"));
            }
            if let Some(false_jump) = false_jump {
                self.patch_jump(false_jump, self.code.len())?;
                live = true;
            }
        }
        for exit in control.exits {
            self.patch_jump(exit, self.code.len())?;
        }
        self.depth = control.base + u16::from(control.has_result);
        self.registers = self.registers.max(self.depth);
        self.path = if live {
            Reachability::Live
        } else {
            Reachability::Dead
        };
        if matches!(control.kind, Kind::Function) {
            if !control.has_result {
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
        let (base, has_result, head) = match control.kind {
            Kind::Loop { head } => (control.base, false, Some(head)),
            _ => (control.base, control.has_result, None),
        };
        if has_result {
            let value = self
                .depth
                .checked_sub(1)
                .filter(|value| *value >= self.control_base())
                .ok_or_else(|| self.control_error("missing Wasm branch result"))?;
            if base != value {
                self.emit(Op::Move, base, value, 0, 0)?;
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

    fn make_dead(&mut self) {
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
