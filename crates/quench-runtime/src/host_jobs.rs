use std::{cell::RefCell, rc::Rc};

use crate::execute::VmError;

type HostJobPump = Rc<dyn Fn() -> Result<bool, VmError>>;

thread_local! {
    static PUMP: RefCell<Option<HostJobPump>> = const { RefCell::new(None) };
}

pub fn install_host_job_pump(pump: HostJobPump) {
    PUMP.with(|slot| *slot.borrow_mut() = Some(pump));
}

pub(crate) fn pump_host_job() -> Result<bool, VmError> {
    PUMP.with(|slot| match slot.borrow().as_ref() {
        Some(pump) => pump(),
        None => Ok(false),
    })
}
