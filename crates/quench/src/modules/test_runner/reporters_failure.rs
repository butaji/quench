//! A failing test's error as Node's reporters read it: the TAP fields after `location`, and
//! the `util.inspect` text spec, dot and junit print. Child module of `reporters`.
use super::super::report::{Failure, Yaml};
use super::text_of;
use crate::host::shared_vm::commonjs::util_module;
use crate::modules::stream_shared_vm::{get, put};
use quench_runtime::{NativeContext, RootId, RootedError};


type Context<'a> = NativeContext<'a>;

/// A failing test's error, as Node's reporters show it: the error wrapper's `failureType`,
/// the cause's message and stack, and the wrapper's `util.inspect`.
pub(super) fn failure(c: &mut Context<'_>, error: RootId) -> Result<Failure, RootedError> {
    let cause = get(c, error, "cause")?;
    let has_cause = c.is_object_rooted(cause)?;
    let source = if has_cause { cause } else { error };
    let failure_type = text_of(c, error, "failureType")?.or(text_of(c, error, "code")?).unwrap_or_default();
    let message = text_of(c, source, "message")?.unwrap_or_default();
    let wrapper_message = text_of(c, error, "message")?.unwrap_or_default();
    let mut fields = Vec::new();
    if !failure_type.is_empty() {
        fields.push(("failureType".to_owned(), Yaml::Scalar(quote(&failure_type))));
    }
    if has_cause {
        fields.push(("cause".to_owned(), Yaml::Map(Vec::new())));
    }
    if !message.is_empty() {
        fields.push(("error".to_owned(), Yaml::Scalar(quote(&message))));
    }
    if let Some(stack) = text_of(c, source, "stack")? {
        fields.push(("stack".to_owned(), Yaml::Lines(frame_lines(&stack))));
    }
    let inspected = inspect(c, error)?;
    Ok(Failure { failure_type, message: wrapper_message, fields, inspected, inspected_colored: None })
}

/// The frames of a stack as Node's TAP shows them: no message header, and no `at ` prefix.
fn frame_lines(stack: &str) -> Vec<String> {
    stack
        .lines()
        .filter(|line| line.starts_with(char::is_whitespace))
        .map(|line| {
            let frame = line.trim_start();
            frame.strip_prefix("at ").unwrap_or(frame).to_owned()
        })
        .collect()
}

/// `util.inspect(value, { colors: false, breakLength: Infinity })`, as the reporters use.
fn inspect(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    let util = util_module(c)?;
    let method = get(c, util, "inspect")?;
    let options = c.object_rooted()?;
    let colors = c.boolean(false);
    put(c, options, "colors", colors)?;
    let break_length = c.number(f64::INFINITY);
    put(c, options, "breakLength", break_length)?;
    let text = c.call_rooted(method, util, &[value, options])?;
    Ok(c.string_text(text)?.unwrap_or_default())
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "\\'"))
}
