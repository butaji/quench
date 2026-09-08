//! Bounded property/numeric expression covering over canonical residual code.
//!
//! This is a disposable physical recipe, not another semantic IR. Every node
//! points at an existing residual operation; execution succeeds only when all
//! property reads are already proven side-effect-free numeric cache hits.

use crate::ir::{Opcode, Register};
use crate::machine::{BaselineEntry, CodeView};
use crate::ops::BinaryOp;
use crate::stencil_cfg::ControlFlowFacts;

pub(crate) const MAX_PROPERTY_NUMERIC_VALUES: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq)]
enum ValueRecipe {
    Empty,
    Local(u16),
    Alias(u8),
    Property {
        receiver_slot: u16,
        operation: u8,
    },
    Binary {
        operator: BinaryOp,
        lhs: u8,
        rhs: u8,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct RecipeNode {
    register: Register,
    recipe: ValueRecipe,
}

const EMPTY_NODE: RecipeNode = RecipeNode {
    register: 0,
    recipe: ValueRecipe::Empty,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PropertyNumericSelection {
    nodes: [RecipeNode; MAX_PROPERTY_NUMERIC_VALUES],
    len: u8,
    result: u8,
    span: u8,
    property_count: u8,
}

pub(crate) struct PropertyNumericPlan {
    selection: PropertyNumericSelection,
    #[cfg(test)]
    entries: u64,
}

impl PropertyNumericPlan {
    pub(crate) const fn new(selection: PropertyNumericSelection) -> Self {
        Self {
            selection,
            #[cfg(test)]
            entries: 0,
        }
    }

    pub(crate) fn execute(
        &mut self,
        environment: &crate::environment::Environment,
        property: impl FnMut(u8, u16) -> Option<f64>,
    ) -> Option<f64> {
        let value = self.selection.evaluate(environment, property)?;
        #[cfg(test)]
        {
            self.entries = self.entries.saturating_add(1);
        }
        Some(value)
    }

    pub(crate) const fn span(&self) -> usize {
        self.selection.span()
    }

    #[cfg(test)]
    pub(crate) const fn entry_count(&self) -> u64 {
        self.entries
    }

    #[cfg(test)]
    pub(crate) const fn selection(&self) -> PropertyNumericSelection {
        self.selection
    }
}

impl PropertyNumericSelection {
    pub(crate) fn evaluate(
        self,
        environment: &crate::environment::Environment,
        mut property: impl FnMut(u8, u16) -> Option<f64>,
    ) -> Option<f64> {
        self.validate_local_bindings(environment)?;
        let mut values = [None; MAX_PROPERTY_NUMERIC_VALUES];
        for (index, node) in self
            .nodes
            .iter()
            .copied()
            .take(usize::from(self.len))
            .enumerate()
        {
            #[cfg(test)]
            crate::test_execution_profile::event("portable_recipe_step");
            values[index] = evaluate_node(
                node.recipe,
                &self.nodes,
                &values,
                environment,
                &mut property,
            )?;
        }
        *values.get(usize::from(self.result))?
    }

    fn validate_local_bindings(self, environment: &crate::environment::Environment) -> Option<()> {
        self.nodes
            .iter()
            .take(usize::from(self.len))
            .filter_map(|node| match node.recipe {
                ValueRecipe::Local(slot) => Some(slot),
                _ => None,
            })
            .all(|slot| !environment.is_deleted_slot(slot) && !environment.is_uninitialized(slot))
            .then_some(())
    }

    pub(crate) const fn span(self) -> usize {
        self.span as usize
    }

    pub(crate) const fn property_count(self) -> u8 {
        self.property_count
    }

    #[cfg(test)]
    fn operation_route(self) -> Vec<&'static str> {
        self.nodes[..usize::from(self.len)]
            .iter()
            .map(|node| match node.recipe {
                ValueRecipe::Local(_) => "local",
                ValueRecipe::Alias(_) => "alias",
                ValueRecipe::Property { .. } => "guarded_property_number",
                ValueRecipe::Binary {
                    operator: BinaryOp::Multiply,
                    ..
                } => "number_multiply",
                ValueRecipe::Binary {
                    operator: BinaryOp::Add,
                    ..
                } => "number_add",
                ValueRecipe::Binary { .. } => "number_binary",
                ValueRecipe::Empty => "empty",
            })
            .chain(std::iter::once("return"))
            .collect()
    }
}

fn evaluate_node(
    recipe: ValueRecipe,
    nodes: &[RecipeNode; MAX_PROPERTY_NUMERIC_VALUES],
    values: &[Option<f64>; MAX_PROPERTY_NUMERIC_VALUES],
    environment: &crate::environment::Environment,
    property: &mut impl FnMut(u8, u16) -> Option<f64>,
) -> Option<Option<f64>> {
    match recipe {
        ValueRecipe::Local(_) => Some(None),
        ValueRecipe::Alias(source) => values.get(usize::from(source)).copied(),
        ValueRecipe::Property {
            receiver_slot,
            operation,
        } => Some(Some(property(operation, receiver_slot)?)),
        ValueRecipe::Binary { operator, lhs, rhs } => Some(Some(crate::vm::arithmetic_number(
            numeric_value(lhs, nodes, values, environment)?,
            numeric_value(rhs, nodes, values, environment)?,
            operator,
        )?)),
        ValueRecipe::Empty => None,
    }
}

fn numeric_value(
    mut index: u8,
    nodes: &[RecipeNode; MAX_PROPERTY_NUMERIC_VALUES],
    values: &[Option<f64>; MAX_PROPERTY_NUMERIC_VALUES],
    environment: &crate::environment::Environment,
) -> Option<f64> {
    for _ in 0..MAX_PROPERTY_NUMERIC_VALUES {
        if let Some(value) = values.get(usize::from(index)).copied().flatten() {
            return Some(value);
        }
        match nodes.get(usize::from(index))?.recipe {
            ValueRecipe::Local(slot) => return environment.get_number(slot),
            ValueRecipe::Alias(source) => index = source,
            _ => return None,
        }
    }
    None
}

pub(crate) fn select_property_numeric_return(
    code: CodeView<'_>,
    entries: &[BaselineEntry],
    cfg: &ControlFlowFacts,
    start: usize,
) -> Option<PropertyNumericSelection> {
    let mut builder = RecipeBuilder::new(start);
    for pc in start..entries.len().min(start + MAX_PROPERTY_NUMERIC_VALUES) {
        let instruction = entries[pc].instruction;
        if instruction.opcode == Opcode::Return {
            return builder.finish(code, cfg, pc, instruction.a);
        }
        builder.push(code, pc, instruction)?;
    }
    None
}

struct RecipeBuilder {
    start: usize,
    nodes: [RecipeNode; MAX_PROPERTY_NUMERIC_VALUES],
    len: u8,
    properties: u8,
    binaries: u8,
}

impl RecipeBuilder {
    const fn new(start: usize) -> Self {
        Self {
            start,
            nodes: [EMPTY_NODE; MAX_PROPERTY_NUMERIC_VALUES],
            len: 0,
            properties: 0,
            binaries: 0,
        }
    }

    fn push(
        &mut self,
        code: CodeView<'_>,
        pc: usize,
        instruction: crate::ir::Instruction,
    ) -> Option<()> {
        let recipe = match instruction.opcode {
            Opcode::LoadLocal | Opcode::LoadLocalChecked => ValueRecipe::Local(instruction.b),
            Opcode::Move if instruction.flags == 0 => {
                ValueRecipe::Alias(self.current(instruction.b)?)
            }
            Opcode::GetN | Opcode::GetNQuickened if instruction.flags == 0 => {
                code.metadata_at(pc)?.name.as_deref()?;
                self.properties = self.properties.checked_add(1)?;
                ValueRecipe::Property {
                    receiver_slot: self.local_slot(self.current(instruction.b)?)?,
                    operation: u8::try_from(pc.checked_sub(self.start)?).ok()?,
                }
            }
            Opcode::Add | Opcode::Sub | Opcode::Mul | Opcode::Div => {
                self.binaries = self.binaries.checked_add(1)?;
                ValueRecipe::Binary {
                    operator: crate::stencil_plan::numeric_operation(instruction)?,
                    lhs: self.current(instruction.b)?,
                    rhs: self.current(instruction.c)?,
                }
            }
            _ => return None,
        };
        self.append(RecipeNode {
            register: instruction.a,
            recipe,
        })
    }

    fn append(&mut self, node: RecipeNode) -> Option<()> {
        *self.nodes.get_mut(usize::from(self.len))? = node;
        self.len = self.len.checked_add(1)?;
        Some(())
    }

    fn current(&self, register: Register) -> Option<u8> {
        self.nodes[..usize::from(self.len)]
            .iter()
            .rposition(|node| node.register == register)
            .and_then(|index| u8::try_from(index).ok())
    }

    fn local_slot(&self, mut index: u8) -> Option<u16> {
        for _ in 0..self.len {
            match self.nodes.get(usize::from(index))?.recipe {
                ValueRecipe::Local(slot) => return Some(slot),
                ValueRecipe::Alias(source) => index = source,
                _ => return None,
            }
        }
        None
    }

    fn finish(
        self,
        _code: CodeView<'_>,
        cfg: &ControlFlowFacts,
        return_pc: usize,
        result: Register,
    ) -> Option<PropertyNumericSelection> {
        let end = return_pc.checked_add(1)?;
        cfg.region_control(self.start, end)?;
        (self.properties >= 2 && self.binaries >= 1).then_some(())?;
        Some(PropertyNumericSelection {
            nodes: self.nodes,
            len: self.len,
            result: self.current(result)?,
            span: u8::try_from(end.checked_sub(self.start)?).ok()?,
            property_count: self.properties,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completion::Completion;
    use crate::machine::{BaselinePlan, FunctionCode};
    use crate::value::{ObjectData, Value};
    use std::rc::Rc;

    fn property_numeric_body(root: CodeView<'_>) -> FunctionCode {
        let mut pending = nested_bodies(root);
        while let Some(body) = pending.pop() {
            let Some(code) = body.code() else { continue };
            let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
            let plan = BaselinePlan::compile_for_test(code, policy);
            if (0..code.len()).any(|pc| plan.property_numeric_at(pc).is_some()) {
                return body;
            }
            pending.extend(nested_bodies(code));
        }
        panic!("property numeric function")
    }

    fn nested_bodies(code: CodeView<'_>) -> Vec<FunctionCode> {
        let mut bodies = Vec::new();
        code.cold_ops()
            .for_each(|(_, op)| op.visit_bodies(&mut |body| bodies.push(body.clone())));
        bodies
    }

    fn object(values: [f64; 3]) -> Value {
        Value::Object(Rc::new(ObjectData::new(vec![
            ("x".into(), Value::Number(values[0])),
            ("y".into(), Value::Number(values[1])),
            ("z".into(), Value::Number(values[2])),
        ])))
    }

    fn source_plan(source: &str) -> (FunctionCode, BaselinePlan, usize, [u16; 2]) {
        let program = crate::reduce::reduce_source(source).expect("source lowers");
        let body = property_numeric_body(program.code());
        let code = body.code().expect("linked body");
        let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
        let plan = BaselinePlan::compile_for_test(code, policy);
        let pc = (0..code.len())
            .find(|pc| plan.property_numeric_at(*pc).is_some())
            .expect("property numeric admission");
        let selection = plan.property_numeric_at(pc).unwrap().borrow().selection();
        let mut slots = Vec::new();
        selection.nodes[..usize::from(selection.len)]
            .iter()
            .filter_map(|node| match node.recipe {
                ValueRecipe::Local(slot) => Some(slot),
                _ => None,
            })
            .for_each(|slot| {
                if !slots.contains(&slot) {
                    slots.push(slot);
                }
            });
        (body, plan, pc, [slots[0], slots[1]])
    }

    fn execute(
        code: CodeView<'_>,
        plan: &BaselinePlan,
        pc: usize,
        slots: [u16; 2],
        values: [Value; 2],
    ) -> Value {
        let environment = crate::environment::Environment::new();
        environment.set(slots[0], values[0].clone());
        environment.set(slots[1], values[1].clone());
        let mut registers = crate::register_file::RegisterFile::with_undefined(
            usize::from(code.register_count()).max(8),
        );
        let (completion, _) = crate::vm::execute_baseline_code_from(
            code,
            plan,
            pc,
            &mut registers,
            &crate::vm::current_context_or_default(),
            environment,
        )
        .expect("execution");
        let Completion::Return(value) = completion else {
            panic!("property expression must return")
        };
        value
    }

    #[test]
    fn ordinary_source_reuses_one_bounded_property_numeric_cover() {
        let case = crate::test_execution_profile::ExecutionCase::load("property_numeric_dot");
        case.assert_standalone();
        let (body, plan, pc, slots) = source_plan(case.source());
        let code = body.code().unwrap();
        let values = [object([1.0, 2.0, 3.0]), object([4.0, 5.0, 6.0])];
        assert_eq!(
            case.warmup(),
            1,
            "this runner currently performs one warmup"
        );
        assert_eq!(
            execute(code, &plan, pc, slots, values.clone()),
            Value::Number(32.0)
        );
        let (result, profile) =
            crate::test_execution_profile::capture(|| execute(code, &plan, pc, slots, values));
        case.assert(&result, &profile);
        let fused = plan.property_numeric_at(pc).unwrap().borrow();
        case.assert_plan(
            crate::test_execution_profile::ExecutionKind::PortableRecipe,
            &fused.selection().operation_route(),
        );
        assert_eq!(fused.selection().property_count(), 6);
        assert_eq!(
            fused.selection().operation_route(),
            [
                "local",
                "guarded_property_number",
                "local",
                "guarded_property_number",
                "number_multiply",
                "local",
                "guarded_property_number",
                "local",
                "guarded_property_number",
                "number_multiply",
                "number_add",
                "local",
                "guarded_property_number",
                "local",
                "guarded_property_number",
                "number_multiply",
                "number_add",
                "return",
            ]
        );
        assert_eq!(
            fused.entry_count(),
            1,
            "first pass installs IC facts; second fuses"
        );
    }

    #[test]
    fn broken_receiver_fact_uses_complete_ordinary_path() {
        let case =
            crate::test_execution_profile::ExecutionCase::load("property_numeric_dot_fallback");
        case.assert_standalone();
        let (body, plan, pc, slots) = source_plan(case.source());
        let code = body.code().unwrap();
        let (value, profile) = crate::test_execution_profile::capture(|| {
            execute(
                code,
                &plan,
                pc,
                slots,
                [
                    Value::String("not an object".into()),
                    object([4.0, 5.0, 6.0]),
                ],
            )
        });
        assert!(matches!(&value, Value::Number(number) if number.is_nan()));
        case.assert(&value, &profile);
        let selection = plan.property_numeric_at(pc).unwrap().borrow().selection();
        case.assert_plan(
            crate::test_execution_profile::ExecutionKind::OrdinaryFallback,
            &selection.operation_route(),
        );
        assert_eq!(
            plan.property_numeric_at(pc).unwrap().borrow().entry_count(),
            0
        );
    }
}
