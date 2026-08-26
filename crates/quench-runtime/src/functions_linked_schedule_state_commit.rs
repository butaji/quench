impl NativeSchedule<'_> {
    #[cfg(feature = "execution-trace")]
    #[inline(always)]
    fn trace_direct_work(&self) {
        for (id, iterations) in [
            ("linked_schedule_idle", self.task_iterations[0]),
            ("linked_schedule_device", self.task_iterations[1]),
            ("linked_schedule_worker", self.task_iterations[2]),
            ("linked_schedule_handler", self.task_iterations[3]),
            ("linked_schedule_queue", self.queue_iterations),
            ("linked_schedule_append_step", self.append_steps),
            ("linked_device_receive", self.direct_branches[0]),
            ("linked_device_send", self.direct_branches[1]),
            ("linked_device_suspend", self.direct_branches[2]),
            ("linked_handler_receive_work", self.direct_branches[3]),
            ("linked_handler_receive_device", self.direct_branches[4]),
            ("linked_handler_no_work", self.direct_branches[5]),
            ("linked_handler_wait_device", self.direct_branches[6]),
            ("linked_handler_send_device", self.direct_branches[7]),
            ("linked_handler_send_work", self.direct_branches[8]),
            ("linked_queue_empty", self.direct_branches[9]),
            ("linked_queue_append", self.direct_branches[10]),
            ("linked_suspend_fused", self.direct_branches[11]),
        ] {
            crate::execution_trace::kernel_iterations(id, iterations);
        }
    }

    #[cfg(not(feature = "execution-trace"))]
    #[inline(always)]
    fn trace_direct_work(&self) {}

    fn commit(
        &self,
        current: &crate::register_file::SlotWord,
        current_id: &crate::register_file::SlotWord,
    ) -> Option<()> {
        for (packet, backing) in self.packets.iter().zip(&self.backings) {
            if backing.payload_dirty {
                apply_linked_worker_payload(
                    &backing.array,
                    &backing.payload[..packet.payload_len],
                )?;
            }
            let words = self.table.packet_words(&backing.object)?;
            words.link.store(self.packet_value(packet.link));
            words.id.store_number(packet.id as f64);
            words.a1.store_number(f64::from(packet.a1));
        }
        for (index, task) in self.tasks.iter().enumerate() {
            let runner = self.table.for_id(index)?;
            runner
                .word(runner.state)
                .store_number(f64::from(task.state));
            runner
                .word(runner.queue)
                .store(self.packet_value(task.queue.head()));
            self.commit_task(runner, task.kind)?;
        }
        self.scheduler
            .word(self.scheduler.hold_count)
            .store_number(f64::from(self.hold_count));
        self.scheduler
            .word(self.scheduler.queue_count)
            .store_number(f64::from(self.queue_count));
        current.store(crate::value::Value::Null);
        current_id.store_number(f64::from(self.current_id));
        Some(())
    }

    fn commit_task(&self, runner: &DirectTaskRunner, kind: NativeTaskKind) -> Option<()> {
        match kind {
            NativeTaskKind::Idle { v1, count, .. } => {
                runner.word(runner.task_v1).store_number(f64::from(v1));
                runner
                    .word(runner.task_count?)
                    .store_number(f64::from(count));
            }
            NativeTaskKind::Device { v1 } => {
                runner.word(runner.task_v1).store(self.packet_value(v1));
            }
            NativeTaskKind::Worker { v1, v2, .. } => {
                runner.word(runner.task_v1).store_number(v1 as f64);
                runner.word(runner.task_v2?).store_number(f64::from(v2));
            }
            NativeTaskKind::Handler { v1, v2, .. } => {
                runner
                    .word(runner.task_v1)
                    .store(self.packet_value(v1.head()));
                runner
                    .word(runner.task_v2?)
                    .store(self.packet_value(v2.head()));
            }
        }
        Some(())
    }

    fn packet_value(&self, packet: Option<usize>) -> crate::value::Value {
        packet.map_or(crate::value::Value::Null, |index| {
            crate::value::Value::Object(std::rc::Rc::clone(&self.backings[index].object))
        })
    }
}
