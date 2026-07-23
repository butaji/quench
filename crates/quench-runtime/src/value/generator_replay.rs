//! Generator yield replay — resume yields inside nested class computed keys.

use std::cell::{Cell, RefCell};

use crate::ast::{Class, ClassMember, Expression, PropertyKey, Statement};
use crate::value::Value;

thread_local! {
    static REPLAY: RefCell<ReplayState> = const { RefCell::new(ReplayState::empty()) };
}

#[derive(Debug, Default, Clone)]
struct ReplayState {
    yields_to_replay: usize,
    stored_resumes: Vec<Value>,
    cursor: usize,
    yields_this_run: usize,
    last_fresh_resume: Option<Value>,
}

impl ReplayState {
    const fn empty() -> Self {
        Self {
            yields_to_replay: 0,
            stored_resumes: Vec::new(),
            cursor: 0,
            yields_this_run: 0,
            last_fresh_resume: None,
        }
    }
}

pub fn begin_stmt_run(yields_to_replay: usize, stored: &[Value]) {
    REPLAY.with(|cell| {
        *cell.borrow_mut() = ReplayState {
            yields_to_replay,
            stored_resumes: stored.to_vec(),
            cursor: 0,
            yields_this_run: 0,
            last_fresh_resume: None,
        };
    });
}

thread_local! {
    static RESUMING_PENDING_YIELD: Cell<bool> = const { Cell::new(false) };
}

pub fn set_resuming_pending_yield(resuming: bool) {
    RESUMING_PENDING_YIELD.with(|cell| cell.set(resuming));
}

pub fn is_resuming_pending_yield() -> bool {
    RESUMING_PENDING_YIELD.with(|cell| cell.get())
}

pub fn yield_pending() -> bool {
    crate::interpreter::peek_generator_yield()
}

pub fn should_suspend_on_fresh_yield() -> bool {
    REPLAY.with(|cell| {
        let g = cell.borrow();
        if !is_resuming_pending_yield() {
            return true;
        }
        g.cursor + g.yields_this_run != g.yields_to_replay
    })
}

pub fn try_replay_yield() -> Option<Value> {
    REPLAY.with(|cell| {
        let mut g = cell.borrow_mut();
        if g.cursor < g.yields_to_replay {
            let v = g.stored_resumes[g.cursor].clone();
            g.cursor += 1;
            Some(v)
        } else {
            None
        }
    })
}

pub fn record_fresh_yield_resume(resume: Value) {
    REPLAY.with(|cell| {
        let mut g = cell.borrow_mut();
        if let Some(prev) = g.last_fresh_resume.take() {
            g.stored_resumes.push(prev);
        }
        g.last_fresh_resume = Some(resume);
        g.yields_this_run += 1;
    });
}

pub fn commit_suspend(stored: &mut Vec<Value>) {
    REPLAY.with(|cell| {
        let mut g = cell.borrow_mut();
        stored.append(&mut g.stored_resumes);
        g.last_fresh_resume = None;
        g.yields_this_run = 0;
        g.cursor = 0;
    });
}

pub fn commit_completed_yields(stored: &mut Vec<Value>) {
    REPLAY.with(|cell| {
        let mut g = cell.borrow_mut();
        stored.append(&mut g.stored_resumes);
        if let Some(r) = g.last_fresh_resume.take() {
            stored.push(r);
        }
        g.yields_this_run = 0;
        g.cursor = 0;
    });
}

pub fn count_yields_in_stmt(stmt: &Statement) -> usize {
    match stmt {
        Statement::Expression(expr) | Statement::Return(Some(expr)) => count_yields_in_expr(expr),
        _ => 0,
    }
}

pub fn count_yields_in_expr(expr: &Expression) -> usize {
    match expr {
        Expression::Yield(_) | Expression::YieldDelegate(_) => 1,
        Expression::Class(class) => count_yields_in_class(class),
        _ => 0,
    }
}

fn count_yields_in_class(class: &Class) -> usize {
    let mut n = class
        .super_class
        .as_ref()
        .map(|e| count_yields_in_expr(e))
        .unwrap_or(0);
    for member in &class.body {
        n += count_yields_in_class_member(member);
    }
    n
}

fn count_yields_in_class_member(member: &ClassMember) -> usize {
    match member {
        ClassMember::Constructor { body, .. } => {
            body.iter().map(count_yields_in_stmt).sum::<usize>()
        }
        ClassMember::Method { name, body, .. } | ClassMember::StaticMethod { name, body, .. } => {
            count_yields_in_property_key(name)
                + body.iter().map(count_yields_in_stmt).sum::<usize>()
        }
        ClassMember::Getter { name, body } | ClassMember::StaticGetter { name, body } => {
            count_yields_in_property_key(name)
                + body.iter().map(count_yields_in_stmt).sum::<usize>()
        }
        ClassMember::Setter { name, body, .. } | ClassMember::StaticSetter { name, body, .. } => {
            count_yields_in_property_key(name)
                + body.iter().map(count_yields_in_stmt).sum::<usize>()
        }
        ClassMember::Field { name, value } | ClassMember::StaticField { name, value } => {
            count_yields_in_property_key(name) + count_yields_in_expr(value)
        }
        ClassMember::StaticBlock { body } => body.iter().map(count_yields_in_stmt).sum::<usize>(),
    }
}

fn count_yields_in_property_key(key: &PropertyKey) -> usize {
    match key {
        PropertyKey::Computed(expr) => count_yields_in_expr(expr),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Class, ClassMember, Expression, PropertyKey};

    #[test]
    fn counts_yield_in_class_computed_getter() {
        let class = Class {
            name: None,
            super_class: None,
            body: vec![ClassMember::Getter {
                name: PropertyKey::Computed(Box::new(Expression::Yield(None))),
                body: vec![],
            }],
        };
        assert_eq!(count_yields_in_expr(&Expression::Class(class)), 1);
    }
}
