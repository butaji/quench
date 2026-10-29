//! Object templates: the graph one construction built, copied in place of running the
//! construction again (`Vm::instantiate_object_template`).
//!
//! A host captures a template from two constructions of the same call (`capture`). Each
//! slot both hold the same value of is shared by every copy; each slot holding a different
//! object of each is an object the construction makes anew, copied in turn. A copy has the
//! template's shapes, prototypes and values, so its keys, their order and attributes are a
//! construction's by identity of layout.
//!
//! A copy equals a new construction only while what the construction read still holds:
//! - the layout of each object on the prototype chains of the graph's objects and of the
//!   probes' holders: its shape (keys, attributes, accessors) and its prototype, which every
//!   key the construction looked up or assigned through the chain depends on;
//! - each probe, an own data property of a holder the host names, value for value.
//!
//! Any other read must be immutable for the host's construction, which the host states by
//! its choice of probes.
//!
//! A template's parameters are own data properties of its root that each copy takes from
//! its caller instead (`instantiate_object_template`'s arguments): the values a host's
//! construction assigns per instance, which the two constructions need not share. The template roots every object it guards, so none is freed and
//! its record reused, and remaps their shapes when shapes are compacted
//! (`remap_object_template_shapes`).
use super::property_key::PropertyKey;
use super::{Host, Vm};
use crate::heap::{Cell, RootId};
use crate::value::Value;
use smallvec::SmallVec;

/// The handle a capture returns: the template's index in `Vm::object_templates`.
pub(crate) type ObjectTemplateId = u32;

/// The graph objects a copy makes without a heap allocation of its own bookkeeping.
const INLINE_COPIES: usize = 8;
/// The slots of one object a copy assembles without a heap allocation.
const INLINE_SLOTS: usize = 64;

/// One object of a template's graph.
struct TemplateNode {
    /// The captured object, which no guest code reaches, so its shape stays the
    /// construction's.
    source: Value,
    prototype: Value,
    /// The captured object's values, a slot holding an object each copy makes anew
    /// standing for it until a copy writes the copy's.
    values: Box<[Value]>,
    /// Slots holding objects each copy makes anew, and the node (earlier in the graph's
    /// order) each copies.
    fresh: Box<[(u32, u32)]>,
    /// Whether the object is an empty array: a copy is a new empty array of its prototype.
    array: bool,
}

/// An object whose layout a template's construction read, as it was at capture.
struct TemplateLayout {
    object: Value,
    shape: u32,
    prototype: Value,
}

/// An own data property a construction reads, at the slot its holder's guarded shape
/// (`TemplateLayout`) keeps it in, and its value at capture; `None` slot for an absent key.
struct TemplateProbe {
    holder: Value,
    slot: Option<u32>,
    value: Value,
}

pub(crate) struct ObjectTemplate {
    /// Children before parents; the root last.
    nodes: Box<[TemplateNode]>,
    layouts: Box<[TemplateLayout]>,
    probes: Box<[TemplateProbe]>,
    /// The root's slots each copy takes from its arguments, in the order the capture named
    /// their keys.
    parameters: Box<[u32]>,
    /// The `prototype_layout_epoch` under which the layouts last held, when every guarded
    /// object is a marked prototype, whose layout changes advance the epoch: while it holds,
    /// so do they. `None` when one could not be marked, so each copy checks them.
    layouts_proved: Option<std::cell::Cell<u64>>,
    /// Keeps the graph and every guarded object and probed value alive, so each compares
    /// by identity for the template's lifetime.
    #[expect(dead_code, reason = "held for their lifetime, never read")]
    roots: Box<[RootId]>,
}

