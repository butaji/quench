//! Pure-Rust common MIME database subset used by Express/body-parser.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::{execute::VmError, host_api, value::Value};

use crate::host::HostState;

fn entry(source: &str, compressible: Option<bool>, extensions: &[&str]) -> Value {
    let mut props = vec![("source".to_string(), Value::String(source.to_string()))];
    if let Some(flag) = compressible {
        props.push(("compressible".to_string(), Value::Boolean(flag)));
    }
    if !extensions.is_empty() {
        props.push((
            "extensions".to_string(),
            host_api::array(
                extensions
                    .iter()
                    .map(|x| Value::String((*x).into()))
                    .collect(),
            ),
        ));
    }
    host_api::object(props)
}

pub fn build(state: &Rc<RefCell<HostState>>) -> Result<Value, VmError> {
    if let Some(cached) = state.borrow().mime_db_module.clone() {
        return Ok(cached);
    }
    let value = build_entries();
    state.borrow_mut().mime_db_module = Some(value.clone());
    Ok(value)
}

fn build_entries() -> Value {
    host_api::object(vec![
        (
            "application/json".into(),
            entry("iana", Some(true), &["json", "map"]),
        ),
        (
            "application/javascript".into(),
            entry("apache", Some(true), &["js"]),
        ),
        ("application/octet-stream".into(), entry("iana", None, &[])),
        (
            "application/xml".into(),
            entry("iana", Some(true), &["xml"]),
        ),
        (
            "text/plain".into(),
            entry(
                "iana",
                Some(true),
                &["txt", "text", "conf", "def", "list", "log", "in", "ini"],
            ),
        ),
        (
            "text/html".into(),
            entry("iana", Some(true), &["html", "htm", "shtml"]),
        ),
        ("text/css".into(), entry("iana", Some(true), &["css"])),
        ("text/javascript".into(), entry("iana", Some(true), &["js"])),
        ("image/png".into(), entry("iana", Some(false), &["png"])),
        (
            "image/jpeg".into(),
            entry("iana", Some(false), &["jpg", "jpeg", "jpe"]),
        ),
        ("image/gif".into(), entry("iana", Some(false), &["gif"])),
        (
            "image/svg+xml".into(),
            entry("iana", Some(true), &["svg", "svgz"]),
        ),
        (
            "video/mp4".into(),
            entry("iana", Some(false), &["mp4", "mp4v", "mpg4"]),
        ),
        (
            "application/pdf".into(),
            entry("iana", Some(false), &["pdf"]),
        ),
        (
            "application/zip".into(),
            entry("iana", Some(false), &["zip"]),
        ),
        ("multipart/form-data".into(), entry("iana", None, &[])),
    ])
}
