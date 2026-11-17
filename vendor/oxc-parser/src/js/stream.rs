//! A function body's statements parsed a run at a time, each run released from the arena once
//! read: a consumer that compiles statement by statement holds one run's tree, not the body's
//! (a 2.7 MB body of 150K statements is a ~45 MB tree).
use std::ptr::NonNull;

use oxc_allocator::GetAllocator;
use oxc_ast::ast::Statement;

use super::function::LazyBodyCheckpoint;
use crate::{ParserImpl, config::NoTokensParserConfig, context::{Context, StatementContext}, lexer::Kind};

/// The statements of a function body's text (its source between the braces), in the context
/// of a function body of the parse options' strictness.
pub struct StatementStream<'a> {
    parser: ParserImpl<'a, NoTokensParserConfig>,
}

/// What `StatementStream::next` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamStep {
    /// A run was read; more text follows.
    More,
    /// The text is all read.
    Done,
    /// The text does not parse as statements; nothing was read.
    Failed,
}

impl<'a> StatementStream<'a> {
    pub(crate) fn new(mut parser: ParserImpl<'a, NoTokensParserConfig>) -> Self {
        parser.token = parser.lexer.first_token();
        parser.ctx = parser.ctx.difference(Context::TopLevel).and_return(true).and_new_target(true);
        Self { parser }
    }

    fn failed(&self) -> bool {
        self.parser.fatal_error.is_some() || !self.parser.errors.is_empty() || !self.parser.lexer.errors.is_empty()
    }

    /// Parses the next statements, up to `max` of them or past `budget` arena bytes, passes each
    /// to `prepare` (which must keep nothing of it) and then the run to `read`, with the
    /// comments read so far, then releases
    /// them from the arena unless the parse left state that outlives them
    /// (`LazyBodyCheckpoint`) or took a new arena chunk.
    pub fn next(
        &mut self,
        max: usize,
        budget: usize,
        prepare: &mut dyn FnMut(&mut Statement<'a>),
        read: &mut dyn FnMut(&[Statement<'_>], &[oxc_ast::Comment]),
    ) -> StreamStep {
        if self.failed() {
            return StreamStep::Failed;
        }
        if self.parser.cur_kind() == Kind::Eof {
            return StreamStep::Done;
        }
        let parser = &mut self.parser;
        let allocator = parser.allocator();
        // The arena allocates downward: everything the run allocates lies before this.
        let marker = NonNull::from(allocator.alloc(0u8));
        let checkpoint = LazyBodyCheckpoint::of(parser);
        let used = allocator.used_bytes();
        let start = parser.cur_token().start();
        let mut statements = Vec::new();
        while statements.len() < max && parser.cur_kind() != Kind::Eof && parser.fatal_error.is_none() {
            statements.push(parser.parse_statement_list_item(StatementContext::StatementList));
            if statements.len() % 64 == 0 && allocator.used_bytes() - used > budget {
                break;
            }
        }
        if self.failed() {
            return StreamStep::Failed;
        }
        let parser = &mut self.parser;
        let end = parser.prev_token_end;
        statements.iter_mut().for_each(&mut *prepare);
        read(&statements, &parser.lexer.trivia_builder.comments);
        drop(statements);
        let escaped_after = parser.lexer.escaped_strings.any_from(end)
            || parser.lexer.escaped_templates.any_from(end);
        if !escaped_after && LazyBodyCheckpoint::of(parser) == checkpoint {
            parser.lexer.escaped_strings.remove_range(start..end);
            parser.lexer.escaped_templates.remove_range(start..end);
            // SAFETY: the arena has the chunk it had at `marker` (its capacity is unchanged) and
            // its alignment is one byte. Nothing allocated after the marker is referenced: the
            // statements are dropped and `read` held them only while it ran, the parser state
            // that can hold arena data is as it was at the checkpoint, the escaped strings and
            // templates of the run are removed and none lies past it (the next token's).
            unsafe { allocator.set_cursor_ptr(marker) };
        }
        StreamStep::More
    }
}
