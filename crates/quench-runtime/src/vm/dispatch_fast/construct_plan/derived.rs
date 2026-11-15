//! Deriving a construct plan from another: the plan a recorded construction leaves for its next
//! instance, and the plan a store of an initializer field makes (`ConstructPlan::with_field`).

use super::*;

impl ConstructPlan {
    /// This plan holding the fields `fields` that a forwarded initializer's adds gave a receiver
    /// of `shape` (`lane_record_forwarded_fields`), forwarding as `forwarded`.
    pub(super) fn with_recorded(&self, fields: &Self, shape: u32, forwarded: ForwardedResolution) -> Self {
        Self {
            shape,
            fields: fields.fields,
            parameters: fields.parameters,
            defaulted: fields.defaulted,
            constant: fields.constant,
            computed: fields.computed,
            operators: fields.operators,
            fallbacks: fields.fallbacks,
            nested: fields.nested,
            forwarded: Some(forwarded),
            ..*self
        }
        .derive_header()
    }

    /// This plan extended by the field `store` adds, `None` once it names all it can.
    pub(super) fn with_field(mut self, store: &crate::bytecode::InitializerStore, value: Value) -> Option<Self> {
        let field = usize::from(self.fields);
        let slot = self.parameters.get_mut(field)?;
        match store.value {
            crate::bytecode::InitializerValue::Parameter { parameter, fallback } => {
                *slot = parameter;
                if let Some(fallback) = fallback {
                    self.kind = ConstructPlanKind::Defaulted;
                    self.defaulted |= 1 << field;
                    self.fallbacks[field] = Value::number(fallback);
                }
            }
            crate::bytecode::InitializerValue::Constant(_) => {
                self.constant |= 1 << field;
                self.fallbacks[field] = value;
            }
            crate::bytecode::InitializerValue::Numeric { parameter, operator, constant } => {
                *slot = parameter;
                self.kind = ConstructPlanKind::Defaulted;
                self.computed |= 1 << field;
                self.operators[field] = operator;
                self.fallbacks[field] = Value::number(constant);
            }
            crate::bytecode::InitializerValue::Construct(_) => return None,
        }
        self.fields += 1;
        Some(self)
    }
}
