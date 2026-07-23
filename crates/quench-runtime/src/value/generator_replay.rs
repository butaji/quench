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
        if g.cursor >= g.yields_to_replay {
            return None;
        }
        let idx = g.cursor;
        g.cursor += 1;
        let v = if is_resuming_pending_yield() && idx + 1 == g.yields_to_replay {
            crate::interpreter::take_generator_resume_value()
        } else {
            g.stored_resumes
                .get(idx)
                .cloned()
                .unwrap_or(Value::Undefined)
        };
        if idx >= g.stored_resumes.len() {
            g.stored_resumes.push(v.clone());
        } else {
            g.stored_resumes[idx] = v.clone();
        }
        Some(v)
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
        *stored = g.stored_resumes.clone();
        if let Some(r) = g.last_fresh_resume.take() {
            stored.push(r);
        }
        g.yields_this_run = 0;
        g.cursor = 0;
    });
}

pub fn commit_completed_yields(stored: &mut Vec<Value>) {
    REPLAY.with(|cell| {
        let mut g = cell.borrow_mut();
        *stored = g.stored_resumes.clone();
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
        Expression::Yield(inner) => {
            1 + inner
                .as_ref()
                .map(|e| count_yields_in_expr(e))
                .unwrap_or(0)
        }
        Expression::YieldDelegate(inner) => 1 + count_yields_in_expr(inner),
        Expression::Spread(inner) => count_yields_in_expr(inner),
        Expression::Array(elements) => elements.iter().map(count_yields_in_expr).sum(),
        Expression::Class(class) => count_yields_in_class(class),
        Expression::Object(props) => props
            .iter()
            .map(|(_, value)| count_yields_in_property_value(value))
            .sum(),
        Expression::Binary { left, right, .. }
        | Expression::LogicalCompoundAssignment { left, right, .. }
        | Expression::CompoundAssignment { left, right, .. }
        | Expression::Assignment { left, right } => {
            count_yields_in_expr(left) + count_yields_in_expr(right)
        }
        Expression::Unary { argument, .. } => count_yields_in_expr(argument),
        Expression::Update { argument, .. } => count_yields_in_expr(argument),
        Expression::Call { callee, arguments } => {
            count_yields_in_expr(callee) + arguments.iter().map(count_yields_in_expr).sum::<usize>()
        }
        Expression::Member { object, property, .. } => {
            count_yields_in_expr(object) + count_yields_in_property_key(property)
        }
        Expression::Conditional {
            condition,
            consequent,
            alternate,
        } => {
            count_yields_in_expr(condition)
                + count_yields_in_expr(consequent)
                + count_yields_in_expr(alternate)
        }
        Expression::New { constructor, arguments } => {
            count_yields_in_expr(constructor)
                + arguments.iter().map(count_yields_in_expr).sum::<usize>()
        }
        Expression::Sequence(exprs) => exprs.iter().map(count_yields_in_expr).sum(),
        Expression::BlockExpr(stmts) => stmts.iter().map(count_yields_in_stmt).sum(),
        _ => 0,
    }
}

fn count_yields_in_property_value(value: &crate::ast::PropertyValue) -> usize {
    use crate::ast::PropertyValue;
    match value {
        PropertyValue::Value(expr) => count_yields_in_expr(expr),
        PropertyValue::Getter { body, .. } | PropertyValue::Setter { body, .. } => {
            body.iter().map(count_yields_in_stmt).sum()
        }
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

    #[test]
    fn counts_nested_yield_in_array_spread() {
        let expr = Expression::Yield(Some(Box::new(Expression::Array(vec![
            Expression::Spread(Box::new(Expression::Yield(None))),
        ]))));
        assert_eq!(count_yields_in_expr(&expr), 2);
    }
}
