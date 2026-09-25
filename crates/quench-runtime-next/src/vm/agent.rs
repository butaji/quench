use super::atomics::AgentTimer;
use super::promise::PromiseState;
use super::property_key::PropertyKey;
use super::*;
use std::time::{Duration, Instant};

pub(super) const AGENT_SLEEP_UNIT_MS: f64 = 1_000.0;
pub(super) const AGENT_YIELD_TIMEOUT_MS: f64 = 1.0;
const AGENT_SMALL_TIMEOUT_MS: f64 = 100.0;
const AGENT_LONG_TIMEOUT_MS: f64 = 1_000.0;
const AGENT_HUGE_TIMEOUT_MS: f64 = 10_000.0;
pub(super) const AGENT_SPIN_LIMIT: u32 = 1_000;

#[derive(Clone, Copy)]
#[repr(u8)]
enum AgentMethod {
    Start,
    Broadcast,
    Report,
    GetReport,
    Leaving,
    ReceiveBroadcast,
    Sleep,
    TryYield,
    TrySleep,
    SetTimeout,
    MonotonicNow,
}

const AGENT_METHODS: &[(AgentMethod, &str, f64)] = &[
    (AgentMethod::Start, "start", 1.0),
    (AgentMethod::Broadcast, "broadcast", 1.0),
    (AgentMethod::Report, "report", 1.0),
    (AgentMethod::GetReport, "getReport", 0.0),
    (AgentMethod::Leaving, "leaving", 0.0),
    (AgentMethod::ReceiveBroadcast, "receiveBroadcast", 1.0),
    (AgentMethod::Sleep, "sleep", 1.0),
    (AgentMethod::TryYield, "tryYield", 0.0),
    (AgentMethod::TrySleep, "trySleep", 1.0),
    (AgentMethod::SetTimeout, "setTimeout", 2.0),
    (AgentMethod::MonotonicNow, "monotonicNow", 0.0),
];

