impl LinkedModule {
    pub fn execute(&self) -> Result<quench_runtime::value::Value, String> {
        if let Some(thrown) = self.thrown.borrow().clone() {
            quench_runtime::module_bindings::request_ensure_throw(thrown);
            return Err("module evaluation failed".to_string());
        }
        if self.evaluated.get() {
            return Ok(quench_runtime::value::Value::Undefined);
        }
        self.evaluating.set(true);
        let start = self.resume_pc.get();
        let mut registers = self.resume_registers.borrow().clone();
        let result = self.scope.execute_from(
            self.program.ops(),
            start,
            &mut registers,
            host_context(),
        );
        self.evaluating.set(false);
        self.complete_execute(result, registers)
    }

    fn complete_execute(
        &self,
        result: Result<(quench_runtime::completion::Completion, usize), quench_runtime::execute::VmError>,
        registers: Vec<quench_runtime::value::Value>,
    ) -> Result<quench_runtime::value::Value, String> {
        match result {
            Ok((quench_runtime::completion::Completion::Suspend(_), next)) => {
                let pc = if quench_runtime::module_bindings::await_advanced() {
                    next
                } else {
                    next.saturating_sub(1)
                };
                self.suspend_execute(pc, registers)
            }
            Ok((quench_runtime::completion::Completion::Throw(value), _)) => {
                self.fail_execute(value)
            }
            Ok((quench_runtime::completion::Completion::Return(value), _)) => {
                self.finish_execute(value)
            }
            Ok((_, _)) => self.finish_execute(quench_runtime::value::Value::Undefined),
            Err(error) => self.error_execute(error),
        }
    }

    fn suspend_execute(
        &self,
        next: usize,
        registers: Vec<quench_runtime::value::Value>,
    ) -> Result<quench_runtime::value::Value, String> {
        if self.resume_pc.get() == next && !registers.is_empty() {
            *self.resume_registers.borrow_mut() = registers;
            return Ok(quench_runtime::value::Value::Undefined);
        }
        self.async_suspended.set(true);
        self.resume_pc.set(next);
        *self.resume_registers.borrow_mut() = registers;
        let unit = self as *const LinkedModule;
        quench_runtime::module_bindings::enqueue_job(Rc::new(move || {
            let _ = unsafe { &*unit }.execute();
        }));
        Ok(quench_runtime::value::Value::Undefined)
    }

    fn fail_execute(
        &self,
        value: quench_runtime::value::Value,
    ) -> Result<quench_runtime::value::Value, String> {
        self.async_suspended.set(false);
        self.evaluated.set(true);
        wake_waiting_modules();
        *self.thrown.borrow_mut() = Some(value.clone());
        quench_runtime::module_bindings::request_ensure_throw(value.clone());
        Err(format!(
            "residual VM error: {}",
            quench_runtime::execute::VmError::Thrown(value).render()
        ))
    }

    fn error_execute(
        &self,
        error: quench_runtime::execute::VmError,
    ) -> Result<quench_runtime::value::Value, String> {
        if let quench_runtime::execute::VmError::Thrown(value) = &error {
            *self.thrown.borrow_mut() = Some(value.clone());
            quench_runtime::module_bindings::request_ensure_throw(value.clone());
        }
        Err(format!("residual VM error: {}", error.render()))
    }

    fn finish_execute(
        &self,
        value: quench_runtime::value::Value,
    ) -> Result<quench_runtime::value::Value, String> {
        self.async_suspended.set(false);
        self.evaluated.set(true);
        wake_waiting_modules();
        for (name, export) in &self.fixed_exports {
            let cell = self
                .export_cell(name)
                .ok_or_else(|| format!("fixed export {name} missing"))?;
            cell.set(export.clone());
        }
        self.refresh_namespace();
        Ok(value)
    }
}
