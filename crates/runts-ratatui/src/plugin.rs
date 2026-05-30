//! Ratatui plugin implementation.

use runts_plugin::{
    CargoDep, DevAction, DevContext, DevState, Plugin,
};

pub struct RatatuiPlugin;

struct RatatuiDevState;

impl DevState for RatatuiDevState {}

impl Plugin for RatatuiPlugin {
    fn name(&self) -> &str {
        "ratatui"
    }

    fn help_text(&self) -> &str {
        "Ratatui TUI framework"
    }

    fn codegen_module(&self, _hir_str: &str) -> anyhow::Result<String> {
        Ok(r#"
use ratatui::{Terminal, Frame, backend::CrosstermBackend};
use std::io;

pub fn run_app<B: ratatui::backend::Backend>(frame: &mut Frame<B>) {
    // Your TUI app logic here
}
"#.to_string())
    }

    fn cargo_deps(&self) -> Vec<CargoDep> {
        vec![
            CargoDep {
                name: "ratatui".to_string(),
                version: Some("0.26".to_string()),
                path: None,
                features: vec!["crossterm".to_string()],
            },
            CargoDep {
                name: "crossterm".to_string(),
                version: Some("0.27".to_string()),
                path: None,
                features: vec![],
            },
            CargoDep {
                name: "tokio".to_string(),
                version: Some("1.0".to_string()),
                path: None,
                features: vec!["full".to_string()],
            },
        ]
    }

    fn codegen_entry(&self, _modules: &[runts_plugin::hir::Module]) -> anyhow::Result<String> {
        Ok(r#"
use ratatui::{Terminal, backend::CrosstermBackend, widgets::Paragraph};
use crossterm::event::{self, Event, KeyCode};
use std::io;

fn main() -> io::Result<()> {
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    loop {
        terminal.draw(|frame| {
            let area = frame.size();
            let widget = Paragraph::new("Hello from Ratatui!");
            frame.render_widget(widget, area);
        })?;

        if let Event::Key(key) = event::read()? {
            if let KeyCode::Char('q') = key.code {
                break;
            }
        }
    }

    Ok(())
}
"#.to_string())
    }

    fn dev_init(&self, _ctx: &mut DevContext) -> anyhow::Result<Box<dyn DevState>> {
        Ok(Box::new(RatatuiDevState))
    }

    fn dev_run_once(&self, _state: &mut dyn DevState) -> anyhow::Result<DevAction> {
        Ok(DevAction::Continue)
    }

    fn dev_reload(&self, _ctx: &mut DevContext, _state: &mut dyn DevState) -> anyhow::Result<()> {
        Ok(())
    }
}
