impl NativeSchedule<'_> {
    fn commit(
        &self,
        current: &crate::register_file::SlotWord,
        current_id: &crate::register_file::SlotWord,
    ) -> Option<()> {
        for (packet, backing) in self.packets.iter().zip(&self.backings) {
            if packet.payload_dirty {
                apply_linked_worker_payload(
                    &backing.payload,
                    &packet.payload[..packet.payload_len],
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
                .store(self.packet_value(task.queue));
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
                runner.word(runner.task_v1).store(self.packet_value(v1));
                runner.word(runner.task_v2?).store(self.packet_value(v2));
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