impl<H: Host> Vm<H> {
    /// A template of `template`'s graph, found against `witness`, which a second
    /// construction of the same call built, with `probes` (holder, key) read now. `None`
    /// when the two differ other than by objects each made anew, a node is not a plain
    /// extensible record or an empty array, or a guarded layout or probe is not one a
    /// shape alone describes. `parameters` names own data properties of `template` that
    /// each copy takes from its arguments; the two constructions may hold anything there.
    pub(crate) fn capture_object_template(
        &mut self,
        template: Value,
        witness: Value,
        probes: &[(Value, Value)],
        parameters: &[Value],
    ) -> Option<ObjectTemplateId> {
        let mut slots = Vec::with_capacity(parameters.len());
        for &key in parameters {
            let key = self.template_probe_key(key)?;
            let data = self.heap.object(template)?;
            let shape = data.shape();
            let slot = self.property_shape_slot(shape, key)?;
            if self.shape_may_have_accessors(shape) && self.slot_attributes(data, slot)?.accessor {
                return None;
            }
            slots.push(u32::try_from(slot).ok()?);
        }
        let mut nodes = Vec::new();
        let mut seen = Vec::new();
        self.template_node(template, witness, &slots, &mut nodes, &mut seen)?;
        let mut layouts: Vec<TemplateLayout> = Vec::new();
        for node in &nodes {
            self.guard_template_chain(node.prototype, &mut layouts)?;
        }
        let mut captured = Vec::with_capacity(probes.len());
        for &(holder, key) in probes {
            let key = self.template_probe_key(key)?;
            self.guard_template_layout(holder, &mut layouts)?;
            let prototype = self.template_layout_object(holder)?.proto();
            self.guard_template_chain(prototype, &mut layouts)?;
            let data = self.template_layout_object(holder)?;
            let shape = data.shape();
            let slot = self.property_shape_slot(shape, key);
            let value = match slot {
                Some(slot) => {
                    if self.shape_may_have_accessors(shape) && self.slot_attributes(data, slot)?.accessor {
                        return None;
                    }
                    self.heap.property_get(data, slot)?
                }
                None => Value::DELETED,
            };
            captured.push(TemplateProbe { holder, slot: slot.map(u32::try_from).transpose().ok()?, value });
        }
        // Marked prototypes advance the epoch on any change to their layout.
        for layout in &layouts {
            self.heap.mark_prototype(layout.object);
        }
        let marked = layouts
            .iter()
            .all(|layout| self.template_layout_object(layout.object).is_some_and(|object| object.was_used_as_prototype()));
        let layouts_proved = marked.then(|| std::cell::Cell::new(crate::heap::prototype_layout_epoch()));
        let mut kept = vec![template];
        kept.extend(layouts.iter().map(|layout| layout.object));
        kept.extend(captured.iter().map(|probe| probe.value));
        let roots = kept.into_iter().map(|value| self.root(value)).collect();
        let id = ObjectTemplateId::try_from(self.object_templates.len()).ok()?;
        self.object_templates.push(ObjectTemplate {
            nodes: nodes.into_boxed_slice(),
            layouts: layouts.into_boxed_slice(),
            probes: captured.into_boxed_slice(),
            parameters: slots.into_boxed_slice(),
            layouts_proved,
            roots,
        });
        Some(id)
    }

    /// A copy of template `id`'s graph with `arguments` in its parameters, or `None` when a
    /// read of its construction no longer holds or the arguments are not one per parameter;
    /// the caller then constructs.
    pub(crate) fn instantiate_object_template(&mut self, id: ObjectTemplateId, arguments: &[Value]) -> Option<Value> {
        let template = self.object_templates.get(id as usize)?;
        if template.parameters.len() != arguments.len() || !self.template_reads_hold(template) {
            return None;
        }
        let root = template.nodes.len().checked_sub(1)?;
        let mut copies: SmallVec<[Value; INLINE_COPIES]> = SmallVec::new();
        let mut values: SmallVec<[Value; INLINE_SLOTS]> = SmallVec::new();
        for index in 0..self.object_templates[id as usize].nodes.len() {
            let node = &self.object_templates[id as usize].nodes[index];
            let copy = if node.array {
                let prototype = node.prototype;
                self.new_array_with_prototype(Vec::new(), prototype)
            } else {
                // A captured object's shape is its own: its id is renumbered with the others.
                let (prototype, shape) = (node.prototype, self.heap.object(node.source)?.shape());
                values.clear();
                values.extend_from_slice(&node.values);
                for &(slot, child) in &node.fresh {
                    values[slot as usize] = copies[child as usize];
                }
                if index == root {
                    for (&slot, &argument) in self.object_templates[id as usize].parameters.iter().zip(arguments) {
                        values[slot as usize] = argument;
                    }
                }
                self.heap.alloc_object_with_properties(prototype, shape, &values)
            };
            copies.push(copy);
        }
        copies.last().copied()
    }

    /// Renumbers the shapes templates guard as shape compaction renumbers every live one.
    pub(super) fn remap_object_template_shapes(&mut self, mapping: &[u32]) {
        for layout in self.object_templates.iter_mut().flat_map(|template| template.layouts.iter_mut()) {
            layout.shape = mapping[layout.shape as usize];
        }
    }

    fn template_reads_hold(&self, template: &ObjectTemplate) -> bool {
        let epoch = crate::heap::prototype_layout_epoch();
        let proved = template.layouts_proved.as_ref().is_some_and(|proved| proved.get() == epoch);
        let layouts = proved
            || template.layouts.iter().all(|layout| {
                self.template_layout_object(layout.object)
                    .is_some_and(|object| object.shape() == layout.shape && object.proto() == layout.prototype)
            });
        if layouts && let Some(proved) = &template.layouts_proved {
            proved.set(epoch);
        }
        layouts && template.probes.iter().all(|probe| {
            let Some(slot) = probe.slot else {
                return true;
            };
            self.template_layout_object(probe.holder)
                .and_then(|object| self.heap.property_get(object, slot as usize))
                == Some(probe.value)
        })
    }

