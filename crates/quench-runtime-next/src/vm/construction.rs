use super::*;
use crate::host::{CapabilityId, HostContext};

impl<H: Host> Vm<H> {
    pub(super) fn construct_value(
        &mut self,
        p: &ResidualProgram,
        callee: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let kind = match self.heap.get(callee) {
            Some(Cell::Function { kind, .. }) => *kind,
            _ => return Err(JsError("not a constructor".into())),
        };
        if let FunctionKind::Native(native) = kind {
            return self.construct_native(p, native, args);
        }
        let proto = self
            .lookup_atom("prototype")
            .and_then(|a| self.own_property(callee, a))
            .unwrap_or(Value::NULL);
        let object = self.heap.alloc(Cell::Object(Self::empty_object(proto)));
        let result = self.call_value(p, callee, object, args)?;
        Ok(if result.is_heap() { result } else { object })
    }

    pub(super) fn construct_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::Object => {
                if let Some(value) = args.first().copied()
                    && self.object_data(value).is_some()
                {
                    return Ok(value);
                }
                Ok(self.object())
            }
            Native::Array => {
                let len = args.first().and_then(|v| v.as_number()).unwrap_or(0.0) as usize;
                Ok(self.heap.alloc(Cell::Array {
                    object: Self::empty_object(self.array_proto),
                    elements: Rc::new(vec![Value::UNDEFINED; len]),
                }))
            }
            Native::ArrayBuffer | Native::SharedArrayBuffer => {
                self.construct_buffer_native(p, native, args)
            }
            Native::Uint8Array => self.construct_uint8_array_native(p, args),
            Native::Uint16Array => self.construct_uint16_array_native(p, args),
            Native::Uint32Array => self.construct_uint32_array_native(p, args),
            Native::Int8Array => self.construct_int8_array_native(p, args),
            Native::Int16Array => self.construct_int16_array_native(p, args),
            Native::Int32Array => self.construct_int32_array_native(p, args),
            Native::DataView => self.construct_data_view_native(p, args),
            Native::Map | Native::Set => self.construct_collection_native(native, args),
            Native::WeakMap | Native::WeakSet => self.construct_weak_collection_native(native),
            Native::WeakRef => self.construct_weak_ref_native(args),
            Native::Date => Ok(self.heap.alloc(Cell::Date(
                HostContext::new(&mut self.host).invoke(CapabilityId::ClockMillis, None),
            ))),
            Native::Error => {
                let v = args.first().copied().unwrap_or(Value::UNDEFINED);
                let text = self.to_string(p, v)?;
                Ok(self.heap.alloc(Cell::Error(text)))
            }
            Native::String => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                let text = self.to_string(p, value)?;
                Ok(self.heap.alloc(Cell::String(text)))
            }
            Native::Number => Ok(Value::number(
                self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?,
            )),
            _ => Err(JsError("native is not constructible".into())),
        }
    }
}
