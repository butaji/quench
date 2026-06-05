# Task 001: Architecture Review & Task Planning

## Overview
Comprehensive code and architecture review of runts-ink for parity testing.

## Project Structure Analysis

### Core Crates
- `runts-ink`: Ink-compatible TUI component types (Rust)
- `runts-ratatui`: Plugin for ratatui rendering
- `runts-lib`: Runtime library
- `runts-macros`: Procedural macros

### Examples Structure
- 80 Ink examples in `examples/ink-*`
- Each has: `main.tsx`, `tui/app.tsx`, `deno.json`, `runts.config.json`
- All examples import from `ink` npm package (Deno) or `@runts/ink` (runts)

### Supported Features (from components.rs, events.rs, style.rs)

#### Components
- `Box`: Flexbox container with full layout props
- `Text`: Styled text with color, bold, italic, etc.
- `Newline`: Vertical separator
- `Spacer`: Flexbox space filler
- `Static`: Pre-rendered fragment
- `Transform`: Position offset wrapper

#### Hooks (JS-side via React reconciler)
- `useInput`: Keyboard input handling
- `useApp`: Terminal/app state
- `useFocus`: Focus management
- `useStdin`/`useStdout`/`useStderr`: I/O streams
- `useWindowSize`: Terminal dimensions
- `useMeasure`: Box dimensions
- `useEnterSubmit`: Form submission
- `useRerender`: Manual re-render trigger
- `useTab`: Tab focus cycling

#### Layout Props
- `flexDirection`: row, column, row-reverse, column-reverse
- `flexWrap`: nowrap, wrap, wrap-reverse
- `alignItems`, `alignSelf`, `alignContent`
- `justifyContent`
- `gap`, `columnGap`, `rowGap`
- `padding`, `paddingX`, `paddingY`
- `margin`
- `width`, `height`, `minWidth`, `minHeight`, `maxWidth`, `maxHeight`
- `position`: relative, absolute
- `top`, `left`, `right`, `bottom`

#### Style Props
- `color`: Named colors + hex
- `backgroundColor`
- `borderStyle`: single, double, round, bold, classic
- `borderColor`, `borderDimColor`, `borderBackgroundColor`
- `bold`, `italic`, `underline`, `strikethrough`, `dimColor`, `inverse`
- `wrap`: wrap, hard, truncate, truncate-middle
- `cursor`
- `display`: flex, none
- `overflow`: visible, hidden

### Three Environments for Parity
1. **Deno**: Real Ink npm package execution
2. **runts dev**: HIR runtime with rquickjs + hot-reload
3. **runts build**: In-memory Rust codegen + cargo build

## Status: COMPLETED
