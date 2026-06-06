use std::sync::{Arc, Mutex};
use std::time::Duration;
use crossterm::event::{self, Event as CtEvent};
use crate::events::{InputEvent, ResizeEvent, WindowSize};
use super::InstanceInner;

pub(crate) fn input_loop(
    inner: Arc<Mutex<InstanceInner>>,
    tick: Duration,
    exit_on_q: bool,
    exit_on_ctrl_c: bool,
) {
    use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
    while inner.lock().unwrap().running {
        if event::poll(tick).unwrap_or(false) {
            match event::read() {
                Ok(CtEvent::Key(key)) if key.kind == KeyEventKind::Press => {
                    let ev = InputEvent::from_crossterm(key.clone());
                    inner.lock().unwrap().input_log.push(ev);
                    if exit_on_q
                        && key.code == KeyCode::Char('q')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        inner.lock().unwrap().running = false;
                    } else if exit_on_q && key.code == KeyCode::Char('q') {
                        inner.lock().unwrap().running = false;
                    } else if exit_on_ctrl_c
                        && key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        inner.lock().unwrap().running = false;
                    }
                }
                Ok(CtEvent::Resize(w, h)) => {
                    let sz = WindowSize { columns: w, rows: h };
                    inner.lock().unwrap().last_size = sz;
                    inner.lock().unwrap().resize_log.push(ResizeEvent { width: w, height: h });
                }
                Ok(CtEvent::Paste(s)) => {
                    inner.lock().unwrap().paste_log.push(s);
                }
                _ => {}
            }
        }
    }
}
