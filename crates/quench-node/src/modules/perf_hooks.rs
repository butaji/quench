//! Shared `perf_hooks` projection for APIs used by Node packages.
//!
//! `performance` keeps Node's buffered user-timing entries: marks and
//! measures stay in its timeline until cleared.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const MODULE: &str = quench_js_check::checked_js!(
    r#"((now) => {
  const marks = [];
  const measures = [];
  const timeOrigin = Date.now() - now();
  const entry = (name, entryType, startTime, duration, detail) => ({
    name, entryType, startTime, duration, detail,
    toJSON() { return { name, entryType, startTime, duration, detail }; },
  });
  const markTime = (mark) => {
    if (typeof mark === 'number') return mark;
    const name = String(mark);
    for (let index = marks.length - 1; index >= 0; index--) {
      if (marks[index].name === name) return marks[index].startTime;
    }
    const error = new SyntaxError(`The "${name}" performance mark has not been set`);
    error.code = 'ERR_INVALID_PERFORMANCE_MARK';
    throw error;
  };
  const clear = (entries, name) => {
    for (let index = entries.length - 1; index >= 0; index--) {
      if (name === undefined || entries[index].name === String(name)) entries.splice(index, 1);
    }
  };
  const timeline = () => [...marks, ...measures].sort((a, b) => a.startTime - b.startTime);
  const performance = {
    now,
    timeOrigin,
    mark(name, options = {}) {
      const mark = entry(String(name), 'mark', options?.startTime ?? now(), 0,
        options?.detail ?? null);
      marks.push(mark);
      return mark;
    },
    measure(name, startOrOptions, endMark) {
      let start = 0;
      let end;
      let detail = null;
      if (startOrOptions !== null && typeof startOrOptions === 'object') {
        if (startOrOptions.start !== undefined) start = markTime(startOrOptions.start);
        if (startOrOptions.end !== undefined) end = markTime(startOrOptions.end);
        if (startOrOptions.duration !== undefined) {
          if (end === undefined) end = start + startOrOptions.duration;
          else start = end - startOrOptions.duration;
        }
        detail = startOrOptions.detail ?? null;
      } else if (startOrOptions !== undefined) {
        start = markTime(startOrOptions);
      }
      if (endMark !== undefined) end = markTime(endMark);
      if (end === undefined) end = now();
      const measure = entry(String(name), 'measure', start, end - start, detail);
      measures.push(measure);
      return measure;
    },
    clearMarks(name) { clear(marks, name); },
    clearMeasures(name) { clear(measures, name); },
    getEntries: timeline,
    getEntriesByName(name, type) {
      return timeline().filter((item) => item.name === String(name) &&
        (type === undefined || item.entryType === String(type)));
    },
    getEntriesByType(type) { return timeline().filter((item) => item.entryType === String(type)); },
    toJSON() { return { timeOrigin }; },
  };
  return { performance };
})"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(MODULE, "node:perf_hooks.js")?;
    let now = context.host_function(crate::host::shared_vm::operation("perfHooksNow"))?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[now])
}

pub(crate) fn now(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let elapsed_ms = context
        .host_mut()
        .shared_state()
        .borrow()
        .process_control
        .uptime()
        * 1_000.0;
    Ok(context.number(elapsed_ms))
}
