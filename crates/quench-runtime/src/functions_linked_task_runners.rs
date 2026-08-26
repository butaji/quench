const LINKED_TASK_ID_LIMIT: usize = 64;
const LINKED_TASK_IDENTITY_SLOTS: usize = LINKED_TASK_ID_LIMIT * 2;

enum DirectTaskKind {
    Idle(IdleTaskPlan),
    Device,
    Worker(WorkerTaskPlan),
    Handler(HandlerTaskPlan),
}

struct DirectTaskRunner {
    index: usize,
    identity: u64,
    id: *const crate::register_file::SlotWord,
    link: *const crate::register_file::SlotWord,
    state: *const crate::register_file::SlotWord,
    queue: *const crate::register_file::SlotWord,
    priority: *const crate::register_file::SlotWord,
    task_word: *const crate::register_file::SlotWord,
    task_v1: *const crate::register_file::SlotWord,
    task_count: Option<*const crate::register_file::SlotWord>,
    task_v2: Option<*const crate::register_file::SlotWord>,
    held_mask: i32,
    suspended: i32,
    tcb: *const crate::value::ObjectData,
    task: *const crate::value::ObjectData,
    task_value: crate::value::Value,
    tcb_value: crate::value::Value,
    scheduler: *const crate::value::ObjectData,
    function: std::rc::Rc<crate::value::FunctionValue>,
    kind: DirectTaskKind,
}

struct DirectTaskStep {
    next: Option<usize>,
}

impl DirectTaskStep {
    fn new(next: Option<usize>) -> Self {
        Self { next }
    }
}

enum DirectTaskOutcome {
    Step(DirectTaskStep),
    Miss(crate::value::Value),
    Value(crate::value::Value),
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

    fn tcb(&self) -> &crate::value::ObjectData {
        // SAFETY: `tcb_value` retains this exact allocation for the table's lifetime.
        unsafe { &*self.tcb }
    }

    fn execute(
        &self,
        packet: &crate::value::Value,
        table: &DirectTaskTable,
        scheduler: Option<&LinkedSchedulerWords>,
    ) -> Option<DirectTaskStep> {
        let scheduler = scheduler?;
        match self.kind {
            DirectTaskKind::Idle(plan) => execute_linked_idle(self, table, scheduler, plan),
            DirectTaskKind::Device => execute_linked_device(self, table, scheduler, packet),
            DirectTaskKind::Worker(plan) => {
                execute_linked_worker(self, table, scheduler, packet, plan)
            }
            DirectTaskKind::Handler(plan) => {
                execute_linked_handler(self, table, scheduler, packet, plan)
            }
        }
    }
}

struct DirectTaskTable {
    runners: Vec<Option<DirectTaskRunner>>,
    identities: [Option<(u64, u8)>; LINKED_TASK_IDENTITY_SLOTS],
    packet_layout: Option<LinkedPacketLayout>,
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

    fn for_id(&self, id: usize) -> Option<&DirectTaskRunner> {
        self.runners.get(id)?.as_ref()
    }

    fn value_for_id(&self, id: usize) -> Option<&crate::value::Value> {
        Some(&self.for_id(id)?.tcb_value)
    }

    fn id_for_value(&self, value: &crate::value::Value) -> Option<Option<usize>> {
        match value {
            crate::value::Value::Null => Some(None),
            crate::value::Value::Object(object) => {
                self.for_object(object).map(|runner| Some(runner.index))
            }
            _ => None,
        }
    }

    fn id_for_word(&self, word: &crate::register_file::SlotWord) -> Option<Option<usize>> {
        let object = word.object_or_null_ptr()?;
        object.map_or(Some(None), |object| {
            // SAFETY: the source word owns this object throughout the lookup.
            self.for_object(unsafe { &*object }).map(|runner| Some(runner.index))
        })
    }

    fn matches_scheduler(&self, scheduler: &crate::value::ObjectData) -> bool {
        let scheduler = std::ptr::from_ref(scheduler);
        self.runners
            .iter()
            .flatten()
            .all(|runner| runner.scheduler == scheduler)
    }

