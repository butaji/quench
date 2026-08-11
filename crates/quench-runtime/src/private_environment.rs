use std::{cell::RefCell, collections::HashMap, rc::Rc};

use crate::{
    completion::Completion, execute::VmError, facts::PrivateNameId, ops::Op, value::PrivateName,
};

/// The lexical capabilities visible while evaluating a class definition.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PrivateEnvironment {
    names: Rc<HashMap<PrivateNameId, PrivateName>>,
}

impl PrivateEnvironment {
    fn extend(&self, definitions: &[PrivateNameId]) -> Self {
        let mut names = self.names.as_ref().clone();
        for definition in definitions {
            names.insert(*definition, PrivateName::new(*definition));
        }
        Self {
            names: Rc::new(names),
        }
    }

    pub(crate) fn resolve(&self, name: PrivateNameId) -> Option<PrivateName> {
        self.names.get(&name).cloned()
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
    pub(crate) fn install(definitions: &[PrivateNameId]) -> Self {
        let environment = current().extend(definitions);
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
    registers: &mut Vec<crate::value::Value>,
    op: &Op,
) -> Result<Completion, VmError> {
    let Op::PrivateScope { names, body } = op else {
        return Err(VmError::MissingReturn);
    };
    let _scope = Guard::install(names);
    let completion = crate::execute::execute_completion_in_place(body, registers)?;
    if matches!(completion, Completion::Yield(_)) {
        SUSPENDED.with(|slot| *slot.borrow_mut() = Some(current()));
    }
    Ok(completion)
}
