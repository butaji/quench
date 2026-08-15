#[derive(Debug, Clone, PartialEq)]
pub struct ArrayData {
    values: Vec<Value>,
    length: usize,
    properties: Vec<(String, Value)>,
    descriptors: Vec<(String, Value)>,
    arguments: bool,
    strict_arguments: bool,
    mapped: Vec<Option<Rc<RefCell<Value>>>>,
    deleted: Vec<bool>,
    argument_live: Option<Rc<RefCell<ArgumentLive>>>,
}

#[derive(Debug, Clone, PartialEq)]
struct ArgumentLive {
    values: Vec<Value>,
    length: usize,
    mapped: Vec<Option<Rc<RefCell<Value>>>>,
    deleted: Vec<bool>,
}

impl ArrayData {
    pub fn new(values: Vec<Value>) -> Self {
        let length = values.len();
        Self {
            values,
            length,
            properties: Vec::new(),
            descriptors: Vec::new(),
            arguments: false,
            strict_arguments: false,
            mapped: Vec::new(),
            deleted: Vec::new(),
        argument_live: Some(Rc::new(RefCell::new(ArgumentLive {
            values: values.clone(),
            length,
            mapped: Vec::new(),
            deleted: Vec::new(),
        }))),
        }
    }

    pub(crate) fn new_arguments(values: Vec<Value>, strict: bool) -> Self {
        let mut data = Self::new(values);
        data.arguments = true;
        data.strict_arguments = strict;
        data.argument_live = Some(Rc::new(RefCell::new(ArgumentLive {
            values: data.values.clone(),
            length: data.length,
            mapped: data.mapped.clone(),
            deleted: data.deleted.clone(),
        })));
        data
    }

    pub(crate) fn is_arguments(&self) -> bool {
        self.arguments
    }

    pub(crate) fn is_strict_arguments(&self) -> bool {
        self.strict_arguments
    }

    pub fn logical_len(&self) -> usize {
        self.argument_live
            .as_ref()
            .map_or(self.length, |live| live.borrow().length)
    }

    pub fn set_length(&mut self, length: usize) {
        if let Some(live) = &self.argument_live {
            let mut live = live.borrow_mut();
            live.values.truncate(length);
            live.deleted.truncate(length);
            live.mapped.truncate(length);
            live.length = length;
        }
        if length < self.length {
            self.values.truncate(length);
            self.deleted.truncate(length);
            self.mapped.truncate(length);
        }
        self.length = length;
    }

    pub fn set_index(&mut self, index: usize, value: Value) {
        if let Some(live) = &self.argument_live {
            let mut live = live.borrow_mut();
            set_live_index(&mut live, index, value.clone());
        }
        if let Some(Some(binding)) = self.mapped.get(index) {
            *binding.borrow_mut() = value.clone();
        }
        if self.values.len() <= index {
            self.values
                .resize(index.saturating_add(1), Value::Undefined);
        }
        self.values[index] = value;
        if self.deleted.len() <= index {
            self.deleted.resize(index.saturating_add(1), false);
        }
        self.deleted[index] = false;
        self.length = self.length.max(index.saturating_add(1));
    }

    pub(crate) fn values_mut(&mut self) -> &mut [Value] {
        &mut self.values
    }

    pub(crate) fn get_index(&self, index: usize) -> Option<Value> {
        if let Some(live) = &self.argument_live {
            return live_index(&live.borrow(), index);
        }
        if self.deleted.get(index) == Some(&true) {
            return None;
        }
        self.mapped
            .get(index)
            .and_then(Option::as_ref)
            .map(|binding| binding.borrow().clone())
            .or_else(|| self.values.get(index).cloned())
    }

    pub(crate) fn has_index(&self, index: usize) -> bool {
        if let Some(live) = &self.argument_live {
            let live = live.borrow();
            return index < live.length
                && live.deleted.get(index) != Some(&true)
                && (index < live.values.len()
                    || live.mapped.get(index).and_then(Option::as_ref).is_some());
        }
        index < self.length
            && self.deleted.get(index) != Some(&true)
            && (index < self.values.len()
                || self
                    .mapped
                    .get(index)
                    .and_then(Option::as_ref)
                    .is_some())
    }

    pub(crate) fn snapshot(&self) -> Vec<Value> {
        (0..self.length)
            .map(|index| self.get_index(index).unwrap_or(Value::Undefined))
            .collect()
    }

