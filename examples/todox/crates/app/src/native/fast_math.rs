//! Fast math utilities written in Rust.
//!
//! These functions are callable from Rune files via:
//! `import { fast_sqrt } from "native:fast_math";`

use protocol::Task;

/// Fast square root approximation using Newton's method.
pub fn fast_sqrt(x: f64) -> f64 {
    if x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return 0.0;
    }
    
    let mut guess = x / 2.0;
    for _ in 0..10 {
        guess = (guess + x / guess) / 2.0;
    }
    guess
}

/// Calculate task priority score based on properties.
pub fn task_priority(task: &Task) -> f64 {
    let mut score = 0.0;
    if task.done {
        score -= 10.0;
    }
    score += (task.title.len() as f64).sqrt() * 0.5;
    score
}

/// Batch toggle tasks by IDs.
pub fn batch_toggle_by_id(tasks: &mut Vec<Task>, ids: &[i32]) {
    for id in ids {
        if let Some(task) = tasks.iter_mut().find(|t| t.id == *id) {
            task.done = !task.done;
        }
    }
}
