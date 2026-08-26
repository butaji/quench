const LINKED_PACKET_LIMIT: usize = 64;
const LINKED_PAYLOAD_LIMIT: usize = 16;

#[derive(Clone, Copy)]
enum NativeTaskKind {
    Idle {
        v1: i32,
        count: i32,
        device_a: usize,
        device_b: usize,
    },
    Device {
        v1: Option<usize>,
    },
    Worker {
        v1: usize,
        v2: i32,
        handler_a: usize,
        handler_b: usize,
        count: usize,
    },
    Handler {
        v1: Option<usize>,
        v2: Option<usize>,
        work_kind: i32,
        data_size: usize,
    },
}

struct NativeTask {
    link: Option<usize>,
    priority: i32,
    queue: Option<usize>,
    state: i32,
    held_mask: i32,
    suspended: i32,
    kind: NativeTaskKind,
}

struct NativePacket {
    link: Option<usize>,
    id: usize,
    kind: i32,
    a1: i32,
    payload: [f64; LINKED_PAYLOAD_LIMIT],
    payload_len: usize,
    payload_dirty: bool,
}

struct NativePacketBacking {
    object: std::rc::Rc<crate::value::ObjectData>,
    payload: std::rc::Rc<crate::value::ArrayData>,
}

struct NativeSchedule<'a> {
    table: &'a DirectTaskTable,
    scheduler: &'a LinkedSchedulerWords,
    tasks: Vec<NativeTask>,
    packets: Vec<NativePacket>,
    backings: Vec<NativePacketBacking>,
    current: Option<usize>,
    current_id: i32,
    hold_count: i32,
    queue_count: i32,
}

fn execute_linked_schedule_state(
    start: &crate::value::Value,
    current: &crate::register_file::SlotWord,
    current_id: &crate::register_file::SlotWord,
    table: &DirectTaskTable,
    scheduler: &LinkedSchedulerWords,
) -> Option<crate::value::Value> {
    let mut state = NativeSchedule::new(start, table, scheduler)?;
    let iterations = state.run()?;
    state.commit(current, current_id)?;
    crate::execution_trace::kernel("linked_schedule_native_state", false);
    crate::execution_trace::numeric_kernel_iterations(
        "linked_schedule_native_state",
        0,
        iterations,
        0,
        0,
    );
    Some(crate::value::Value::Undefined)
}

impl<'a> NativeSchedule<'a> {
    fn new(
        start: &crate::value::Value,
        table: &'a DirectTaskTable,
        scheduler: &'a LinkedSchedulerWords,
    ) -> Option<Self> {
        let mut state = Self {
            table,
            scheduler,
            tasks: Vec::with_capacity(table.runners.len()),
            packets: Vec::with_capacity(16),
            backings: Vec::with_capacity(16),
            current: table.id_for_value(start)?,
            current_id: 0,
            hold_count: exact_i32(scheduler.word(scheduler.hold_count).number()?)?,
            queue_count: exact_i32(scheduler.word(scheduler.queue_count).number()?)?,
        };
        state.current?;
        for index in 0..table.runners.len() {
            let runner = table.for_id(index)?;
            if exact_linked_task_id(runner.word(runner.id).number()?)? != index {
                return None;
            }
            let task = state.load_task(runner)?;
            state.tasks.push(task);
        }
        Some(state)
    }

    fn load_task(&mut self, runner: &DirectTaskRunner) -> Option<NativeTask> {
        let link = self.table.id_for_word(runner.word(runner.link))?;
        let queue = self.packet_from_word(runner.word(runner.queue))?;
        let kind = match runner.kind {
            DirectTaskKind::Idle(plan) => NativeTaskKind::Idle {
                v1: exact_i32(runner.word(runner.task_v1).number()?)?,
                count: exact_i32(runner.word(runner.task_count?).number()?)?,
                device_a: exact_linked_task_id(plan.device_a)?,
                device_b: exact_linked_task_id(plan.device_b)?,
            },
            DirectTaskKind::Device => NativeTaskKind::Device {
                v1: self.packet_from_word(runner.word(runner.task_v1))?,
            },
            DirectTaskKind::Worker(plan) => NativeTaskKind::Worker {
                v1: exact_linked_task_id(runner.word(runner.task_v1).number()?)?,
                v2: exact_i32(runner.word(runner.task_v2?).number()?)?,
                handler_a: exact_linked_task_id(plan.handler_a)?,
                handler_b: exact_linked_task_id(plan.handler_b)?,
                count: plan.count,
            },
            DirectTaskKind::Handler(plan) => NativeTaskKind::Handler {
                v1: self.packet_from_word(runner.word(runner.task_v1))?,
                v2: self.packet_from_word(runner.word(runner.task_v2?))?,
                work_kind: exact_i32(plan.work_kind)?,
                data_size: usize::try_from(exact_i32(plan.data_size)?).ok()?,
            },
        };
        Some(NativeTask {
            link,
            priority: exact_i32(runner.word(runner.priority).number()?)?,
            queue,
            state: exact_i32(runner.word(runner.state).number()?)?,
            held_mask: runner.held_mask,
            suspended: runner.suspended,
            kind,
        })
    }

