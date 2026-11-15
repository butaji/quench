//! V8's magic comments, `//# sourceURL=<value>` and `//# sourceMappingURL=<value>`: read once
//! from the parse's comment table at load and kept with the program's source, never found
//! again by scanning the text.
use {crate::vm::is_ecma_whitespace_character as white_space, oxc_ast::ast::{Comment, CommentKind}};

const SOURCE_URL: &str = "sourceURL";
const SOURCE_MAP_URL: &str = "sourceMappingURL";
const LINE_COMMENT_OPENER: &str = "//";
const SIGILS: [char; 2] = ['#', '@'];

/// The values of a source's last `sourceURL` and `sourceMappingURL` directives; a directive
/// whose value is absent, empty or not one word replaces an earlier one with nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SourceDirectives {
    pub(crate) source_url: Option<Box<str>>,
    pub(crate) source_map_url: Option<Box<str>>,
}

impl SourceDirectives {
    pub(crate) fn of_comments(text: &str, comments: &[Comment]) -> Self {
        let mut directives = Self::default();
        for comment in comments.iter().filter(|comment| comment.kind == CommentKind::Line) {
            let Some(body) = text.get(comment.span.start as usize..comment.span.end as usize) else { continue };
            if let Some((name, value)) = body.strip_prefix(LINE_COMMENT_OPENER).and_then(directive) {
                match name {
                    SOURCE_URL => directives.source_url = value.map(Into::into),
                    SOURCE_MAP_URL => directives.source_map_url = value.map(Into::into),
                    _ => (),
                }
            }
        }
        directives
    }
}

impl SourceDirectives {
    pub(crate) fn write_snapshot(&self, writer: &mut crate::snapshot::Writer) {
        for value in [&self.source_url, &self.source_map_url] {
            writer.bool(value.is_some());
            if let Some(value) = value {
                writer.str(value);
            }
        }
    }

    pub(crate) fn read_snapshot(reader: &mut crate::snapshot::Reader<'_>) -> crate::snapshot::SnapshotResult<Self> {
        let mut value = || Ok(if reader.bool()? { Some(reader.str()?.into()) } else { None });
        Ok(Self { source_url: value()?, source_map_url: value()? })
    }
}

impl CompiledScript {
    /// The value of the source's last `//# sourceURL=` directive, found when it loaded.
    pub fn source_url(&self) -> Option<&str> {
        self.directives().and_then(|directives| directives.source_url.as_deref())
    }

    /// The value of the source's last `//# sourceMappingURL=` directive.
    pub fn source_map_url(&self) -> Option<&str> {
        self.directives().and_then(|directives| directives.source_map_url.as_deref())
    }

    fn directives(&self) -> Option<&crate::bytecode::SourceDirectives> {
        self.state.program.source.as_ref().map(|source| &source.directives)
    }
}

/// `[#@]`, one white space, `name=`, optional white space, one word and nothing after it:
/// the name and its value, which is none when what follows `=` is not exactly that.
fn directive(comment: &str) -> Option<(&str, Option<&str>)> {
    let comment = comment.strip_prefix(SIGILS)?.strip_prefix(white_space)?;
    let (name, value) = comment.split_once('=')?;
    let value = value.trim_start_matches(white_space);
    let (word, rest) = value.split_at(value.find(white_space).unwrap_or(value.len()));
    Some((name, (!word.is_empty() && rest.trim_start_matches(white_space).is_empty()).then_some(word)))
}

#[cfg(test)]
mod tests {
    type Found = (Option<&'static str>, Option<&'static str>);

    fn directives(source: &str) -> super::SourceDirectives {
        let program = crate::Engine::specialize(source, "directives.js").expect("compiles");
        program.source.expect("a script keeps its source").directives
    }

    /// Node 24's `vm.Script` answers for each source: its `sourceURL` and `sourceMapURL`.
    #[test]
    fn directives_follow_v8_magic_comments() {
        let cases: [(&str, Found); 27] = [
            ("1\n//# sourceURL=a.js", (Some("a.js"), None)),
            ("1\n//@ sourceURL=a.js", (Some("a.js"), None)),
            ("//# sourceURL=a\n//# sourceURL=b\n", (Some("b"), None)),
            ("//# sourceURL=a\n//# sourceURL=\n", (None, None)),
            ("//# sourceURL=a\n//# sourceURL=b c\n", (None, None)),
            ("/*# sourceURL=blk.js */", (None, None)),
            ("/*\n//# sourceURL=blk.js\n*/ 1", (None, None)),
            ("var s = `\n//# sourceURL=tpl.js\n`;", (None, None)),
            ("var s = 'x'; //# sourceURL=trail.js", (Some("trail.js"), None)),
            ("   \t//# sourceURL=ind.js\n", (Some("ind.js"), None)),
            ("//#sourceURL=ns.js", (None, None)),
            ("// # sourceURL=sp.js", (None, None)),
            ("//#  sourceURL=ts.js", (None, None)),
            ("//# sourceURL = se.js", (None, None)),
            ("//# sourceURL= af.js", (Some("af.js"), None)),
            ("//# sourceURL=t.js   \n", (Some("t.js"), None)),
            ("//# sourceURL=t.js junk\n", (None, None)),
            ("//# sourceURL=crlf.js\r\n1", (Some("crlf.js"), None)),
            ("//# sourceMappingURL=m.map\n", (None, Some("m.map"))),
            ("//# sourceURL=u.js\n//# sourceMappingURL=m.map\n", (Some("u.js"), Some("m.map"))),
            ("//# sourceURLx=a", (None, None)),
            ("var r = /\\//; //# sourceURL=r.js", (Some("r.js"), None)),
            ("//# sourceURL=ls.js\u{2028} 1", (Some("ls.js"), None)),
            ("//#\tsourceURL=tab.js", (Some("tab.js"), None)),
            ("#!/x\n//# sourceURL=hb.js", (Some("hb.js"), None)),
            ("//# sourceURL=eof.js", (Some("eof.js"), None)),
            ("var r = /[//# sourceURL=x.js]/;", (None, None)),
        ];
        for (source, (url, map)) in cases {
            let found = directives(source);
            assert_eq!((found.source_url.as_deref(), found.source_map_url.as_deref()), (url, map), "{source:?}");
        }
    }

    #[test]
    fn directives_survive_the_program_encoding() {
        let source = "1\n//# sourceURL=enc.js\n//# sourceMappingURL=enc.map";
        let program = crate::Engine::specialize(source, "enc.js").expect("compiles");
        let bytes: crate::bytecode::KeptBytes = std::rc::Rc::new(program.encode_trusted());
        let decoded = crate::ResidualProgram::decode_trusted(bytes).expect("decodes");
        assert_eq!(decoded.source.expect("source").directives, directives(source));
    }
}
