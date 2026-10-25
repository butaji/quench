//! Nested literals of constants, built by one instruction (`MakeLiteralTemplate`).

use super::*;
use crate::bytecode::LiteralStep;
use rustc_hash::FxHashSet;

/// Containers a literal must nest before it is built from a template: a single array or
/// object of constants already has its own one-instruction form (`MakeConstArray`,
/// `MakeObjectLiteral`), so a template pays once it replaces evaluating nested containers.
const MIN_TEMPLATE_CONTAINERS: usize = 2;

impl FunctionCompiler<'_, '_> {
    /// A register holding `value` built from a literal template, when `value` is a nested
    /// literal of constants. Building one has no observable effect besides creating its
    /// containers, in the order and with the keys evaluation would.
    pub(super) fn literal_template(&mut self, value: &Expression<'_>) -> Option<Register> {
        if Self::literal_containers(value)? < MIN_TEMPLATE_CONTAINERS {
            return None;
        }
        let mut steps = Vec::new();
        self.literal_steps(value, &mut steps);
        let index = self.owner.literal_templates.len() as u32;
        self.owner.literal_templates.push(steps.into_boxed_slice());
        let dst = self.reg();
        self.emit(Op::MakeLiteralTemplate, dst, 0, 0, index);
        Some(dst)
    }

    /// The containers of `value` when it is a literal of constants: arrays without holes
    /// or spreads, and objects of distinct static data keys, whose leaves are constants.
    fn literal_containers(value: &Expression<'_>) -> Option<usize> {
        match value {
            Expression::ArrayExpression(array) => {
                array.elements.iter().try_fold(1, |count, element| {
                    Some(count + Self::literal_containers(element.as_expression()?)?)
                })
            }
            Expression::ObjectExpression(object) => {
                let mut keys = FxHashSet::default();
                Self::static_object_data_properties(object)?.iter().try_fold(
                    1,
                    |count, (property, key)| {
                        keys.insert(*key).then_some(())?;
                        Some(count + Self::literal_containers(&property.value)?)
                    },
                )
            }
            value => binding_time::expression(value).static_value().map(|_| 0),
        }
    }

    /// Appends the steps that build `value`, which `literal_containers` admitted.
    fn literal_steps(&mut self, value: &Expression<'_>, steps: &mut Vec<LiteralStep>) {
        match value {
            Expression::ArrayExpression(array) => {
                for element in &array.elements {
                    let element = element.as_expression().expect("an admitted element is a value");
                    self.literal_steps(element, steps);
                }
                steps.push(LiteralStep::Array(array.elements.len() as u32));
            }
            Expression::ObjectExpression(object) => {
                let properties = Self::static_object_data_properties(object)
                    .expect("an admitted object has static data properties");
                let atoms = properties.iter().map(|(_, key)| self.owner.atom(key)).collect();
                for (property, _) in &properties {
                    self.literal_steps(&property.value, steps);
                }
                let site = self.owner.object_sites.len() as u32;
                self.owner.object_sites.push(ObjectSite { atoms });
                steps.push(LiteralStep::Object(site));
            }
            value => {
                let constant = binding_time::expression(value)
                    .static_value()
                    .expect("an admitted leaf is a constant");
                steps.push(LiteralStep::Constant(self.owner.constant(constant)));
            }
        }
    }
}