    fn packet_words<'a>(
        &self,
        packet: &'a crate::value::ObjectData,
    ) -> Option<LinkedPacketWords<'a>> {
        self.packet_layout?.words(packet)
    }

    fn packet_tail_link(
        &self,
        head: *const crate::value::ObjectData,
        packet: *const crate::value::ObjectData,
    ) -> Option<*const crate::register_file::SlotWord> {
        self.packet_layout?.tail_link(head, packet)
    }

    fn derive_packet_layout(&self) -> Option<LinkedPacketLayout> {
        self.runners.iter().flatten().find_map(|runner| {
            [runner.queue, runner.task_v1]
                .into_iter()
                .chain(runner.task_v2)
                .find_map(|slot| runner.word(slot).object_or_null_ptr().flatten())
                .and_then(|packet| LinkedPacketLayout::new(unsafe { &*packet }))
        })
    }
}

fn linked_task_runners(start: &crate::value::Value) -> Option<DirectTaskTable> {
    let mut table = DirectTaskTable {
        runners: Vec::new(),
        identities: [None; LINKED_TASK_IDENTITY_SLOTS],
        packet_layout: None,
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
        let runner = linked_task_runner(id, &cursor, &tcb)?;
        cursor = runner.word(runner.link).load();
        table.insert(id, runner)?;
    }
    if !matches!(cursor, crate::value::Value::Null) {
        return None;
    }
    table.packet_layout = table.derive_packet_layout();
    table.packet_layout.map(|_| table)
}

fn linked_task_runner(
    index: usize,
    tcb_value: &crate::value::Value,
    tcb: &crate::value::ObjectData,
) -> Option<DirectTaskRunner> {
    let id = crate::vm::proven_own_word(tcb, "id")?;
    let link = crate::vm::proven_own_word(tcb, "link")?;
    let state = crate::vm::proven_own_word(tcb, "state")?;
    let queue = crate::vm::proven_own_word(tcb, "queue")?;
    let priority = crate::vm::proven_own_word(tcb, "priority")?;
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
    let task_count = matches!(&kind, DirectTaskKind::Idle(_))
        .then(|| crate::vm::proven_own_word(task_object, "count"))
        .flatten()
        .map(std::ptr::from_ref);
    if matches!(&kind, DirectTaskKind::Idle(_)) && task_count.is_none() {
        return None;
    }
    let task_v2 = matches!(&kind, DirectTaskKind::Worker(_) | DirectTaskKind::Handler(_))
        .then(|| crate::vm::proven_own_word(task_object, "v2"))
        .flatten()
        .map(std::ptr::from_ref);
    if matches!(&kind, DirectTaskKind::Worker(_) | DirectTaskKind::Handler(_))
        && task_v2.is_none()
    {
        return None;
    }
    let crate::value::Value::Object(scheduler) = task_scheduler(task_object)? else {
        return None;
    };
    if scheduler.has_replacement() {
        return None;
    }
    Some(DirectTaskRunner {
        index,
        identity: tcb.identity(),
        id: std::ptr::from_ref(id),
        link: std::ptr::from_ref(link),
        state: std::ptr::from_ref(state),
        queue: std::ptr::from_ref(queue),
        priority: std::ptr::from_ref(priority),
        task_word: std::ptr::from_ref(task_word),
        task_v1: std::ptr::from_ref(task_v1),
        task_count,
        task_v2,
        held_mask,
        suspended,
        tcb: std::ptr::from_ref(tcb),
        task: std::rc::Rc::as_ptr(task_object),
        task_value: task,
        tcb_value: tcb_value.clone(),
        scheduler: std::rc::Rc::as_ptr(&scheduler),
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
    if let Some(plan) = idle_task_fact(function) {
        Some(DirectTaskKind::Idle(plan))
    } else if device_task_fact(function) {
        Some(DirectTaskKind::Device)
    } else if let Some(plan) = worker_task_fact(function) {
        Some(DirectTaskKind::Worker(plan))
    } else if let Some(plan) = handler_task_fact(function) {
        Some(DirectTaskKind::Handler(plan))
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
