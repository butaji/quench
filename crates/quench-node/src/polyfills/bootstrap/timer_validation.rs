//! Polyfill: `timer-validation`

pub const JS: &str = r#"const __nodeTimerDelay = (value) => {
  const delay = Number(value);
  return delay > 0 && delay <= 2147483647 ? delay : 0;
};
"#;
