//! Getter-free constructor display, shared by host inspection and stack APIs.
use super::object_get::DiagnosticProperty;
use super::*;

// Generic Object/empty names defer to prototype or intrinsic class lookup.
fn constructor_name_is_specific(name: &JsString) -> bool {
    !name.units().is_empty() && !name.units().iter().eq("Object".encode_utf16())
}

impl<H: Host> Vm<H> {
    pub(super) fn constructor_name(&self, value: Value) -> Option<JsString> {
        let object = self.object_data(value)?;
        if self.proxy_parts(value).is_some() {
            return Some(JsString::from_str(if self.is_function(value) {
                "Function"
            } else {
                "Object"
            }));
        }
        if let Some(name) = self.constructed_name(value) {
            if constructor_name_is_specific(&name) {
                return Some(name);
            }
        }
        let tag = self.well_known_symbols.get(WellKnownSymbol::ToStringTag);
        let constructor = self.lookup_atom("constructor");
        let mut current = value;
        while let Some(data) = self.object_data(current) {
            if self.proxy_parts(current).is_some() {
                break;
            }
            if let Some(tag) = tag {
                if let DiagnosticProperty::Data(tag) =
                    self.diagnostic_own_property(current, PropertyKey::symbol(tag))
                {
                    if let Some(name) = self.heap.string(tag) {
                        return Some(name.clone());
                    }
                }
            }
            if current != value {
                if let Some(key) = constructor {
                    if let DiagnosticProperty::Data(constructor) =
                        self.diagnostic_own_property(current, PropertyKey::string(key))
                    {
                        if matches!(self.heap.function(constructor), Some(FunctionRecord { kind, .. }) if !matches!(kind, FunctionKind::Native(Native::FunctionBoundCall)))
                        {
                            if let Some(name) = self.original_debug_name(constructor) {
                                if constructor_name_is_specific(&name) {
                                    return Some(name);
                                }
                            }
                        }
                    }
                }
            }
            current = data.proto();
        }
        Some(JsString::from_str(self.intrinsic_class_name(value, object)))
    }

    fn intrinsic_class_name(&self, value: Value, object: &Object) -> &'static str {
        if object.is_arguments_object() {
            return "Arguments";
        }
        // Arrays may live in the direct-record arena rather than a legacy Cell.
        if self.heap.array(value).is_some() {
            return "Array";
        }
        if self.error_is_error(value) {
            return "Error";
        }
        if let Some((brand, _)) = self.boxed_primitive_data(value) {
            use crate::BuiltinBrand as B;
            return match brand {
                B::BooleanObject => "Boolean",
                B::NumberObject => "Number",
                B::StringObject => "String",
                B::SymbolObject => "Symbol",
                B::BigIntObject => "BigInt",
                _ => unreachable!("boxed primitive brand"),
            };
        }
        // Explicit construction/prototype roles have a physical-class fallback.
        // Uncaptured intrinsic factories retain their existing class display;
        // their mutable constructor identity is a separate provenance gap.
        let uncaptured_factory = !object.was_used_as_prototype()
            && super::provenance::shape_names(&self.shapes, object.shape())
                .1
                .is_none();
        if self.is_promise(value) {
            return if uncaptured_factory {
                "Promise"
            } else {
                "Object"
            };
        }
        if self.heap.is_function_value(value) {
            return "Function";
        }
        if let Some(iterator) = self.heap.iterator(value) {
            return match iterator.kind {
                kind if kind.is_array_iterator() => "Array Iterator",
                IteratorKind::MapKeys | IteratorKind::MapValues | IteratorKind::MapEntries => {
                    "Map Iterator"
                }
                IteratorKind::SetValues | IteratorKind::SetEntries => "Set Iterator",
                IteratorKind::Generator | IteratorKind::AsyncGenerator => "Generator",
                _ => "Object",
            };
        }
        match self.heap.get(value) {
            Some(Cell::ArrayBuffer { shared: true, .. }) => "SharedArrayBuffer",
            Some(Cell::ArrayBuffer { .. }) => "ArrayBuffer",
            Some(Cell::TypedArray { kind, .. }) => super::typed_array_install::TYPED_ARRAY_INSTALLS
                .iter()
                .find_map(|(installed, _, name)| (installed == kind).then_some(*name))
                .expect("typed array intrinsic"),
            Some(Cell::DataView { .. }) => {
                if uncaptured_factory {
                    "DataView"
                } else {
                    "Object"
                }
            }
            Some(Cell::Map { .. }) => "Map",
            Some(Cell::Set { .. }) => "Set",
            Some(Cell::WeakMap { .. }) => "WeakMap",
            Some(Cell::WeakSet { .. }) => "WeakSet",
            Some(Cell::Date { .. }) => "Date",
            Some(Cell::RegExp { .. }) => "RegExp",
            Some(Cell::Error(_)) => "Error",
            _ => "Object",
        }
    }

    pub(crate) fn embedding_constructor_name(
        &self,
        root: RootId,
    ) -> Result<Option<JsString>, JsError> {
        Ok(self.constructor_name(self.embedding_value(root)?))
    }
}
