//! Node's `ERR_TEST_FAILURE` for a failing test, captured as report data: the fields its
//! TAP reporter writes (unwrapping the user's error for code and hook failures) and the
//! inspection its spec reporter prints.
use super::report::{Failure, Yaml};
use super::{get, Context};
use quench_runtime::{BuiltinBrand, RootId, RootedError};

const TEST_FAILURE_CODE: &str = "ERR_TEST_FAILURE";
const FRAME_START: &str = "    at ";
const ERROR_NAME: &str = "Error";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FailureType {
    TestCode,
    Hook,
    SubtestsFailed,
    CancelledByParent,
    CallbackAndPromisePresent,
    TestTimeout,
    UncaughtException,
    UnhandledRejection,
}

impl FailureType {
    fn tag(self) -> &'static str {
        match self {
            Self::TestCode => "testCodeFailure",
            Self::Hook => "hookFailed",
            Self::SubtestsFailed => super::report::SUBTESTS_FAILED,
            Self::CancelledByParent => "cancelledByParent",
            Self::CallbackAndPromisePresent => "callbackAndPromisePresent",
            Self::TestTimeout => "testTimeoutFailure",
            Self::UncaughtException => "uncaughtException",
            Self::UnhandledRejection => "unhandledRejection",
        }
    }

    /// Node's `kUnwrapErrors`: the failures whose report shows the user's error.
    fn unwraps(self) -> bool {
        matches!(self, Self::TestCode | Self::Hook | Self::UncaughtException | Self::UnhandledRejection)
    }
}

/// What failed: a value the test threw (or rejected with), or the runner's own message.
pub(super) enum Cause {
    Thrown(RootId),
    Message(String),
}

/// Whether the spec reporter also needs the coloured inspection.
#[derive(Clone, Copy)]
pub(super) struct Inspection {
    pub(super) colored: bool,
}

pub(super) fn capture(c: &mut Context<'_>, kind: FailureType, cause: Cause, inspection: Inspection) -> Result<Failure, RootedError> {
    let mut fields = vec![("failureType".to_owned(), Yaml::Scalar(inspect(c, &Value::Text(kind.tag()), false)?))];
    let (thrown, wrapper_message) = match &cause {
        Cause::Thrown(value) => (Some(*value), wrapper_message(c, *value)?),
        Cause::Message(message) => (None, message.clone()),
    };
    let unwrapped = thrown.filter(|_| kind.unwraps());
    let property = |c: &mut Context<'_>, name: &str| -> Result<Option<RootId>, RootedError> {
        match unwrapped {
            Some(value) if c.is_object_rooted(value)? => {
                let property = get(c, value, name)?;
                Ok((!c.rooted_value(property).is_some_and(|value| value.is_null() || value.is_undefined())).then_some(property))
            }
            _ => Ok(None),
        }
    };
    let message = property(c, "message")?;
    let error = match message {
        Some(message) => yaml(c, message, &mut Vec::new())?,
        None => Some(text_yaml(c, &wrapper_message)?),
    };
    fields.extend(error.map(|error| ("error".to_owned(), error)));
    let code = property(c, "code")?;
    match code {
        Some(code) if c.truthy_rooted(code)? => fields.extend(yaml(c, code, &mut Vec::new())?.map(|code| ("code".to_owned(), code))),
        Some(_) => {}
        None => fields.push(("code".to_owned(), text_yaml(c, TEST_FAILURE_CODE)?)),
    }
    if let Some(name) = property(c, "name")? {
        if c.truthy_rooted(name)? && c.string_text(name)?.as_deref() != Some(ERROR_NAME) {
            fields.extend(yaml(c, name, &mut Vec::new())?.map(|name| ("name".to_owned(), name)));
        }
    }
    if let Some(value) = unwrapped.filter(|value| assertion_like(c, *value).unwrap_or(false)) {
        let mut seen = vec![value];
        for name in ["expected", "actual"] {
            let member = get(c, value, name)?;
            fields.extend(yaml(c, member, &mut seen.clone())?.map(|member| (name.to_owned(), member)));
        }
        let operator = get(c, value, "operator")?;
        if c.truthy_rooted(operator)? {
            fields.extend(yaml(c, operator, &mut seen)?.map(|operator| ("operator".to_owned(), operator)));
        }
    }
    if let Some(stack) = property(c, "stack")? {
        if let Some(stack) = c.string_text(stack)? {
            let frames = stack
                .split('\n')
                .map(|line| line.trim_end_matches('\r'))
                .filter_map(|line| line.strip_prefix(FRAME_START))
                .filter(|frame| !frame.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if !frames.is_empty() {
                fields.push(("stack".to_owned(), Yaml::Lines(frames)));
            }
        }
    }
    let shown = match thrown {
        Some(value) => value,
        None => c.string_rooted(&wrapper_message),
    };
    let inspected = inspect(c, &Value::Root(shown), false)?;
    let inspected_colored = if inspection.colored { Some(inspect(c, &Value::Root(shown), true)?) } else { None };
    Ok(Failure { failure_type: kind.tag().to_owned(), fields, inspected, inspected_colored })
}

