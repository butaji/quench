use super::object_descriptors::PropertyDescriptorRecord;
use super::*;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
struct CloneGraph {
    // Each source stays rooted for the traversal, so its handle remains a
    // stable memo key while recursive getters may run guest code.
    copies: HashMap<Value, RootId>,
    roots: Vec<RootId>,
    views: Vec<SerializedView>,
}

struct SerializedView {
    view: RootId,
    byte_length: usize,
    backed_by_resizable: bool,
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
    pub(crate) fn embedding_mark_untransferable(&mut self, source: RootId) -> Result<(), JsError> {
        let source = self.embedding_value(source)?;
        if let Some(object) = self.object_data_mut(source) {
            object.forbid_structured_transfer();
        }
        Ok(())
    }

    pub(crate) fn embedding_is_marked_untransferable(
        &self,
        source: RootId,
    ) -> Result<bool, JsError> {
        let source = self.embedding_value(source)?;
        Ok(self
            .object_data(source)
            .is_some_and(Object::is_structured_transfer_forbidden))
    }

    pub(crate) fn embedding_mark_uncloneable(&mut self, source: RootId) -> Result<(), JsError> {
        let source = self.embedding_value(source)?;
        // The host marker applies to ordinary objects. Built-in serialized
        // types (arrays, buffers, maps) retain their own clone protocol.
        if self.heap.object_record(source).is_some() {
            self.heap
                .object_mut(source)
                .expect("ordinary object remains live")
                .forbid_structured_clone();
        }
        Ok(())
    }

