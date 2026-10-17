//! Shared `perf_hooks` projection for APIs used by Node packages.
//!
//! `performance` keeps Node's buffered user-timing entries: marks and
//! measures stay in its timeline until cleared.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const MODULE: &str = quench_js_check::checked_js!(
    r#"((now, query) => {
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
    getEntries() { return query(this, marks, measures, 0, undefined, undefined, arguments.length); },
    getEntriesByName(name, type = undefined) { return query(this, marks, measures, 1, name, type, arguments.length); },
    getEntriesByType(type) { return query(this, marks, measures, 2, type, undefined, arguments.length); },
    toJSON() { return { timeOrigin }; },
  };
  return { performance };
})"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(MODULE, "node:perf_hooks.js")?;
    let now = context.host_function(crate::host::shared_vm::operation("perfHooksNow"))?;
    let undefined = context.undefined();
    let data = context.null_object_rooted()?;
    let global = context.global_root()?;
    let array = super::event_constructor_shared_vm::get(context, global, "Array")?;
    let prototype = super::event_constructor_shared_vm::get(context, array, "prototype")?;
    let sort = super::event_constructor_shared_vm::get(context, prototype, "sort")?;
    let compare = context.host_function(crate::host::shared_vm::operation("perfHooksCompareEntries"))?;
    let coerce = context.evaluate_script_rooted(quench_js_check::checked_js!("value => `${value}`"), "node:perf_hooks/coerce.js")?;
    for (name, value) in [("sort", sort), ("compare", compare), ("coerce", coerce)] {
        let key = context.string_rooted(name);
        context.define_data_property_rooted(data, key, value, false, false, false)?;
    }
    let query = context.host_function_with_data(crate::host::shared_vm::operation("perfHooksEntries"), data)?;
    let module = context.call_rooted(factory, undefined, &[now, query])?;
    let performance = super::event_constructor_shared_vm::get(context, module, "performance")?;
    let brand = context.symbol_rooted(Some("performance"));
    let yes = context.boolean(true);
    context.define_data_property_rooted(performance, brand, yes, true, true, true)?;
    let key = context.string_rooted("brand");
    context.define_data_property_rooted(data, key, brand, false, false, false)?;
    // Actual configured SDK policy owns observer registration, buffering and dispatch.
    let observe = crate::host::shared_vm::commonjs::node_library_module(context, "perf_hooks.js", "internal/perf/observe.js")?;
    for name in ["PerformanceObserver", "PerformanceObserverEntryList"] {
        let key = context.string_rooted(name);
        let value = context.get_property_rooted(observe, key)?;
        context.define_data_property_rooted(module, key, value, true, true, true)?;
    }
    Ok(module)
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
