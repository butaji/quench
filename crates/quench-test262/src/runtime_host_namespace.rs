impl LinkedModule {
    fn sync_namespace_object(&self, object: &quench_runtime::value::Value) -> bool {
        let quench_runtime::value::Value::Object(properties) = object else {
            return false;
        };
        for name in self.export_names() {
            let Some(export) = self.export_cell(&name) else {
                continue;
            };
            sync_export_cells(properties.iter(), &name, export.get());
        }
        true
    }

    fn refresh_namespace(&self) {
        if let Some(cell) = self.namespace_cell.borrow().clone() {
            self.write_namespace_cell(&cell);
        }
        if let Some(cell) = self.deferred_namespace_cell.borrow().clone() {
            self.write_namespace_cell(&cell);
        }
    }

    fn write_namespace_cell(&self, cell: &ModuleBindingCell) {
        if self.sync_namespace_object(&cell.get()) {
            return;
        }
        let previous = cell.get();
        let next = quench_runtime::value::Value::object(self.namespace_properties());
        cell.set(next.clone());
        quench_runtime::module_bindings::rehome_evaluator(&previous, &next);
    }

    fn namespace_properties(&self) -> Vec<(String, quench_runtime::value::Value)> {
        let mut properties: Vec<(String, quench_runtime::value::Value)> = self
            .export_names()
            .iter()
            .filter_map(|name| self.export_cell(name).map(|value| (name.clone(), value)))
            .map(|(name, value)| {
                (
                    name,
                    quench_runtime::value::Value::BindingCell(value.shared()),
                )
            })
            .collect();
        properties.push((
            "\0prototype".to_string(),
            quench_runtime::value::Value::Null,
        ));
        properties.push((
            "\0quench:non_extensible".to_string(),
            quench_runtime::value::Value::Boolean(true),
        ));
        quench_runtime::module_bindings::mark_namespace(&mut properties);
        for name in self.export_names() {
            if let Some(cell) = self.export_cell(&name) {
                properties.push(export_descriptor(&name, cell));
            }
        }
        properties
    }
}

fn sync_export_cells<'a, I>(
    properties: I,
    name: &str,
    live: quench_runtime::value::Value,
) where
    I: IntoIterator<Item = (&'a quench_runtime::value::PropertyName, &'a quench_runtime::value::Value)>,
{
    let descriptor = format!("\0quench:descriptor:\0{name}");
    for (key, value) in properties {
        if key.as_ref() == name {
            if let quench_runtime::value::Value::BindingCell(cell) = value {
                *cell.borrow_mut() = live.clone();
            }
        }
        if key.as_ref() == descriptor {
            sync_descriptor_value(value, &live);
        }
    }
}

fn sync_descriptor_value(value: &quench_runtime::value::Value, live: &quench_runtime::value::Value) {
    let quench_runtime::value::Value::Object(descriptor) = value else {
        return;
    };
    for (field, field_value) in descriptor.iter() {
        if field != "value" {
            continue;
        }
        if let quench_runtime::value::Value::BindingCell(cell) = field_value {
            *cell.borrow_mut() = live.clone();
        }
    }
}

fn export_descriptor(
    name: &str,
    cell: ModuleBindingCell,
) -> (String, quench_runtime::value::Value) {
    (
        format!("\0quench:descriptor:\0{name}"),
        quench_runtime::value::Value::object(vec![
            (
                "value".to_string(),
                quench_runtime::value::Value::BindingCell(cell.shared()),
            ),
            (
                "writable".to_string(),
                quench_runtime::value::Value::Boolean(true),
            ),
            (
                "enumerable".to_string(),
                quench_runtime::value::Value::Boolean(true),
            ),
            (
                "configurable".to_string(),
                quench_runtime::value::Value::Boolean(false),
            ),
        ]),
    )
}
