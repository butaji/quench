const LINKED_TASK_ID_LIMIT: usize = 64;
const LINKED_TASK_IDENTITY_SLOTS: usize = LINKED_TASK_ID_LIMIT * 2;

enum DirectTaskKind {
    Idle,
    Device,
    Worker,
    Handler,
}

struct DirectTaskRunner {
    identity: u64,
    id: *const crate::register_file::SlotWord,
    link: *const crate::register_file::SlotWord,
    state: *const crate::register_file::SlotWord,
    queue: *const crate::register_file::SlotWord,
    task_word: *const crate::register_file::SlotWord,
    task_v1: *const crate::register_file::SlotWord,
    held_mask: i32,
    suspended: i32,
    task: *const crate::value::ObjectData,
    task_value: crate::value::Value,
    scheduler: crate::value::Value,
    function: std::rc::Rc<crate::value::FunctionValue>,
    kind: DirectTaskKind,
}

impl DirectTaskRunner {
    fn word(&self, slot: *const crate::register_file::SlotWord) -> &crate::register_file::SlotWord {
        // SAFETY: the admitted linked list retains every TCB, and its exact
        // task methods only overwrite existing slots while this table lives.
        unsafe { &*slot }
    }

    fn is_held_or_suspended(&self) -> Option<bool> {
        let state = exact_i32(self.word(self.state).number()?)?;
        Some(state & self.held_mask != 0 || state == self.suspended)
    }

    fn matches(&self, task: *const crate::value::ObjectData) -> bool {
        task == self.task
    }

    fn execute(
        &self,
        packet: &crate::value::Value,
    ) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
        let arguments = std::slice::from_ref(packet);
        match self.kind {
            DirectTaskKind::Idle => execute_idle_task(&self.function, &self.task_value),
            DirectTaskKind::Device => {
                execute_device_task_words(
                    &self.function,
                    arguments,
                    self.word(self.task_v1),
                    &self.scheduler,
                )
            }
            DirectTaskKind::Worker => {
                execute_worker_task(&self.function, &self.task_value, arguments)
            }
            DirectTaskKind::Handler => {
                execute_handler_task(&self.function, &self.task_value, arguments)
            }
        }
    }
}

struct DirectTaskTable {
    runners: Vec<Option<DirectTaskRunner>>,
    identities: [Option<(u64, u8)>; LINKED_TASK_IDENTITY_SLOTS],
}

impl DirectTaskTable {
    fn insert(&mut self, id: usize, runner: DirectTaskRunner) -> Option<()> {
        let mut slot = runner.identity as usize & (LINKED_TASK_IDENTITY_SLOTS - 1);
        for _ in 0..LINKED_TASK_IDENTITY_SLOTS {
            if self.identities[slot].is_none() {
                self.identities[slot] = Some((runner.identity, id as u8));
                self.runners[id] = Some(runner);
                return Some(());
            }
            slot = (slot + 1) & (LINKED_TASK_IDENTITY_SLOTS - 1);
        }
        None
    }

    fn for_object(&self, object: &crate::value::ObjectData) -> Option<&DirectTaskRunner> {
        let identity = object.identity();
        let mut slot = identity as usize & (LINKED_TASK_IDENTITY_SLOTS - 1);
        for _ in 0..LINKED_TASK_IDENTITY_SLOTS {
            let (stored, id) = self.identities[slot]?;
            if stored == identity {
                return self.runners.get(usize::from(id))?.as_ref();
            }
            slot = (slot + 1) & (LINKED_TASK_IDENTITY_SLOTS - 1);
        }
        None
    }
}

fn linked_task_runners(start: &crate::value::Value) -> Option<DirectTaskTable> {
    let mut table = DirectTaskTable {
        runners: Vec::new(),
        identities: [None; LINKED_TASK_IDENTITY_SLOTS],
    };
    let mut cursor = start.clone();
    while let crate::value::Value::Object(ref tcb) = cursor {
        let id = exact_linked_task_id(crate::vm::proven_own_word(&tcb, "id")?.number()?)?;
        if table.runners.len() <= id {
            table.runners.resize_with(id + 1, || None);
        }
        if table.runners[id].is_some() {
            return None;
        }
        let runner = linked_task_runner(&cursor, &tcb)?;
        cursor = runner.word(runner.link).load();
        table.insert(id, runner)?;
    }
    matches!(cursor, crate::value::Value::Null).then_some(table)
}

