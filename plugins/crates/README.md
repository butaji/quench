# runts plugins/crates

Framework integration crates for runts (Rust TypeScript compiler).

## Structure

```
plugins/crates/
├── runts-ink/        -> ../../crates/runts-ink/
└── runts-ratatui/    -> ../../crates/runts-ratatui/
```

## Usage

These are symlinked to the main `crates/` directory for easy access.

### runts-ink

Framework-agnostic Ink-style TUI components. Provides:
- `Box`, `Text`, `Spacer`, `Newline`, `Static`, `Transform` components
- Hooks: `useInput`, `useFocus`, `useFocusManager`, `useApp`, `useStdin`, `useStdout`, `useStderr`, `useWindowSize`, `useCursor`
- Yoga-based flexbox layout
- Ratatui rendering backend

### runts-ratatui

Ratatui integration for runts. Provides:
- Terminal event handling (keyboard, mouse, resize)
- Render loop with crossterm
- Ink component rendering to Ratatui widgets

## Building

```bash
# Build all plugins
cargo build -p runts-ink -p runts-ratatui

# Build with examples
cargo build --examples -p runts-ink
```
