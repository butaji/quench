//! Polyfill: `timer-validation`

pub const JS: &str = quench_js_check::checked_js!(r#"const __nodeTimerWarnings = new Set();
const __nodeTimerDelay = (value) => {
  const delay = Number(value);
  const warning = delay < 0
    ? ["TimeoutNegativeWarning", `${delay} is a negative number.`]
    : Number.isNaN(delay)
      ? ["TimeoutNaNWarning", `${delay} is not a number.`]
      : delay > 2147483647
        ? ["TimeoutOverflowWarning", `${delay} does not fit into a 32-bit signed integer.`]
        : null;
  if (warning && !__nodeTimerWarnings.has(warning[0])) {
    __nodeTimerWarnings.add(warning[0]);
    process.emitWarning(`${warning[1]}\nTimeout duration was set to 1.`, {
      name: warning[0]
    });
  }
  return delay > 0 && delay <= 2147483647 ? delay : 0;
};
"#);