/// `ERR_TEST_FAILURE`'s message: the error's message, or the value, inspected unless a string.
fn wrapper_message(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    let message = if c.is_object_rooted(value)? { get(c, value, "message")? } else { c.undefined() };
    let message = if c.rooted_value(message).is_some_and(|message| message.is_null() || message.is_undefined()) {
        value
    } else {
        message
    };
    match c.string_text(message)? {
        Some(text) => Ok(text),
        None => inspect(c, &Value::Root(message), false),
    }
}

fn assertion_like(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    if !c.is_object_rooted(value)? {
        return Ok(false);
    }
    let expected = c.string_rooted("expected");
    let actual = c.string_rooted("actual");
    Ok(c.has_property_rooted(value, expected)? && c.has_property_rooted(value, actual)?)
}

enum Value<'a> {
    Root(RootId),
    Text(&'a str),
}

/// `util.inspect(value, { colors, breakLength: Infinity })`, the reporters' inspection.
fn inspect(c: &mut Context<'_>, value: &Value<'_>, colors: bool) -> Result<String, RootedError> {
    let value = match value {
        Value::Root(root) => *root,
        Value::Text(text) => c.string_rooted(text),
    };
    let util = super::super::util_shared_vm::module(c)?;
    let inspect = get(c, util, "inspect")?;
    let options = c.object_rooted()?;
    let colors = c.boolean(colors);
    super::put(c, options, "colors", colors)?;
    let unbounded = c.number(f64::INFINITY);
    super::put(c, options, "breakLength", unbounded)?;
    let text = c.call_rooted(inspect, util, &[value, options])?;
    c.to_string(text)
}

fn text_yaml(c: &mut Context<'_>, text: &str) -> Result<Yaml, RootedError> {
    let lines = split_lines(text);
    if lines.len() == 1 {
        return Ok(Yaml::Scalar(inspect(c, &Value::Text(text), false)?));
    }
    Ok(Yaml::Lines(lines))
}

fn split_lines(text: &str) -> Vec<String> {
    text.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned()).collect()
}

/// Node's TAP `jsToYaml` of one value; `None` for undefined, which it omits.
fn yaml(c: &mut Context<'_>, value: RootId, seen: &mut Vec<RootId>) -> Result<Option<Yaml>, RootedError> {
    let Some(raw) = c.rooted_value(value) else {
        return Ok(None);
    };
    if raw.is_undefined() {
        return Ok(None);
    }
    if raw.is_null() {
        return Ok(Some(Yaml::Null));
    }
    if !c.is_object_rooted(value)? {
        return match c.string_text(value)? {
            Some(text) => text_yaml(c, &text).map(Some),
            None => Ok(Some(Yaml::Scalar(inspect(c, &Value::Root(value), false)?))),
        };
    }
    seen.push(value);
    let is_error = c.has_builtin_brand_rooted(value, BuiltinBrand::NativeError)?;
    let global = c.global_root()?;
    let object = get(c, global, "Object")?;
    let entries = get(c, object, "entries")?;
    let entries = c.call_rooted(entries, object, &[value])?;
    let mut fields = Vec::new();
    for entry in c.array_like_values_rooted(entries)? {
        let key = get(c, entry, "0")?;
        let key = c.to_string(key)?;
        if is_error && (key == "cause" || key == "code") {
            continue;
        }
        let member = get(c, entry, "1")?;
        let mut circular = false;
        for previous in seen.iter() {
            if c.strict_equal_rooted(*previous, member)? {
                circular = true;
                break;
            }
        }
        if circular {
            fields.push((key, Yaml::Circular));
            continue;
        }
        if let Some(member) = yaml(c, member, seen)? {
            fields.push((key, member));
        }
    }
    Ok(Some(Yaml::Map(fields)))
}