    /// Adds the node of `template` (and of each object it holds anew) to `nodes`, children
    /// first; its index. `seen` maps the graph's objects to their nodes, so an object the
    /// graph holds twice is copied once. `parameters` are slots of this node that copies take
    /// from their arguments, compared to nothing.
    fn template_node(
        &self,
        template: Value,
        witness: Value,
        parameters: &[u32],
        nodes: &mut Vec<TemplateNode>,
        seen: &mut Vec<(Value, Value, u32)>,
    ) -> Option<u32> {
        if let Some(&(_, paired, index)) = seen.iter().find(|(source, ..)| *source == template) {
            return (paired == witness).then_some(index);
        }
        let (object, other) = (self.heap.object(template)?, self.heap.object(witness)?);
        let shape = object.exact_shape();
        let prototype = object.proto();
        if shape != other.exact_shape()
            || prototype != other.proto()
            || self.shape_is_dictionary(shape)
            || !self.template_record(template)
            || !self.template_record(witness)
        {
            return None;
        }
        let array = object.is_array();
        let len = self.shapes[shape as usize].storage_len();
        if array && (len != 0 || !self.empty_array(template) || !self.empty_array(witness)) {
            return None;
        }
        let mut values = Vec::with_capacity(len);
        let mut fresh = Vec::new();
        for slot in 0..len {
            let (object, other) = (self.heap.object(template)?, self.heap.object(witness)?);
            let (value, paired) = (self.heap.property_get(object, slot)?, self.heap.property_get(other, slot)?);
            let parameter = parameters.iter().any(|&parameter| parameter as usize == slot);
            if value != paired && !parameter {
                fresh.push((u32::try_from(slot).ok()?, self.template_node(value, paired, &[], nodes, seen)?));
            }
            values.push(value);
        }
        let index = u32::try_from(nodes.len()).ok()?;
        nodes.push(TemplateNode {
            source: template,
            prototype,
            values: values.into_boxed_slice(),
            fresh: fresh.into_boxed_slice(),
            array,
        });
        seen.push((template, witness, index));
        Some(index)
    }

    /// Guards `object` and each object on its prototype chain (`TemplateLayout`).
    fn guard_template_chain(&self, mut object: Value, layouts: &mut Vec<TemplateLayout>) -> Option<()> {
        while !object.is_null() {
            self.guard_template_layout(object, layouts)?;
            object = self.template_layout_object(object)?.proto();
        }
        Some(())
    }

    fn guard_template_layout(&self, object: Value, layouts: &mut Vec<TemplateLayout>) -> Option<()> {
        if layouts.iter().any(|layout| layout.object == object) {
            return Some(());
        }
        let data = self.template_layout_object(object)?;
        // A dictionary keeps its shape across changes, and side descriptors and exotic
        // objects keep properties no shape describes.
        if self.shape_is_dictionary(data.exact_shape()) || data.has_side_descriptors() || data.is_module_namespace() {
            return None;
        }
        layouts.push(TemplateLayout { object, shape: data.shape(), prototype: data.proto() });
        Some(())
    }

    /// The ordinary object part of `value`: a function's own, else the object's.
    fn template_layout_object(&self, value: Value) -> Option<&crate::heap::Object> {
        match self.heap.function(value) {
            Some(function) => Some(&function.object),
            None => self.heap.object(value),
        }
    }

    /// Whether `value` is an object a template copies as its record: an extensible plain
    /// object or array, never a prototype, whose shape describes every own property.
    fn template_record(&self, value: Value) -> bool {
        let is_record = matches!(self.heap.get(value), Some(Cell::Object(_) | Cell::Array { .. }))
            || value.direct_object_index().is_some()
            || value.direct_array_index().is_some();
        is_record
            && self.heap.object(value).is_some_and(|object| {
                object.is_extensible()
                    && !object.was_used_as_prototype()
                    && !object.has_side_descriptors()
                    && !object.is_module_namespace()
            })
    }

    fn empty_array(&self, value: Value) -> bool {
        self.heap.array_elements(value).is_some_and(|elements| elements.len() == 0)
            && self.heap.sparse_length(value).is_none_or(|length| length == 0)
    }

    fn template_probe_key(&mut self, key: Value) -> Option<PropertyKey> {
        if let Some(atom) = self.string_key_atom(key) {
            return Some(PropertyKey::string(atom));
        }
        matches!(self.heap.get(key), Some(Cell::Symbol(_))).then_some(PropertyKey::symbol(key))
    }
}
