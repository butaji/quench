use proc_macro2::TokenStream;
use quote::quote;

/// Transform <Block title="..." borders={true}>...children...</Block>
/// into ratatui::widgets::Block + Paragraph or similar
pub fn widget_block(title: Option<&str>, borders: bool, children: Vec<TokenStream>) -> TokenStream {
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
            let text = format!("{}", #(#children)*);
            ratatui::widgets::Paragraph::new(text).block(block)
        }
    }
}

/// Transform <Text>Hello</Text>
pub fn widget_text(text: &str) -> TokenStream {
    quote! {
        ratatui::widgets::Paragraph::new(#text)
    }
}

/// Transform <Layout direction="vertical">...children...</Layout>
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

    quote! {
        {
            let layout = ratatui::layout::Layout::default()
                .direction(#dir)
                .constraints(vec![#(#constraints),*]);
            // Note: actual rendering would use layout.split() in the draw callback
            // For codegen, we return the layout + children tuple
            (layout, vec![#(#children),*])
        }
    }
}

fn tui_setup() -> TokenStream {
    quote! {
        use ratatui::prelude::*;
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

// allow:complexity
fn tui_handle_events() -> TokenStream {
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

fn tui_loop_body(app_body: TokenStream) -> TokenStream {
    let events = tui_handle_events();
    quote! {
        loop {
            if should_quit { break; }
            terminal.draw(|f| {
                let area = f.size();
                #app_body
            })?;
            #events
        }
    }
}

fn tui_cleanup() -> TokenStream {
    quote! {
        disable_raw_mode()?;
        execute!(std::io::stdout(), LeaveAlternateScreen)?;
    }
}

/// Generate main function for TUI app
pub fn tui_main(app_body: TokenStream) -> TokenStream {
    let setup = tui_setup();
    let body = tui_loop_body(app_body);
    let cleanup = tui_cleanup();
    quote! {
        fn main() -> anyhow::Result<()> {
            #setup
            let mut should_quit = false;
            #body
            #cleanup
            Ok(())
        }
    }
}