const LINKED_TASK_ID_LIMIT: usize = 64;

enum DirectTaskKind {
    Idle,
    Device,
    Worker,
    Handler,
}

struct DirectTaskRunner {
    task: *const crate::value::ObjectData,
    function: std::rc::Rc<crate::value::FunctionValue>,
    kind: DirectTaskKind,
}

impl DirectTaskRunner {
    fn matches(&self, task: &crate::value::Value) -> bool {
        matches!(task, crate::value::Value::Object(object) if std::rc::Rc::as_ptr(object) == self.task)
    }

    fn callee(&self) -> crate::value::Value {
        crate::value::Value::Function(std::rc::Rc::clone(&self.function))
    }

    fn execute(
        &self,
        task: &crate::value::Value,
        packet: &crate::value::Value,
    ) -> Result<Option<crate::value::Value>, crate::execute::VmError> {
        let arguments = std::slice::from_ref(packet);
        match self.kind {
            DirectTaskKind::Idle => execute_idle_task(&self.function, task),
            DirectTaskKind::Device => execute_device_task(&self.function, task, arguments),
            DirectTaskKind::Worker => execute_worker_task(&self.function, task, arguments),
            DirectTaskKind::Handler => execute_handler_task(&self.function, task, arguments),
        }
    }
}

fn direct_task_runner<'a>(
    runners: &'a [Option<DirectTaskRunner>],
    id: &crate::register_file::SlotWord,
) -> Option<&'a DirectTaskRunner> {
    let id = exact_linked_task_id(id.number()?)?;
    runners.get(id)?.as_ref()
}

fn linked_task_runners(start: &crate::value::Value) -> Option<Vec<Option<DirectTaskRunner>>> {
    let mut runners = Vec::new();
    let mut cursor = start.clone();
    while let crate::value::Value::Object(tcb) = cursor {
        let id = exact_linked_task_id(crate::vm::proven_own_word(&tcb, "id")?.number()?)?;
        if runners.len() <= id {
            runners.resize_with(id + 1, || None);
        }
        if runners[id].is_some() {
            return None;
        }
        runners[id] = Some(linked_task_runner(&tcb)?);
        cursor = crate::vm::proven_own_word(&tcb, "link")?.load();
    }
    matches!(cursor, crate::value::Value::Null).then_some(runners)
}

fn linked_task_runner(tcb: &crate::value::ObjectData) -> Option<DirectTaskRunner> {
    let task = crate::vm::proven_own_word(tcb, "task")?.load();
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
    let kind = direct_task_kind(&function)?;
    Some(DirectTaskRunner {
        task: std::rc::Rc::as_ptr(task_object),
        function,
        kind,
    })
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

fn exact_linked_task_id(value: f64) -> Option<usize> {
    (value.is_finite()
        && value >= 0.0
        && value.fract() == 0.0
        && value < LINKED_TASK_ID_LIMIT as f64)
        .then_some(value as usize)
}