    pub(crate) fn map_index(&mut self, index: usize, binding: Rc<RefCell<Value>>) {
        if let Some(live) = &self.argument_live {
            let mut live = live.borrow_mut();
            live.mapped.resize(index.saturating_add(1), None);
            live.mapped[index] = Some(Rc::clone(&binding));
        }
        self.mapped.resize(index.saturating_add(1), None);
        self.mapped[index] = Some(binding);
    }

    pub(crate) fn disconnect_index(&mut self, index: usize) {
        if let Some(live) = &self.argument_live {
            if let Some(mapping) = live.borrow_mut().mapped.get_mut(index) {
                *mapping = None;
            }
        }
        if let Some(mapping) = self.mapped.get_mut(index) {
            *mapping = None;
        }
    }

    pub(crate) fn descriptor(&self, key: &str) -> Option<Value> {
        self.descriptors
            .iter()
            .rev()
            .find_map(|(name, value)| (name == key).then(|| value.clone()))
    }

    pub(crate) fn define_descriptor(&mut self, key: &str, descriptor: Value) {
        self.descriptors.retain(|(name, _)| name != key);
        self.descriptors.push((key.to_string(), descriptor));
    }

    pub(crate) fn descriptor_keys(&self) -> Vec<String> {
        self.descriptors.iter().map(|(key, _)| key.clone()).collect()
    }

    pub(crate) fn property(&self, key: &str) -> Option<Value> {
        self.properties
            .iter()
            .rev()
            .find_map(|(name, value)| (name == key).then(|| value.clone()))
    }

    pub(crate) fn property_keys(&self) -> Vec<String> {
        self.properties.iter().map(|(key, _)| key.clone()).collect()
    }

    pub(crate) fn set_property(&mut self, key: &str, value: Value) {
        if let Some((_, current)) = self
            .properties
            .iter_mut()
            .rev()
            .find(|(name, _)| name == key)
        {
            *current = value;
        } else {
            self.properties.push((key.to_string(), value));
        }
        self.sync_descriptor_value(key);
    }

    fn sync_descriptor_value(&mut self, key: &str) {
        let value = self.property(key);
        let Some((_, Value::Object(descriptor))) = self
            .descriptors
            .iter_mut()
            .rev()
            .find(|(name, _)| name == key)
        else {
            return;
        };
        if let Some((_, current)) = Rc::make_mut(descriptor)
            .iter_mut()
            .find(|(name, _)| name == "value")
        {
            *current = value.unwrap_or(Value::Undefined);
        }
    }

    pub(crate) fn delete_property(&mut self, key: &str) {
        self.properties.retain(|(name, _)| name != key);
        self.descriptors.retain(|(name, _)| name != key);
        if let Some(index) = crate::arrays::array_index(key) {
            let index = index as usize;
            self.disconnect_index(index);
            self.deleted.resize(index.saturating_add(1), false);
            self.deleted[index] = true;
            if let Some(live) = &self.argument_live {
                let mut live = live.borrow_mut();
                live.deleted.resize(index.saturating_add(1), false);
                live.deleted[index] = true;
            }
        }
    }
}

fn set_live_index(live: &mut ArgumentLive, index: usize, value: Value) {
    if let Some(Some(binding)) = live.mapped.get(index) {
        *binding.borrow_mut() = value.clone();
    }
    if live.values.len() <= index {
        live.values.resize(index.saturating_add(1), Value::Undefined);
    }
    live.values[index] = value;
    if live.deleted.len() <= index {
        live.deleted.resize(index.saturating_add(1), false);
    }
    live.deleted[index] = false;
    live.length = live.length.max(index.saturating_add(1));
}

fn live_index(live: &ArgumentLive, index: usize) -> Option<Value> {
    if live.deleted.get(index) == Some(&true) {
        return None;
    }
    live.mapped
        .get(index)
        .and_then(Option::as_ref)
        .map(|binding| binding.borrow().clone())
        .or_else(|| live.values.get(index).cloned())
}

impl std::ops::Deref for ArrayData {
    type Target = [Value];

    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl Value {
    /// Create an ordinary JavaScript object from own data properties.
    pub fn object(properties: ObjectProperties) -> Self {
        Self::Object(Rc::new(ObjectData::new(properties)))
    }

    pub(crate) fn array(values: Vec<Value>) -> Self {
        Self::Array(Rc::new(ArrayData::new(values)))
    }
}
