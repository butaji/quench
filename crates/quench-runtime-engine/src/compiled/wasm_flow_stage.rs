//! The Wasm tier's control flow: branches, tables, traps and returns built from the `Leave` of
//! a `Flow` step (`staged::wasm_flow`), the select as the stage's primitive, and the analysis
//! of each step's operands.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::wasm_instance_stage::value_kind;
use super::{Analysis, AnalysisState, Emitter, Kind, Slot};
use quench_runtime::WasmType;
use quench_runtime::bytecode::{Function, WideInstruction};
use quench_runtime::staged::machine::{MachineFlow, MachineStage};
use quench_runtime::staged::wasm::Width;
use quench_runtime::staged::wasm_flow::{Flow, Leave};
use cranelift_codegen::ir;

impl Emitter<'_, '_> {
    /// Lowers a flow step; whether control continues to the next instruction.
    pub(crate) fn flow(
        &mut self,
        function: &Function,
        pc: usize,
        instruction: WideInstruction,
        flow: Flow,
    ) -> bool {
        if flow == Flow::Select {
            self.select_register(pc, instruction);
        }
        let leave = flow.leave(pc, instruction, |entry| {
            function.widened(function.code[entry])
        });
        match leave {
            Leave::Next => return true,
            Leave::Jump(target) => self.jump(target, self.polls(pc, target)),
            Leave::Table(targets) => self.table(pc, instruction.a(), &targets),
            Leave::Trap(trap) => {
                let block = self.trap_block(trap);
                self.goto(block, &[]);
            }
            Leave::Return => self.return_register(instruction.a()),
        }
        false
    }

    fn select_register(&mut self, pc: usize, instruction: WideInstruction) {
        let (condition, _) = instruction.register_pair();
        let kind = self.operand_kind(pc);
        let condition = self.read(condition, Kind::I32);
        let taken = self.read(instruction.b(), kind);
        let other = self.read(instruction.c(), kind);
        let value = MachineStage::select(self, condition, taken, other);
        self.write(instruction.a(), kind, value);
    }

    /// A branch table on the i32 `selector`, a loop that repeats through the table alone
    /// polling like a jump.
    fn table(&mut self, pc: usize, selector: u16, targets: &[usize]) {
        let selector = self.read(selector, Kind::I32);
        if targets.iter().any(|&target| target <= pc) {
            self.interrupt_safepoint();
        }
        let (default, cases) = targets.split_last().expect("a default entry");
        let default = self.block(*default);
        let default = self.body.core.builder.func.dfg.block_call(default, &[]);
        let cases: Vec<_> = cases
            .iter()
            .map(|target| {
                let block = self.block(*target);
                self.body.core.builder.func.dfg.block_call(block, &[])
            })
            .collect();
        let table = ir::JumpTableData::new(default, &cases);
        let table = self.body.core.builder.create_jump_table(table);
        self.dispatch(selector, table);
    }

    /// Returns the function's result, read from `register`, with the success status.
    pub(crate) fn return_register(&mut self, register: u16) {
        let status = self.constant(Width::I64, 0);
        let mut values = vec![status];
        if let Some(ty) = self.plan.result {
            values.push(self.read(register, Kind::of(ty).expect("planned")));
        }
        self.leave(&values);
    }
}

impl Analysis<'_> {
    /// Checks a flow step's operands and applies its register writes. `result` is the type the
    /// function returns.
    pub(crate) fn transfer_flow(
        &self,
        function: &Function,
        instruction: WideInstruction,
        state: &mut AnalysisState,
        flow: Flow,
        result: Option<WasmType>,
    ) -> Option<()> {
        let registers = function.registers;
        let kind = |register: u16| value_kind(state.get(register, registers)?);
        match flow {
            Flow::Jump => Some(()),
            // Entries are `Jump`s; validation guarantees it for reachable tables.
            Flow::Table => (kind(instruction.a())? == Kind::I32).then_some(()),
            Flow::Select => {
                let (condition, _) = instruction.register_pair();
                (kind(condition)? == Kind::I32).then_some(())?;
                let selected = kind(instruction.b())?;
                (kind(instruction.c())? == selected).then_some(())?;
                state.set(instruction.a(), registers, Slot::Value(selected))
            }
            // The trap lowers to a return of the trap status (C asserts and aborts, which most Emscripten
            // functions hold), so a function holding `unreachable` lowers like any other.
            Flow::Unreachable => Some(()),
            Flow::Return => match result {
                Some(ty) => (kind(instruction.a())? == Kind::of(ty)?).then_some(()),
                None => Some(()),
            },
        }
    }
}
