use super::collections::canonicalize_keyed_collection_key;
use super::*;

#[derive(Clone, Copy)]
pub(super) enum GroupByKind {
    Object,
    Map,
}

impl<H: Host> Vm<H> {
    pub(super) fn group_by(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        kind: GroupByKind,
    ) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        let callback = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        self.require_object_coercible(p, source)?;
        if !self.is_function(callback) {
            return Err(self.type_error(p, "groupBy callback is not callable".into()));
        }
        let source = self.heap.root(source);
        let callback = self.heap.root(callback);
        let mut iterator_root = None;
        let mut next_root = None;
        let mut result_root = None;
        let outcome = (|| {
            let value = self.heap.root_value(source).unwrap();
            let iterator = self.get_iterator(p, value)?;
            let iterator = self.heap.root(iterator);
            iterator_root = Some(iterator);
            let next_atom = self.intern_atom("next");
            let receiver = self.heap.root_value(iterator).unwrap();
            let next = self.get_property(p, receiver, next_atom)?;
            let next = self.heap.root(next);
            next_root = Some(next);
            let result = match kind {
                GroupByKind::Object => Cell::Object(Self::empty_object(Value::NULL)),
                GroupByKind::Map => Cell::Map {
                    object: Self::empty_object(
                        self.realm
                            .intrinsics
                            .builtin_prototypes
                            .get(&(self.realm.globals, Native::Map))
                            .copied()
                            .unwrap_or(self.map_proto),
                    ),
                    entries: Vec::new(),
                },
            };
            let result = self.heap.alloc(result);
            let result = self.heap.root(result);
            result_root = Some(result);
            let mut index = 0u64;
            loop {
                if index >= MAX_SAFE_INTEGER as u64 {
                    let error = self
                        .type_error(p, "groupBy iteration exceeds the safe integer limit".into());
                    let receiver = self.heap.root_value(iterator).unwrap();
                    return Err(self.iterator_abrupt(p, receiver, error));
                }
                let Some(value) = self.rooted_iterator_step_value(p, iterator, next)? else {
                    return Ok(self.heap.root_value(result).unwrap());
                };
                let value = self.heap.root(value);
                let mut key_root = None;
                let entry = (|| {
                    let element = self.heap.root_value(value).unwrap();
                    let callback = self.heap.root_value(callback).unwrap();
                    let key = self.call_value(
                        p,
                        callback,
                        Value::UNDEFINED,
                        &[element, Value::number(index as f64)],
                    )?;
                    let root = self.heap.root(key);
                    key_root = Some(root);
                    let key = match kind {
                        GroupByKind::Object => self.to_property_key(p, key)?,
                        GroupByKind::Map => canonicalize_keyed_collection_key(key),
                    };
                    self.heap.update_root(root, key);
                    let output = self.heap.root_value(result).unwrap();
                    let key = self.heap.root_value(root).unwrap();
                    let element = self.heap.root_value(value).unwrap();
                    self.add_value_to_group(p, kind, output, key, element)
                })();
                let entry = entry.map_err(|error| {
                    let receiver = self.heap.root_value(iterator).unwrap();
                    self.iterator_abrupt(p, receiver, error)
                });
                for root in [Some(value), key_root].into_iter().flatten() {
                    self.heap.release_root(root);
                }
                entry?;
                index += 1;
            }
        })();
        for root in [
            Some(source),
            Some(callback),
            iterator_root,
            next_root,
            result_root,
        ]
        .into_iter()
        .flatten()
        {
            self.heap.release_root(root);
        }
        outcome
    }

    fn add_value_to_group(
        &mut self,
        p: &ResidualProgram,
        kind: GroupByKind,
        result: Value,
        key: Value,
        value: Value,
    ) -> Result<(), JsError> {
        let group = match kind {
            GroupByKind::Object => match self.heap.get(key).cloned() {
                Some(Cell::Symbol(_)) => self.symbol_property(result, key),
                Some(Cell::String(name)) => {
                    let atom = self.intern_js_atom(&name);
                    self.own_property(result, atom)
                }
                _ => unreachable!("ToPropertyKey returns a string or symbol"),
            },
            GroupByKind::Map => self.map_entry_index(result, key).map(|index| {
                let Some(Cell::Map { entries, .. }) = self.heap.get(result) else {
                    unreachable!("groupBy owns a Map result")
                };
                entries[index].1
            }),
        };
        if let Some(group) = group {
            let Some(Cell::Array { elements, .. }) = self.heap.get_mut(group) else {
                unreachable!("groupBy owns array groups")
            };
            Rc::make_mut(elements).push(value);
        } else {
            let group = self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_prototype_for_realm(self.realm.globals)),
                elements: Rc::new(vec![value]),
            });
            match kind {
                GroupByKind::Object => match self.heap.get(key).cloned() {
                    Some(Cell::Symbol(_)) => {
                        self.set_index(p, result, key, group)?;
                    }
                    Some(Cell::String(name)) => {
                        let atom = self.intern_js_atom(&name);
                        self.set_property(result, atom, group)?;
                    }
                    _ => unreachable!("ToPropertyKey returns a string or symbol"),
                },
                GroupByKind::Map => {
                    let Some(Cell::Map { entries, .. }) = self.heap.get_mut(result) else {
                        unreachable!("groupBy owns a Map result")
                    };
                    entries.push((key, group));
                }
            }
        }
        Ok(())
    }
}