    fn packet_from_word(&mut self, word: &crate::register_file::SlotWord) -> Option<Option<usize>> {
        let value = word.load();
        self.packet_from_value(&value)
    }

    fn packet_from_value(&mut self, value: &crate::value::Value) -> Option<Option<usize>> {
        let crate::value::Value::Object(object) = value else {
            return value.is_nullish().then_some(None);
        };
        if let Some(index) = self
            .backings
            .iter()
            .position(|backing| std::rc::Rc::ptr_eq(&backing.object, object))
        {
            return Some(Some(index));
        }
        if self.packets.len() == LINKED_PACKET_LIMIT || object.has_replacement() {
            return None;
        }
        let words = self.table.packet_words(object)?;
        let crate::value::Value::Array(payload) = words.a2.load() else {
            return None;
        };
        if !crate::locals::array_word_is_current(&payload)
            || payload.header_length() > LINKED_PAYLOAD_LIMIT
            || !(payload.is_packed_ordinary() && payload.is_numeric_packed()
                || payload.is_holey() && payload.physical_len() == 0)
        {
            return None;
        }
        let payload_len = payload.header_length();
        let mut values = [0.0; LINKED_PAYLOAD_LIMIT];
        if payload.is_numeric_packed() {
            for (index, value) in values[..payload_len].iter_mut().enumerate() {
                let number = payload.dense_number_at(index)?;
                exact_i32(number)?;
                *value = number;
            }
        }
        let index = self.packets.len();
        self.packets.push(NativePacket {
            link: None,
            id: exact_linked_task_id(words.id.number()?)?,
            kind: exact_i32(words.kind.number()?)?,
            a1: exact_i32(words.a1.number()?)?,
            payload: values,
            payload_len,
            payload_dirty: false,
        });
        self.backings.push(NativePacketBacking {
            object: std::rc::Rc::clone(object),
            payload,
        });
        let link = self.packet_from_word(words.link)?;
        self.packets[index].link = link;
        Some(Some(index))
    }

    fn run(&mut self) -> Option<usize> {
        let mut iterations = 0usize;
        while let Some(current) = self.current {
            iterations = iterations.checked_add(1)?;
            let task = self.task(current)?;
            if task.state & task.held_mask != 0 || task.state == task.suspended {
                self.current = task.link;
                continue;
            }
            self.current_id = i32::try_from(current).ok()?;
            let packet = self.take_packet(current)?;
            self.current = self.run_task(current, packet)?;
        }
        Some(iterations)
    }

    fn take_packet(&mut self, current: usize) -> Option<Option<usize>> {
        let runnable = self.scheduler.runnable;
        let (state, suspended, queue) = {
            let task = self.task(current)?;
            (task.state, task.suspended, task.queue)
        };
        if state != suspended | runnable {
            return Some(None);
        }
        let packet = queue?;
        let next = self.packets.get(packet)?.link;
        let task = self.task_mut(current)?;
        task.queue = next;
        task.state = if task.queue.is_none() { 0 } else { runnable };
        Some(Some(packet))
    }

    fn run_task(&mut self, current: usize, packet: Option<usize>) -> Option<Option<usize>> {
        match self.task(current)?.kind {
            NativeTaskKind::Idle { .. } => self.run_idle(current),
            NativeTaskKind::Device { .. } => self.run_device(current, packet),
            NativeTaskKind::Worker { .. } => self.run_worker(current, packet),
            NativeTaskKind::Handler { .. } => self.run_handler(current, packet),
        }
    }

    fn run_idle(&mut self, current: usize) -> Option<Option<usize>> {
        let NativeTaskKind::Idle {
            mut v1,
            mut count,
            device_a,
            device_b,
        } = self.task(current)?.kind
        else {
            return None;
        };
        count -= 1;
        let next = if count == 0 {
            self.hold(current)?
        } else {
            let target = if v1 & 1 == 0 { device_a } else { device_b };
            v1 = if v1 & 1 == 0 {
                v1 >> 1
            } else {
                (v1 >> 1) ^ 0xD008
            };
            self.release(current, target)?
        };
        self.task_mut(current)?.kind = NativeTaskKind::Idle {
            v1,
            count,
            device_a,
            device_b,
        };
        Some(next)
    }

    fn run_device(&mut self, current: usize, packet: Option<usize>) -> Option<Option<usize>> {
        let NativeTaskKind::Device { v1 } = self.task(current)?.kind else {
            return None;
        };
        let (next_v1, next) = if let Some(packet) = packet {
            (Some(packet), self.hold(current)?)
        } else if let Some(queued) = v1 {
            (
                None,
                Some(self.queue_packet(current, queued, self.packets.get(queued)?.id)?),
            )
        } else {
            (None, self.suspend(current)?)
        };
        self.task_mut(current)?.kind = NativeTaskKind::Device { v1: next_v1 };
        Some(next)
    }

