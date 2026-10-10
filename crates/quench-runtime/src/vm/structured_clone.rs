use super::object_descriptors::PropertyDescriptorRecord;
use super::property_key::PropertyKey;
use super::*;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
struct CloneGraph {
    // Each source stays rooted for the traversal, so its handle remains a
    // stable memo key while recursive getters may run guest code.
    copies: HashMap<Value, RootId>,
    roots: Vec<RootId>,
}

impl CloneGraph {
    fn remember(&mut self, source: Value, target: RootId) {
        self.copies.insert(source, target);
    }

    fn copied<H: Host>(&self, vm: &Vm<H>, source: Value) -> Option<Value> {
        self.copies
            .get(&source)
            .and_then(|clone| vm.heap.root_value(*clone))
    }

    fn root<H: Host>(&mut self, vm: &mut Vm<H>, value: Value) -> RootId {
        let root = vm.heap.root(value);
        self.roots.push(root);
        root
    }

    fn release<H: Host>(&mut self, vm: &mut Vm<H>) {
        for root in self.roots.drain(..) {
            vm.heap.release_root(root);
        }
    }
}

impl<H: Host> Vm<H> {
    pub(crate) fn embedding_structured_clone(
        &mut self,
        source: RootId,
        options: Option<RootId>,
    ) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        let mut graph = CloneGraph::default();
        let result = (|| {
            let transfers = self.structured_clone_transfers(&program, options, &mut graph)?;
            let source = self.embedding_value(source)?;
            let clone = self.structured_clone_value(&program, source, &mut graph)?;
            self.validate_transfer_roots(&program, &transfers)?;
            for transfer in transfers {
                let buffer = self.heap.root_value(transfer).unwrap();
                self.detach_array_buffer_native(&program, &[buffer])?;
            }
            Ok(clone)
        })();
        graph.release(self);
        result
    }

    fn structured_clone_transfers(
        &mut self,
        program: &ResidualProgram,
        options: Option<RootId>,
        graph: &mut CloneGraph,
    ) -> Result<Vec<RootId>, JsError> {
        let Some(options) = options else {
            return Ok(Vec::new());
        };
        let options = self.embedding_value(options)?;
        if options.is_null() || options.is_undefined() {
            return Ok(Vec::new());
        }
        if !self.is_object_like(options) {
            return Err(
                self.type_error(program, "structuredClone options must be an object".into())
            );
        }

        let transfer_atom = self.intern_atom("transfer");
        let transfer = self.get_property(program, options, transfer_atom)?;
        if transfer.is_null() || transfer.is_undefined() {
            return Ok(Vec::new());
        }
        if !self.is_object_like(transfer) {
            return Err(self.type_error(
                program,
                "structuredClone transfer must be an iterable object".into(),
            ));
        }

        let transfer_root = graph.root(self, transfer);
        let transfer = self.heap.root_value(transfer_root).unwrap();
        let transfer_roots = self.iterable_to_rooted_list(program, transfer)?;
        graph.roots.extend(transfer_roots.iter().copied());
        self.validate_transfer_roots(program, &transfer_roots)?;
        Ok(transfer_roots)
    }

    fn validate_transfer_roots(
        &mut self,
        program: &ResidualProgram,
        transfers: &[RootId],
    ) -> Result<(), JsError> {
        let mut seen = HashSet::new();
        for root in transfers.iter().copied() {
            let value = self.heap.root_value(root).unwrap();
            let valid = matches!(
                self.heap.get(value),
                Some(Cell::ArrayBuffer {
                    shared: false,
                    detached: false,
                    immutable: false,
                    ..
                })
            );
            if !valid {
                return Err(self.data_clone_error(
                    program,
                    "Transfer list contains a value that cannot be transferred",
                ));
            }
            if !seen.insert(value) {
                return Err(
                    self.data_clone_error(program, "Transfer list contains a duplicate value")
                );
            }
        }
        Ok(())
    }

    fn structured_clone_value(
        &mut self,
        program: &ResidualProgram,
        source: Value,
        graph: &mut CloneGraph,
    ) -> Result<Value, JsError> {
        let Some(cell) = self.heap.get(source).cloned() else {
            return Ok(source);
        };
        if let Some(clone) = graph.copied(self, source) {
            return Ok(clone);
        }
        match cell {
            Cell::String(value) => Ok(self.heap.alloc(Cell::String(value))),
            Cell::BigInt(value) => Ok(self.heap.alloc(Cell::BigInt(value))),
            Cell::Symbol(_) => {
                Err(self.data_clone_error(program, "Symbol values cannot be cloned"))
            }
            Cell::Object(object) if object.has_error_data() || object.is_module_namespace() => {
                Err(self.data_clone_error(program, "Object cannot be cloned"))
            }
            Cell::Object(_) => {
                let source_root = graph.root(self, source);
                let target = self
                    .heap
                    .alloc(Cell::Object(Self::empty_object(self.object_proto)));
                let target_root = graph.root(self, target);
                graph.remember(source, target_root);
                self.structured_clone_properties(program, source_root, target_root, graph)?;
                Ok(self.heap.root_value(target_root).unwrap())
            }
            Cell::Array { .. } => {
                let length = self.own_array_length(source).unwrap_or_default();
                let source_root = graph.root(self, source);
                let prototype = self.array_prototype_for_realm(self.realm.globals);
                let target = self.heap.alloc(Cell::Array {
                    object: Self::empty_object(prototype),
                    elements: Rc::new(Vec::new()),
                });
                let target_root = graph.root(self, target);
                graph.remember(source, target_root);
                self.structured_clone_properties(program, source_root, target_root, graph)?;
                let target = self.heap.root_value(target_root).unwrap();
                self.set_array_length(program, target, Value::number(length as f64))?;
                Ok(self.heap.root_value(target_root).unwrap())
            }
            Cell::ArrayBuffer {
                bytes,
                shared,
                detached,
                max_byte_length,
                resizable,
                ..
            } if !shared && !detached => {
                graph.root(self, source);
                let mut copied_bytes = Vec::new();
                copied_bytes.try_reserve_exact(bytes.len()).map_err(|_| {
                    self.range_error(program, "ArrayBuffer is too large to clone".into())
                })?;
                copied_bytes.extend_from_slice(&bytes);
                let prototype = self.array_buffer_clone_prototype();
                let target = self.heap.alloc(Cell::ArrayBuffer {
                    object: Self::empty_object(prototype),
                    bytes: Rc::new(copied_bytes),
                    shared: false,
                    detached: false,
                    max_byte_length: if resizable {
                        max_byte_length
                    } else {
                        bytes.len()
                    },
                    resizable,
                    immutable: false,
                });
                let target_root = graph.root(self, target);
                graph.remember(source, target_root);
                Ok(self.heap.root_value(target_root).unwrap())
            }
            Cell::ArrayBuffer { .. } => {
                Err(self
                    .data_clone_error(program, "Detached or shared ArrayBuffer cannot be cloned"))
            }
            _ => Err(self.data_clone_error(program, "Value cannot be cloned")),
        }
    }

    fn structured_clone_properties(
        &mut self,
        program: &ResidualProgram,
        source: RootId,
        target: RootId,
        graph: &mut CloneGraph,
    ) -> Result<(), JsError> {
        if matches!(
            self.heap.get(self.heap.root_value(source).unwrap()),
            Some(Cell::Proxy { .. })
        ) {
            return Err(self.data_clone_error(program, "Proxy objects cannot be cloned"));
        }
        let source_value = self.heap.root_value(source).unwrap();
        let keys = self.object_own_key_values(program, source_value)?;
        let keys: Vec<_> = keys.into_iter().map(|key| graph.root(self, key)).collect();
        for key_root in keys {
            let key = self.heap.root_value(key_root).unwrap();
            let Some(Cell::String(name)) = self.heap.get(key).cloned() else {
                continue;
            };
            let atom = self.intern_js_atom(&name);
            if self.is_private_name(atom)
                || !self
                    .property_attributes(
                        self.heap.root_value(source).unwrap(),
                        PropertyKey::string(atom),
                    )
                    .is_some_and(|attributes| attributes.enumerable)
            {
                continue;
            }
            let source_value = self.heap.root_value(source).unwrap();
            let key = self.heap.root_value(key_root).unwrap();
            let item = self.get_index(program, source_value, key)?;
            let item = self.structured_clone_value(program, item, graph)?;
            let target_value = self.heap.root_value(target).unwrap();
            let key = self.heap.root_value(key_root).unwrap();
            if !self.define_own_property_record(
                program,
                target_value,
                key,
                PropertyDescriptorRecord::data(item),
            )? {
                return Err(self.data_clone_error(program, "Unable to create cloned property"));
            }
        }
        Ok(())
    }

    fn array_buffer_clone_prototype(&mut self) -> Value {
        let constructor = self.native_value(Native::ArrayBuffer);
        let prototype_atom = self.prototype_atom();
        self.own_property(constructor, prototype_atom)
            .unwrap_or(self.object_proto)
    }

    fn data_clone_error(&mut self, program: &ResidualProgram, message: &str) -> JsError {
        let message_value = self.heap.alloc(Cell::String(message.into()));
        let error = self
            .construct_error_native(program, Native::Error, &[message_value])
            .unwrap_or_else(|_| {
                self.heap
                    .alloc(Cell::Object(Object::error(self.object_proto)))
            });
        let name = self.heap.alloc(Cell::String("DataCloneError".into()));
        let _ = self.set_builtin_value_named(error, "name", name);
        JsError::thrown(error, format!("DataCloneError: {message}"))
    }
}
