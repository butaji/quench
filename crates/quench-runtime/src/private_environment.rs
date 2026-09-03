use std::{cell::RefCell, collections::HashMap, rc::Rc};

use crate::{
    completion::Completion, execute::VmError, facts::PrivateNameId, ops::Op, value::PrivateName,
};

/// The lexical capabilities visible while evaluating a class definition.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PrivateEnvironment {
    names: Rc<HashMap<PrivateNameId, PrivateName>>,
    labels: Rc<HashMap<String, PrivateNameId>>,
}

impl PrivateEnvironment {
    fn extend(&self, definitions: &[PrivateNameId], labels: &[String]) -> Self {
        let mut names = self.names.as_ref().clone();
        let mut by_label = self.labels.as_ref().clone();
        for (index, definition) in definitions.iter().enumerate() {
            if let Some(label) = labels.get(index) {
                names.insert(*definition, PrivateName::new(*definition, label));
                by_label.insert(label.clone(), *definition);
            } else {
                names.insert(*definition, PrivateName::new(*definition, ""));
            }
        }
        Self {
            names: Rc::new(names),
            labels: Rc::new(by_label),
        }
    }

    pub(crate) fn resolve(&self, name: PrivateNameId) -> Option<PrivateName> {
        self.names.get(&name).cloned()
    }

    pub(crate) fn id_for_label(&self, label: &str) -> Option<PrivateNameId> {
        self.labels.get(label).copied().or_else(|| self.only_id())
    }

    pub(crate) fn has_names(&self) -> bool {
        !self.names.is_empty()
    }

    fn only_id(&self) -> Option<PrivateNameId> {
        if self.names.len() == 1 {
            self.names.keys().next().copied()
        } else {
            None
        }
    }
}

thread_local! {
    static CURRENT: RefCell<Option<PrivateEnvironment>> = const { RefCell::new(None) };
    static SUSPENDED: RefCell<Option<PrivateEnvironment>> = const { RefCell::new(None) };
}

/// Takes the environment captured by a class body that just suspended on `yield`.
pub(crate) fn take_suspended() -> Option<PrivateEnvironment> {
    SUSPENDED.with(|slot| slot.take())
}

/// Restores the private environment even when class evaluation completes abruptly.
pub(crate) struct Guard {
    previous: Option<PrivateEnvironment>,
}

impl Guard {
    pub(crate) fn install(definitions: &[PrivateNameId], labels: &[String]) -> Self {
        let environment = current().extend(definitions, labels);
        Self::install_environment(environment)
    }

    pub(crate) fn install_environment(environment: PrivateEnvironment) -> Self {
        let previous = CURRENT.with(|current| current.replace(Some(environment)));
        Self { previous }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        CURRENT.with(|current| current.replace(self.previous.take()));
    }
}

/// Returns an Rc-backed snapshot so nested scopes can copy the lexical mapping.
pub(crate) fn current() -> PrivateEnvironment {
    CURRENT
        .with(|current| current.borrow().clone())
        .unwrap_or_default()
}

pub(crate) fn resolve(name: PrivateNameId) -> Option<PrivateName> {
    current().resolve(name)
}

pub(crate) fn execute_scope(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
) -> Result<Completion, VmError> {
    let Op::PrivateScope {
        names,
        labels,
        class_name,
        body,
    } = op
    else {
        return Err(VmError::MissingReturn);
    };
    let _scope = Guard::install(names, labels);
    let _class_name = class_name.as_deref().map(ClassNameGuard::install);
    let completion = crate::vm::execute_function_code_completion_in_current_frame(body, registers)?;
    if completion.is_suspension() {
        SUSPENDED.with(|slot| *slot.borrow_mut() = Some(current()));
    }
    Ok(completion)
}

struct ClassNameGuard {
    _environment: crate::locals::EnvironmentGuard,
    name: String,
}

impl ClassNameGuard {
    fn install(name: &str) -> Self {
        let parent = crate::locals::current();
        let child = crate::environment::Environment::child(&parent, Vec::new());
        let cell = crate::value::BindingCell::new(crate::value::Value::Undefined);
        child.alias_binding(name, cell);
        child.mark_immutable(name);
        crate::locals::begin_class_name(name);
        Self {
            _environment: crate::locals::EnvironmentGuard::install(child),
            name: name.to_string(),
        }
    }
}

impl Drop for ClassNameGuard {
    fn drop(&mut self) {
        crate::locals::end_class_name(&self.name);
    }
}