    pub(crate) fn embedding_structured_clone(
        &mut self,
        source: RootId,
        options: Option<RootId>,
    ) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        let mut graph = CloneGraph::default();
        let result = (|| {
            let transfers = self.structured_clone_transfers(&program, options, &mut graph)?;
            self.finish_structured_clone(&program, source, &transfers, &mut graph)
        })();
        graph.release(self);
        result
    }

    pub(crate) fn embedding_structured_clone_with_transfers(
        &mut self,
        source: RootId,
        transfers: &[RootId],
    ) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        let mut graph = CloneGraph::default();
        let result = self.finish_structured_clone(&program, source, transfers, &mut graph);
        graph.release(self);
        result
    }

    fn finish_structured_clone(
        &mut self,
        program: &ResidualProgram,
        source: RootId,
        transfers: &[RootId],
        graph: &mut CloneGraph,
    ) -> Result<Value, JsError> {
        self.validate_transfer_roots(program, transfers)?;
        for transfer in transfers {
            let buffer = self.heap.root_value(*transfer).unwrap();
            graph.root(self, buffer);
            let prototype = self.intrinsic_builtin_prototype(Native::ArrayBuffer);
            let target = self.heap.alloc(Cell::ArrayBuffer {
                object: Box::new(Self::empty_object(prototype)),
                bytes: Default::default(),
                shared: false,
                detached: false,
                max_byte_length: 0,
                resizable: false,
                immutable: false,
            });
            let target = graph.root(self, target);
            graph.remember(buffer, target);
        }
        let source = self.embedding_value(source)?;
        let clone = self.structured_clone_value(program, source, graph)?;
        // Node serializes first, then transfers the current backing stores.
        // Getters may resize/detach a buffer, or install a detach key meanwhile.
        for transfer in transfers {
            let buffer = self.heap.root_value(*transfer).unwrap();
            let (bytes, maximum, resizable) = match self.heap.get(buffer) {
                Some(Cell::ArrayBuffer { detached: true, .. }) => (Default::default(), 0, false),
                Some(Cell::ArrayBuffer {
                    bytes,
                    max_byte_length,
                    resizable,
                    ..
                }) => (bytes.clone(), *max_byte_length, *resizable),
                _ => unreachable!("validated transfer remains an ArrayBuffer"),
            };
            self.detach_array_buffer_native(program, &[buffer])?;
            let target = graph.copied(self, buffer).unwrap();
            let Some(Cell::ArrayBuffer {
                bytes: target_bytes,
                max_byte_length,
                resizable: target_resizable,
                ..
            }) = self.heap.get_mut(target)
            else {
                unreachable!("reserved transfer target remains an ArrayBuffer");
            };
            let before = target_bytes.capacity();
            let after = bytes.capacity();
            *target_bytes = bytes;
            *max_byte_length = maximum;
            *target_resizable = resizable;
            self.heap.adjust_external_bytes(before, after);
        }
        self.validate_cloned_views(program, graph)?;
        Ok(clone)
    }

    fn validate_cloned_views(
        &mut self,
        program: &ResidualProgram,
        graph: &CloneGraph,
    ) -> Result<(), JsError> {
        for view in &graph.views {
            let value = self.heap.root_value(view.view).unwrap();
            let (buffer, offset, tracking) = match self.heap.get(value) {
                Some(
                    Cell::TypedArray {
                        buffer,
                        object,
                        length_tracking,
                        ..
                    }
                    | Cell::DataView {
                        buffer,
                        object,
                        length_tracking,
                        ..
                    },
                ) => (*buffer, object.offset, *length_tracking),
                _ => unreachable!("serialized view remains live"),
            };
            let Some(Cell::ArrayBuffer {
                bytes,
                resizable,
                shared,
                ..
            }) = self.heap.get(buffer)
            else {
                unreachable!("serialized view retains its backing buffer");
            };
            if offset > bytes.len()
                || view.byte_length > bytes.len() - offset
                || ((view.backed_by_resizable || tracking) && !resizable)
                || (view.backed_by_resizable && *shared)
                || (*resizable && !shared && !view.backed_by_resizable)
            {
                let message = "Unable to deserialize cloned data.";
                let text = self.heap.alloc(Cell::String(message.into()));
                let error = self.construct_error_native(program, Native::Error, &[text])?;
                return Err(JsError::thrown(error, message.into()));
            }
        }
        Ok(())
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
            if self
                .object_data(value)
                .is_some_and(Object::is_structured_transfer_forbidden)
                || matches!(self.heap.get(value), Some(Cell::ArrayBuffer { detached, immutable, bytes, .. })
                    if *detached || *immutable || bytes.is_wasm_view())
            {
                return Err(
                    self.data_clone_error(program, "Cannot transfer object of unsupported type.")
                );
            }
            let valid = matches!(
                self.heap.get(value),
                Some(Cell::ArrayBuffer {
                    bytes,
                    shared: false,
                    detached: false,
                    immutable: false,
                    ..
                }) if !bytes.is_wasm_view()
            );
            if !valid {
                return Err(self.data_clone_error(program, "Found invalid value in transferList."));
            }
            if !seen.insert(value) {
                return Err(
                    self.data_clone_error(program, "Transfer list contains duplicate ArrayBuffer")
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
        if let Some(clone) = graph.copied(self, source) {
            return Ok(clone);
        }
        if self
            .object_data(source)
            .is_some_and(Object::is_arguments_object)
        {
            return Err(self.data_clone_error(program, "#<Object> could not be cloned."));
        }
        if let Some(object) = self.heap.object_record(source) {
            if object.is_structured_clone_forbidden() {
                return Err(
                    self.data_clone_error(program, "Cannot clone object of unsupported type.")
                );
            }
            if self.realm.promise.records.contains_key(&source) {
                return Err(self.data_clone_error(program, "#<Promise> could not be cloned."));
            }
            if object.has_error_data() {
                return self.structured_clone_error(program, source, graph);
            }
            if object.is_module_namespace() {
                return Err(self.data_clone_error(program, "Object cannot be cloned"));
            }
            if let Some((brand, value)) = self.boxed_primitive_data(source) {
                if brand == crate::BuiltinBrand::SymbolObject {
                    return Err(
                        self.data_clone_error(program, "[object Symbol] could not be cloned.")
                    );
                }
                graph.root(self, source);
                let target = self.box_primitive_object(value)?;
                let target_root = graph.root(self, target);
                graph.remember(source, target_root);
                return Ok(self.heap.root_value(target_root).unwrap());
            }
            let source_root = graph.root(self, source);
            let target = self
                .heap
                .alloc(Cell::Object(Self::empty_object(self.object_proto)));
            let target_root = graph.root(self, target);
            graph.remember(source, target_root);
            self.structured_clone_properties(program, source_root, target_root, graph)?;
            return Ok(self.heap.root_value(target_root).unwrap());
        }
        if self.heap.array(source).is_some() {
            let length = self.own_array_length(source).unwrap_or_default();
            let source_root = graph.root(self, source);
            let prototype = self.array_prototype_for_realm(self.realm.globals);
            let target = self.heap.alloc(Cell::Array {
                object: crate::heap::ArrayObject::new(
                    Self::empty_object(prototype),
                    Rc::new(Vec::new()),
                ),
            });
            let target_root = graph.root(self, target);
            graph.remember(source, target_root);
            self.structured_clone_properties(program, source_root, target_root, graph)?;
            let target = self.heap.root_value(target_root).unwrap();
            self.set_array_length(program, target, Value::number(length as f64))?;
            return Ok(self.heap.root_value(target_root).unwrap());
        }
        let Some(cell) = self.heap.get(source).cloned() else {
            return Ok(source);
        };
        match cell {
            Cell::TypedArray {
                kind,
                object,
                buffer,
                length,
                length_tracking,
            } => {
                graph.root(self, source);
                let detached = self.array_buffer_detached(buffer);
                if detached && graph.copied(self, buffer).is_none() {
                    return Err(self.data_clone_error(
                        program,
                        "An ArrayBuffer is detached and could not be cloned.",
                    ));
                }
                if !detached && self.typed_array_out_of_bounds(source) {
                    return Err(
                        self.data_clone_error(program, "[object Object] could not be cloned.")
                    );
                }
                // Length-tracking views serialize an auto-length, not the
                // source buffer's current derived length.
                let byte_length = if length_tracking {
                    0
                } else {
                    length * kind.width()
                };
                let backed_by_resizable = self.array_buffer_resizable(buffer)
                    && matches!(
                        self.heap.get(buffer),
                        Some(Cell::ArrayBuffer { shared: false, .. })
                    );
                let buffer = self.structured_clone_value(program, buffer, graph)?;
                let prototype = self.typed_array_proto(kind);
                let target = self.heap.alloc(Cell::TypedArray {
                    kind,
                    object: Box::new(crate::heap::BufferViewObject {
                        object: Self::empty_object(prototype),
                        offset: object.offset,
                    }),
                    buffer,
                    length,
                    length_tracking,
                });
                let target_root = graph.root(self, target);
                graph.remember(source, target_root);
                graph.views.push(SerializedView {
                    view: target_root,
                    byte_length,
                    backed_by_resizable,
                });
                Ok(self.heap.root_value(target_root).unwrap())
            }
            Cell::DataView {
                object,
                buffer,
                length,
                length_tracking,
            } => {
                graph.root(self, source);
                let detached = self.array_buffer_detached(buffer);
                if detached && graph.copied(self, buffer).is_none() {
                    return Err(self.data_clone_error(
                        program,
                        "An ArrayBuffer is detached and could not be cloned.",
                    ));
                }
                let required = if length_tracking { 0 } else { length };
                let backed_by_resizable = self.array_buffer_resizable(buffer)
                    && matches!(
                        self.heap.get(buffer),
                        Some(Cell::ArrayBuffer { shared: false, .. })
                    );
                if (!detached || backed_by_resizable)
                    && self.array_buffer_out_of_bounds(buffer, object.offset, required)
                {
                    return Err(self.data_clone_error(program, "#<DataView> could not be cloned."));
                }
                let byte_length = required;
                let buffer = self.structured_clone_value(program, buffer, graph)?;
                let prototype = self.intrinsic_builtin_prototype(Native::DataView);
                let target = self.heap.alloc(Cell::DataView {
                    object: Box::new(crate::heap::BufferViewObject {
                        object: Self::empty_object(prototype),
                        offset: object.offset,
                    }),
                    buffer,
                    length,
                    length_tracking,
                });
                let target_root = graph.root(self, target);
                graph.remember(source, target_root);
                graph.views.push(SerializedView {
                    view: target_root,
                    byte_length,
                    backed_by_resizable,
                });
                Ok(self.heap.root_value(target_root).unwrap())
            }
            Cell::Map { entries, .. } => {
                graph.root(self, source);
                // Snapshot and root every entry before cloning can invoke a
                // getter that deletes or replaces entries in the source.
                let entries: Vec<_> = entries
                    .iter()
                    .map(|(key, value)| [graph.root(self, key), graph.root(self, value)])
                    .collect();
                let prototype = self.intrinsic_builtin_prototype(Native::Map);
                let target = self.heap.alloc(Cell::Map {
                    object: Box::new(Self::empty_object(prototype)),
                    entries: Box::default(),
                });
                let target_root = graph.root(self, target);
                graph.remember(source, target_root);
                for [key, value] in entries {
                    let key = self.heap.root_value(key).unwrap();
                    let key = self.structured_clone_value(program, key, graph)?;
                    let key = graph.root(self, key);
                    let value = self.heap.root_value(value).unwrap();
                    let value = self.structured_clone_value(program, value, graph)?;
                    let key = self.heap.root_value(key).unwrap();
                    let target = self.heap.root_value(target_root).unwrap();
                    self.call_collection_native(program, Native::MapSet, target, &[key, value])?;
                }
                Ok(self.heap.root_value(target_root).unwrap())
            }
            Cell::Set { entries, .. } => {
                graph.root(self, source);
                let entries: Vec<_> = entries
                    .iter()
                    .map(|value| graph.root(self, value))
                    .collect();
                let prototype = self.intrinsic_builtin_prototype(Native::Set);
                let target = self.heap.alloc(Cell::Set {
                    object: Box::new(Self::empty_object(prototype)),
                    entries: Box::default(),
                });
                let target_root = graph.root(self, target);
                graph.remember(source, target_root);
                for value in entries {
                    let value = self.heap.root_value(value).unwrap();
                    let value = self.structured_clone_value(program, value, graph)?;
                    let target = self.heap.root_value(target_root).unwrap();
                    self.call_collection_native(program, Native::SetAdd, target, &[value])?;
                }
                Ok(self.heap.root_value(target_root).unwrap())
            }
            Cell::Date { milliseconds, .. } => {
                graph.root(self, source);
                let prototype = self.intrinsic_builtin_prototype(Native::Date);
                let target = self.heap.alloc(Cell::Date {
                    milliseconds,
                    object: Box::new(Self::empty_object(prototype)),
                });
                let target_root = graph.root(self, target);
                graph.remember(source, target_root);
                Ok(self.heap.root_value(target_root).unwrap())
            }
            Cell::RegExp { meta, matcher, .. } => {
                graph.root(self, source);
                let intrinsics = &self.realm.intrinsics.regexp_intrinsics[&self.realm.globals];
                let target = self.regexp_from_matcher(
                    intrinsics.prototype,
                    meta.source,
                    meta.flags,
                    matcher,
                    crate::heap::RegExpLegacyOwner::Enabled(intrinsics.constructor),
                )?;
                let target_root = graph.root(self, target);
                graph.remember(source, target_root);
                Ok(self.heap.root_value(target_root).unwrap())
            }
            Cell::String(value) => Ok(self.heap.alloc(Cell::String(value))),
            Cell::BigInt(value) => Ok(self.heap.alloc(Cell::BigInt(value))),
            Cell::Symbol(_) => {
                Err(self.data_clone_error(program, "Symbol values cannot be cloned"))
            }
            Cell::ArrayBuffer {
                bytes,
                shared,
                detached,
                max_byte_length,
                resizable,
                immutable,
                ..
            } if !shared && !detached => {
                graph.root(self, source);
                let mut copied_bytes = Vec::new();
                copied_bytes.try_reserve_exact(bytes.len()).map_err(|_| {
                    self.range_error(program, "ArrayBuffer is too large to clone".into())
                })?;
                copied_bytes.extend_from_slice(&bytes.read());
                let prototype = self.intrinsic_builtin_prototype(Native::ArrayBuffer);
                let target = self.heap.alloc(Cell::ArrayBuffer {
                    object: Box::new(Self::empty_object(prototype)),
                    bytes: copied_bytes.into(),
                    shared: false,
                    detached: false,
                    max_byte_length: if resizable {
                        max_byte_length
                    } else {
                        bytes.len()
                    },
                    resizable,
                    immutable,
                });
                let target_root = graph.root(self, target);
                graph.remember(source, target_root);
                Ok(self.heap.root_value(target_root).unwrap())
            }
            Cell::ArrayBuffer {
                bytes,
                shared: true,
                detached: false,
                max_byte_length,
                resizable,
                immutable,
                ..
            } => {
                if bytes.shared_backing().is_none() {
                    return Err(
                        self.data_clone_error(program, "SharedArrayBuffer has no shared backing")
                    );
                }
                graph.root(self, source);
                let prototype = self.intrinsic_builtin_prototype(Native::SharedArrayBuffer);
                let target = self.heap.alloc(Cell::ArrayBuffer {
                    object: Box::new(Self::empty_object(prototype)),
                    bytes,
                    shared: true,
                    detached: false,
                    max_byte_length,
                    resizable,
                    immutable,
                });
                let target_root = graph.root(self, target);
                graph.remember(source, target_root);
                Ok(self.heap.root_value(target_root).unwrap())
            }
            Cell::ArrayBuffer { detached: true, .. } => Err(self.data_clone_error(
                program,
                "An ArrayBuffer is detached and could not be cloned.",
            )),
            Cell::ArrayBuffer { .. } => {
                Err(self
                    .data_clone_error(program, "Detached or shared ArrayBuffer cannot be cloned"))
            }
            _ => Err(self.data_clone_error(program, "Value cannot be cloned")),
        }
    }

    fn structured_clone_error(
        &mut self,
        program: &ResidualProgram,
        source: Value,
        graph: &mut CloneGraph,
    ) -> Result<Value, JsError> {
        let source_root = graph.root(self, source);
        // Node snapshots both own descriptors before name/message coercion or
        // stack access can run guest code and mutate the original error.
        let mut fields = [None, None];
        for (index, name) in ["message", "cause"].into_iter().enumerate() {
            let key = self.heap.alloc(Cell::String(name.into()));
            if let Some((value, attributes)) = self.ordinary_own_property(program, source, key)?
                && !attributes.accessor
            {
                fields[index] = Some(graph.root(self, value));
            }
        }
        let [message, cause] = fields;
        let name_atom = self.intern_atom("name");
        let name = self.get_property(program, source, name_atom)?;
        let name = self.to_string(program, name)?;
        let native = match name.as_str() {
            "EvalError" => Native::EvalError,
            "RangeError" => Native::RangeError,
            "ReferenceError" => Native::ReferenceError,
            "SyntaxError" => Native::SyntaxError,
            "TypeError" => Native::TypeError,
            "URIError" => Native::URIError,
            _ => Native::Error,
        };
        let message = if let Some(root) = message {
            let value = self.heap.root_value(root).unwrap();
            let text = self.coerce_js_string(program, value)?;
            let value = self.heap.alloc(Cell::String(text));
            Some(graph.root(self, value))
        } else {
            None
        };
        let stack_atom = self.intern_atom("stack");
        let source = self.heap.root_value(source_root).unwrap();
        let stack = self.get_property(program, source, stack_atom)?;
        let stack = if matches!(self.heap.get(stack), Some(Cell::String(_))) {
            stack
        } else {
            Value::UNDEFINED
        };
        let stack = graph.root(self, stack);
        let prototype = self.intrinsic_builtin_prototype(native);
        let target = self.heap.alloc(Cell::Object(Object::error(prototype)));
        let target_root = graph.root(self, target);
        graph.remember(source, target_root);
        let stack = self.heap.root_value(stack).unwrap();
        self.install_error_stack(program, target, StackData::Formatted(stack))?;
        if let Some(message) = message {
            let message = self.heap.root_value(message).unwrap();
            self.set_builtin_value_named(target, "message", message)?;
        }
        if let Some(cause) = cause {
            let cause = self.heap.root_value(cause).unwrap();
            let cause = self.structured_clone_value(program, cause, graph)?;
            let target = self.heap.root_value(target_root).unwrap();
            self.set_builtin_value_named(target, "cause", cause)?;
        }
        Ok(self.heap.root_value(target_root).unwrap())
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
                    .ordinary_own_property(program, self.heap.root_value(source).unwrap(), key)?
                    .is_some_and(|(_, attributes)| attributes.enumerable)
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
        JsError::data_clone(error, message)
    }
}
