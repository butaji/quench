use std::io::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};

pub trait Host {
    fn write_line(&mut self, text: &str);
    fn clock_millis(&mut self) -> f64;
}

#[derive(Default)]
pub struct SystemHost;

impl Host for SystemHost {
    fn write_line(&mut self, text: &str) {
        let _ = writeln!(io::stdout().lock(), "{text}");
    }

    fn clock_millis(&mut self) -> f64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64()
            * 1000.0
    }
}
