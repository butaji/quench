# Task 026: Render Text (Paragraph/Span)

## Goal
Render `ink-text` nodes as ratatui `Paragraph` / `Span` with full style support.

## Acceptance Criteria
- [ ] Text content rendered inside Yoga rect using `Paragraph::new`.
- [ ] `color` / `bg` / `bold` / `dimColor` / `italic` / `underline` / `strikethrough` / `inverse` mapped to ratatui `Style`.
- [ ] `wrap` prop enables `Wrap { trim }`.
- [ ] Unit test: styled text renders correct ANSI sequences in Buffer.

## Dependencies
- Task 025

## SPEC Reference
§3.2 Renderer