impl<H: Host> Vm<H> {
    pub(super) fn install_test262_agent(
        &mut self,
        p: &ResidualProgram,
        global: Value,
    ) -> Result<(), JsError> {
        let agent = self.object();
        for (method, name, length) in AGENT_METHODS {
            let state = self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: Rc::new(vec![Value::number(*method as u8 as f64)]),
            });
            let function = self.native_with_env(Native::Test262Agent, state);
            self.set_builtin_function_name(function, name)?;
            self.set_agent_function_length(function, *length);
            self.set_builtin_value_named(agent, name, function)?;
        }
        let timeouts = self.object();
        for (name, timeout) in [
            ("yield", AGENT_YIELD_TIMEOUT_MS),
            ("small", AGENT_SMALL_TIMEOUT_MS),
            ("long", AGENT_LONG_TIMEOUT_MS),
            ("huge", AGENT_HUGE_TIMEOUT_MS),
        ] {
            self.set_named(p, timeouts, name, Value::number(timeout))?;
        }
        self.set_builtin_value_named(agent, "timeouts", timeouts)?;
        self.set_builtin_value_named(global, "agent", agent)
    }

    fn set_agent_function_length(&mut self, function: Value, length: f64) {
        let atom = self.intern_atom("length");
        let _ = self.set_property(function, atom, Value::number(length));
        self.set_property_attributes(
            function,
            PropertyKey::string(atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
    }

    pub(super) fn call_test262_agent(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let method = self.active_native_env().and_then(|env| {
            let Some(Cell::Array { elements, .. }) = self.heap.get(env) else {
                return None;
            };
            elements.first().and_then(|value| value.as_number())
        });
        match method.and_then(AgentMethod::from_index) {
            Some(AgentMethod::Start) => self.agent_start(p, args),
            Some(AgentMethod::Broadcast) => self.agent_broadcast(p, args),
            Some(AgentMethod::Report) => self.agent_report(p, args),
            Some(AgentMethod::GetReport) => self.agent_get_report(p),
            Some(AgentMethod::Leaving) => Ok(Value::UNDEFINED),
            Some(AgentMethod::ReceiveBroadcast) => self.agent_receive_broadcast(p, args),
            Some(AgentMethod::Sleep | AgentMethod::TryYield | AgentMethod::TrySleep) => {
                self.agent_sleep(p, args)
            }
            Some(AgentMethod::SetTimeout) => self.agent_set_timeout(p, args),
            Some(AgentMethod::MonotonicNow) => Ok(Value::number(
                self.test262_agent.started_at.elapsed().as_secs_f64() * AGENT_SLEEP_UNIT_MS,
            )),
            None => Err(JsError("invalid Test262 agent method".into())),
        }
    }

    fn agent_start(&mut self, p: &ResidualProgram, args: &[Value]) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !matches!(self.heap.get(source), Some(Cell::String(_))) {
            return Err(self.type_error(p, "agent.start expects a string".into()));
        }
        self.begin_agent_callback();
        let result = self.eval_script_native(p, &[source]);
        self.end_agent_callback();
        result
    }

    fn agent_receive_broadcast(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_function(callback) {
            return Err(self.type_error(p, "agent.receiveBroadcast expects a callback".into()));
        }
        self.test262_agent.callbacks.push(callback);
        Ok(Value::UNDEFINED)
    }

    fn agent_broadcast(&mut self, p: &ResidualProgram, args: &[Value]) -> Result<Value, JsError> {
        let buffer = args.first().copied().unwrap_or(Value::UNDEFINED);
        let callbacks = self.test262_agent.callbacks.clone();
        for callback in callbacks {
            self.begin_agent_callback();
            let result = self.call_value(p, callback, Value::UNDEFINED, &[buffer]);
            self.end_agent_callback();
            result?;
        }
        Ok(Value::UNDEFINED)
    }

    fn agent_report(&mut self, p: &ResidualProgram, args: &[Value]) -> Result<Value, JsError> {
        let value = self.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let report = self.heap.alloc(Cell::String(value.clone().into()));
        let index = self.test262_agent.reports.len();
        self.test262_agent.reports.push(report);
        self.test262_agent.consumed_reports.push(false);
        let associates = matches!(value.as_str(), "ok" | "timed-out" | "not-equal")
            || [" ok", " timed-out", " not-equal"]
                .iter()
                .any(|suffix| value.ends_with(suffix));
        if let Some(waiter_id) = self.test262_agent.current_waiter
            && let Some(waiter) = self
                .test262_agent
                .waiters
                .iter_mut()
                .find(|waiter| waiter.id == waiter_id)
        {
            if associates && waiter.report.is_none() {
                waiter.report = Some(index);
            } else {
                waiter.followups.push(index);
            }
        }
        Ok(Value::UNDEFINED)
    }

    fn agent_get_report(&mut self, p: &ResidualProgram) -> Result<Value, JsError> {
        self.run_due_agent_timers(p);
        self.expire_agent_waiters(p)?;
        self.drain_jobs(p)?;
        let now = Instant::now();
        let report = self
            .test262_agent
            .reports
            .iter()
            .enumerate()
            .find_map(|(index, report)| {
                let consumed = self.test262_agent.consumed_reports[index];
                let waiting = self.test262_agent.waiters.iter().find(|waiter| {
                    waiter.report == Some(index) || waiter.followups.contains(&index)
                });
                (!consumed
                    && waiting.is_none_or(|waiter| {
                        waiter.woken || waiter.deadline.is_some_and(|deadline| deadline <= now)
                    }))
                .then_some((index, *report))
            });
        let Some((index, report)) = report else {
            return Ok(Value::UNDEFINED);
        };
        self.test262_agent.consumed_reports[index] = true;
        Ok(report)
    }

    fn agent_sleep(&mut self, p: &ResidualProgram, args: &[Value]) -> Result<Value, JsError> {
        let delay = self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        if delay.is_finite() && delay > 0.0 {
            std::thread::sleep(Duration::from_secs_f64(delay / AGENT_SLEEP_UNIT_MS));
        } else {
            std::thread::yield_now();
        }
        self.run_due_agent_timers(p);
        self.expire_agent_waiters(p)?;
        Ok(Value::UNDEFINED)
    }

    fn agent_set_timeout(&mut self, p: &ResidualProgram, args: &[Value]) -> Result<Value, JsError> {
        let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_function(callback) {
            return Err(self.type_error(p, "agent.setTimeout expects a callback".into()));
        }
        let delay = self.to_number(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
        let delay = if delay.is_finite() && delay > 0.0 {
            delay
        } else {
            0.0
        };
        self.test262_agent.timers.push(AgentTimer {
            callback,
            deadline: Instant::now() + Duration::from_secs_f64(delay / AGENT_SLEEP_UNIT_MS),
        });
        Ok(Value::UNDEFINED)
    }

    fn run_due_agent_timers(&mut self, p: &ResidualProgram) {
        let now = Instant::now();
        let (due, pending): (Vec<_>, Vec<_>) = self
            .test262_agent
            .timers
            .drain(..)
            .partition(|timer| timer.deadline <= now);
        self.test262_agent.timers = pending;
        for timer in due {
            self.begin_agent_callback();
            let _ = self.call_value(p, timer.callback, Value::UNDEFINED, &[]);
            self.end_agent_callback();
        }
    }

    fn expire_agent_waiters(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        let now = Instant::now();
        let waiters = std::mem::take(&mut self.test262_agent.waiters);
        let mut remaining = Vec::with_capacity(waiters.len());
        for waiter in waiters {
            if !waiter.deadline.is_some_and(|deadline| deadline <= now) {
                remaining.push(waiter);
                continue;
            }
            if let Some(promise) = waiter.async_promise {
                let timeout_result = self.heap.alloc(Cell::String("timed-out".into()));
                self.promise_settle(p, promise, PromiseState::Fulfilled, timeout_result)?;
            } else if !waiter.woken {
                if let Some(report) = waiter.report {
                    self.replace_agent_report(report, "timed-out");
                }
                if let Some(timeout) = waiter.timeout_ms {
                    for report in waiter.followups {
                        if self.agent_report_is_number(report) {
                            self.replace_agent_report(report, &timeout.to_string());
                        }
                    }
                }
            }
        }
        self.test262_agent.waiters = remaining;
        Ok(())
    }

    fn agent_report_is_number(&self, index: usize) -> bool {
        self.test262_agent.reports.get(index).is_some_and(|report| {
            matches!(self.heap.get(*report), Some(Cell::String(value)) if value.host_string().parse::<f64>().is_ok())
        })
    }

    fn replace_agent_report(&mut self, index: usize, status: &str) {
        let Some(report) = self.test262_agent.reports.get(index).copied() else {
            return;
        };
        let replacement = match self.heap.get(report) {
            Some(Cell::String(value)) if value.host_string().contains(' ') => value
                .host_string()
                .rsplit_once(' ')
                .map(|(prefix, _)| format!("{prefix} {status}")),
            _ => Some(status.to_owned()),
        };
        if let Some(replacement) = replacement {
            self.test262_agent.reports[index] = self.heap.alloc(Cell::String(replacement.into()));
        }
    }

    fn begin_agent_callback(&mut self) {
        self.test262_agent.active_callback = true;
        self.test262_agent.spin_count = 0;
        self.test262_agent.current_waiter = None;
    }

    fn end_agent_callback(&mut self) {
        self.test262_agent.active_callback = false;
    }
}

impl AgentMethod {
    fn from_index(index: f64) -> Option<Self> {
        Some(match index as u8 {
            value if value == Self::Start as u8 => Self::Start,
            value if value == Self::Broadcast as u8 => Self::Broadcast,
            value if value == Self::Report as u8 => Self::Report,
            value if value == Self::GetReport as u8 => Self::GetReport,
            value if value == Self::Leaving as u8 => Self::Leaving,
            value if value == Self::ReceiveBroadcast as u8 => Self::ReceiveBroadcast,
            value if value == Self::Sleep as u8 => Self::Sleep,
            value if value == Self::TryYield as u8 => Self::TryYield,
            value if value == Self::TrySleep as u8 => Self::TrySleep,
            value if value == Self::SetTimeout as u8 => Self::SetTimeout,
            value if value == Self::MonotonicNow as u8 => Self::MonotonicNow,
            _ => return None,
        })
    }
}