fn linked_task_runner(
    tcb_value: &crate::value::Value,
    tcb: &crate::value::ObjectData,
) -> Option<DirectTaskRunner> {
    let id = crate::vm::proven_own_word(tcb, "id")?;
    let link = crate::vm::proven_own_word(tcb, "link")?;
    let state = crate::vm::proven_own_word(tcb, "state")?;
    let queue = crate::vm::proven_own_word(tcb, "queue")?;
    let task_word = crate::vm::proven_own_word(tcb, "task")?;
    let task = task_word.load();
    let crate::value::Value::Object(task_object) = &task else {
        return None;
    };
    if task_object.has_replacement() {
        return None;
    }
    let run = crate::execute::get_property_result(&task, "run").ok()?;
    let crate::value::Value::Function(function) = run else {
        return None;
    };
    let (held_mask, suspended) = linked_state_predicate(tcb_value)?;
    let kind = direct_task_kind(&function)?;
    let task_v1 = crate::vm::proven_own_word(task_object, "v1")?;
    let scheduler = task_scheduler(task_object)?;
    Some(DirectTaskRunner {
        identity: tcb.identity(),
        id: std::ptr::from_ref(id),
        link: std::ptr::from_ref(link),
        state: std::ptr::from_ref(state),
        queue: std::ptr::from_ref(queue),
        task_word: std::ptr::from_ref(task_word),
        task_v1: std::ptr::from_ref(task_v1),
        held_mask,
        suspended,
        task: std::rc::Rc::as_ptr(task_object),
        task_value: task,
        scheduler,
        function,
        kind,
    })
}

fn linked_state_predicate(tcb: &crate::value::Value) -> Option<(i32, i32)> {
    let predicate = crate::execute::get_property_result(tcb, "isHeldOrSuspended").ok()?;
    let crate::value::Value::Function(predicate) = predicate else {
        return None;
    };
    let ShapeKernelPlan::StatePredicate(plan) = shape_kernel_fact(&predicate)? else {
        return None;
    };
    let held = predicate
        .captures
        .get_number(plan.held_slot)
        .and_then(exact_i32)?;
    let suspended = predicate
        .captures
        .get_number(plan.suspended_slot)
        .and_then(exact_i32)?;
    Some((held, suspended))
}

fn direct_task_kind(function: &std::rc::Rc<crate::value::FunctionValue>) -> Option<DirectTaskKind> {
    if idle_task_fact(function).is_some() {
        Some(DirectTaskKind::Idle)
    } else if device_task_fact(function) {
        Some(DirectTaskKind::Device)
    } else if worker_task_fact(function).is_some() {
        Some(DirectTaskKind::Worker)
    } else if handler_task_fact(function).is_some() {
        Some(DirectTaskKind::Handler)
    } else {
        None
    }
}

fn linked_method(
    receiver: &crate::value::Value,
    code: crate::machine::CodeView<'_>,
    pc: usize,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let metadata = code
        .metadata_at(pc)
        .ok_or(crate::execute::VmError::MissingReturn)?;
    let name = metadata
        .name
        .as_deref()
        .ok_or(crate::execute::VmError::MissingReturn)?;
    crate::vm::get_named_property_result(receiver, name, &metadata.named_cache)
}

fn call_linked_method(
    receiver: &crate::value::Value,
    code: crate::machine::CodeView<'_>,
    pc: usize,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let callee = linked_method(receiver, code, pc)?;
    crate::functions::execute_target(&callee, receiver, &[])
}

fn writable_own_word<'a>(
    object: &'a crate::value::ObjectData,
    key: &str,
) -> Option<&'a crate::register_file::SlotWord> {
    if object.hot_properties().names().any(|name| {
        crate::builtins::is_deleted_key_for(name, key)
            || crate::builtins::is_descriptor_key_for(name, key)
    }) {
        return None;
    }
    crate::vm::proven_own_word(object, key)
}

fn exact_linked_task_id(value: f64) -> Option<usize> {
    (value.is_finite()
        && value >= 0.0
        && value.fract() == 0.0
        && value < LINKED_TASK_ID_LIMIT as f64)
        .then_some(value as usize)
}