    fn run_worker(&mut self, current: usize, packet: Option<usize>) -> Option<Option<usize>> {
        let Some(packet) = packet else {
            return self.suspend(current);
        };
        let NativeTaskKind::Worker {
            v1,
            mut v2,
            handler_a,
            handler_b,
            count,
        } = self.task(current)?.kind
        else {
            return None;
        };
        if count > self.packets.get(packet)?.payload_len {
            return None;
        }
        let next_v1 = if v1 == handler_a {
            handler_b
        } else {
            handler_a
        };
        let target = self.packets.get_mut(packet)?;
        target.id = next_v1;
        target.a1 = 0;
        target.payload_dirty = true;
        for value in &mut target.payload[..count] {
            v2 += 1;
            if v2 > 26 {
                v2 = 1;
            }
            *value = f64::from(v2);
        }
        self.task_mut(current)?.kind = NativeTaskKind::Worker {
            v1: next_v1,
            v2,
            handler_a,
            handler_b,
            count,
        };
        self.queue_packet(current, packet, next_v1).map(Some)
    }

    fn run_handler(&mut self, current: usize, incoming: Option<usize>) -> Option<Option<usize>> {
        let NativeTaskKind::Handler {
            mut v1,
            mut v2,
            work_kind,
            data_size,
        } = self.task(current)?.kind
        else {
            return None;
        };
        if let Some(packet) = incoming {
            if self.packets.get(packet)?.kind == work_kind {
                v1 = Some(self.append_packet(v1, packet)?);
            } else {
                v2 = Some(self.append_packet(v2, packet)?);
            }
        }
        let next = match v1 {
            None => self.suspend(current)?,
            Some(work) if usize::try_from(self.packets.get(work)?.a1).ok()? < data_size => {
                let Some(device) = v2 else {
                    self.task_mut(current)?.kind = NativeTaskKind::Handler {
                        v1,
                        v2,
                        work_kind,
                        data_size,
                    };
                    return self.suspend(current);
                };
                v2 = self.packets.get(device)?.link;
                let offset = usize::try_from(self.packets.get(work)?.a1).ok()?;
                if offset >= self.packets.get(work)?.payload_len {
                    return None;
                }
                self.packets.get_mut(device)?.a1 = self.packets.get(work)?.payload[offset] as i32;
                self.packets.get_mut(work)?.a1 += 1;
                let target = self.packets.get(device)?.id;
                Some(self.queue_packet(current, device, target)?)
            }
            Some(work) => {
                v1 = self.packets.get(work)?.link;
                let target = self.packets.get(work)?.id;
                Some(self.queue_packet(current, work, target)?)
            }
        };
        self.task_mut(current)?.kind = NativeTaskKind::Handler {
            v1,
            v2,
            work_kind,
            data_size,
        };
        Some(next)
    }

    fn append_packet(&mut self, head: Option<usize>, packet: usize) -> Option<usize> {
        self.packets.get_mut(packet)?.link = None;
        let Some(mut tail) = head else {
            return Some(packet);
        };
        for _ in 0..self.packets.len() {
            match self.packets.get(tail)?.link {
                Some(next) => tail = next,
                None => {
                    self.packets.get_mut(tail)?.link = Some(packet);
                    return Some(head?);
                }
            }
        }
        None
    }

    fn queue_packet(&mut self, current: usize, packet: usize, target: usize) -> Option<usize> {
        self.queue_count = self.queue_count.checked_add(1)?;
        self.packets.get_mut(packet)?.link = None;
        self.packets.get_mut(packet)?.id = current;
        let current_priority = self.task(current)?.priority;
        let runnable = self.scheduler.runnable;
        let target_task = self.task_mut(target)?;
        if target_task.queue.is_none() {
            target_task.queue = Some(packet);
            target_task.state |= runnable;
            return Some(if target_task.priority > current_priority {
                target
            } else {
                current
            });
        }
        let head = target_task.queue;
        self.append_packet(head, packet)?;
        Some(current)
    }

    fn release(&mut self, current: usize, target: usize) -> Option<Option<usize>> {
        let current_priority = self.task(current)?.priority;
        let target_task = self.task_mut(target)?;
        target_task.state &= !target_task.held_mask;
        Some(Some(if target_task.priority > current_priority {
            target
        } else {
            current
        }))
    }

    fn hold(&mut self, current: usize) -> Option<Option<usize>> {
        self.hold_count = self.hold_count.checked_add(1)?;
        let task = self.task_mut(current)?;
        task.state |= task.held_mask;
        Some(task.link)
    }

    fn suspend(&mut self, current: usize) -> Option<Option<usize>> {
        let task = self.task_mut(current)?;
        task.state |= task.suspended;
        Some(Some(current))
    }

    fn task(&self, index: usize) -> Option<&NativeTask> {
        self.tasks.get(index)
    }

    fn task_mut(&mut self, index: usize) -> Option<&mut NativeTask> {
        self.tasks.get_mut(index)
    }
}
