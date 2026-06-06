//! Ratatui app generation functions

use proc_macro2::TokenStream;
use quote::quote;

/// Generate a block widget with title and borders, rendering children into inner area.
pub fn widget_block(title: Option<&str>, borders: bool, children: TokenStream) -> TokenStream {
    let title_quote = title.map(|t| quote! { .title(#t) });
    let borders_quote = if borders {
        quote! { .borders(ratatui::widgets::Borders::ALL) }
    } else {
        quote! {}
    };

    quote! {
        {
            let block = ratatui::widgets::Block::default()
                #title_quote
                #borders_quote;
            frame.render_widget(block, area);
            let inner = block.inner(area);
            #children
        }
    }
}

/// Generate a text/paragraph widget.
pub fn widget_text(text: &str) -> TokenStream {
    quote! {
        frame.render_widget(ratatui::widgets::Paragraph::new(#text), area);
    }
}

/// Generate a layout widget that splits area and renders children.
pub fn widget_layout(direction: &str, children: Vec<TokenStream>) -> TokenStream {
    let dir = match direction {
        "vertical" => quote! { ratatui::layout::Direction::Vertical },
        "horizontal" => quote! { ratatui::layout::Direction::Horizontal },
        _ => quote! { ratatui::layout::Direction::Vertical },
    };

    let child_count = children.len();
    let constraints: Vec<TokenStream> = (0..child_count)
        .map(|_| quote! { ratatui::layout::Constraint::Percentage(100 / #child_count as u16) })
        .collect();

    let renders: Vec<TokenStream> = children
        .iter()
        .map(|child| quote! { #child })
        .collect();

    quote! {
        {
            let layout = ratatui::layout::Layout::default()
                .direction(#dir)
                .constraints(vec![#(#constraints),*]);
            let chunks = layout.split(area);
            #(#renders)*
        }
    }
}

/// Generate panic-safe cleanup using TerminalGuard struct.
pub fn tui_cleanup() -> TokenStream {
    quote! {
        struct TerminalGuard;
        impl Drop for TerminalGuard {
            fn drop(&mut self) {
                let _ = disable_raw_mode();
                let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
            }
        }
    }
}

/// Generate terminal setup code.
pub fn tui_setup() -> TokenStream {
    quote! {
        use ratatui::backend::CrosstermBackend;
        use ratatui::Terminal;
        use crossterm::{
            event::{self, Event, KeyCode, KeyEventKind},
            execute,
            terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
        };

        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
    }
}

pub fn tui_handle_events() -> TokenStream {
    quote! {
        if crossterm::event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => should_quit = true,
                        _ => {}
                    }
                }
            }
        }
    }
}

pub fn tui_loop_body(app_body: TokenStream) -> TokenStream {
    let events = tui_handle_events();
    quote! {
        loop {
            if should_quit { break; }
            terminal.draw(|frame| {
                let area = frame.size();
                #app_body
            })?;
            #events
        }
    }
}

/// Generate main function for TUI app
pub fn tui_main(app_body: TokenStream) -> TokenStream {
    let setup = tui_setup();
    let cleanup = tui_cleanup();
    let body = tui_loop_body(app_body);
    quote! {
        fn main() -> anyhow::Result<()> {
            #cleanup
            #setup
            let _guard = TerminalGuard;
            let mut should_quit = false;
            #body
            Ok(())
        }
    }
}
