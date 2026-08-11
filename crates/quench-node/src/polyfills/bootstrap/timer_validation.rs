//! Polyfill: `timer-validation`

pub const JS: &str = quench_js_check::checked_js!(r#"const __nodeTimerDelay = (value) => {
  const delay = Number(value);
  return delay > 0 && delay <= 2147483647 ? delay : 0;
};
"#);
