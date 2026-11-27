//! Glob patterns (`path.matchesGlob`, `fs.glob`, `fs.watch`'s `ignore`), parsed once into
//! tokens: matched here by hand, so matching links no regex engine, and rendered as a RegExp
//! source where JavaScript matches them. Matching follows minimatch where Node does: a leading
//! `!` or `^` negates a class, and a wildcard never matches a segment's leading `.`.

#[derive(Clone, PartialEq)]
enum Token {
    Literal(char),
    /// `*`: `[^/]*`.
    Run,
    /// `**/` at a segment's start: `(?:[^/]+/)*`.
    Segments,
    /// Any other `**`: `.*`, which stops at a line feed.
    Any,
    /// `?`: `[^/]`.
    One,
    /// `[...]`, its members as written; a leading `!` or `^` negates, as in minimatch.
    Class { negated: bool, members: Vec<char> },
}

pub(crate) struct Glob(Vec<Token>);

/// A class member set as a RegExp class reads it: single characters and `a-z` ranges.
struct Members(Vec<(char, char)>);

impl Glob {
    pub(crate) fn new(pattern: &str) -> Self {
        let chars: Vec<char> = pattern.chars().collect();
        let mut tokens = Vec::new();
        let mut index = 0;
        while index < chars.len() {
            match chars[index] {
                '*' if chars.get(index + 1) == Some(&'*') => {
                    let segment_start = index == 0 || chars[index - 1] == '/';
                    if segment_start && chars.get(index + 2) == Some(&'/') {
                        tokens.push(Token::Segments);
                        index += 2;
                    } else {
                        tokens.push(Token::Any);
                        index += 1;
                    }
                }
                '*' => tokens.push(Token::Run),
                '?' => tokens.push(Token::One),
                '[' => match chars[index + 1..].iter().position(|ch| *ch == ']') {
                    Some(end) => {
                        let end = index + 1 + end;
                        let negated = matches!(chars.get(index + 1), Some('!' | '^'));
                        let start = index + 1 + usize::from(negated);
                        tokens.push(Token::Class { negated, members: chars[start..end].to_vec() });
                        index = end;
                    }
                    None => tokens.push(Token::Literal('[')),
                },
                ch => tokens.push(Token::Literal(ch)),
            }
            index += 1;
        }
        Self(tokens)
    }

    /// The pattern as an anchored RegExp source.
    pub(crate) fn regex_source(&self) -> String {
        let mut source = String::from("^");
        for token in &self.0 {
            match token {
                Token::Literal(ch) => {
                    if ".+()|{}^$\\[".contains(*ch) {
                        source.push('\\');
                    }
                    source.push(*ch);
                }
                Token::Run => source.push_str("[^/]*"),
                Token::Segments => source.push_str("(?:[^/]+/)*"),
                Token::Any => source.push_str(".*"),
                Token::One => source.push_str("[^/]"),
                Token::Class { negated, members } => {
                    source.push('[');
                    if *negated {
                        source.push('^');
                    }
                    for ch in members {
                        if *ch == '\\' {
                            source.push_str("\\\\");
                        } else {
                            source.push(*ch);
                        }
                    }
                    source.push(']');
                }
            }
        }
        source.push('$');
        source
    }

    /// Whether `text` matches the whole pattern; `None` for a class a RegExp rejects.
    pub(crate) fn matches(&self, text: &str) -> Option<bool> {
        let classes = self.0.iter().map(|token| match token {
            Token::Class { members, .. } => Members::new(members).map(Some),
            _ => Some(None),
        }).collect::<Option<Vec<_>>>()?;
        let text: Vec<char> = text.chars().collect();
        // `failed[token][position]`: a state already known not to match the rest.
        let mut failed = vec![false; (self.0.len() + 1) * (text.len() + 1)];
        Some(self.match_from(&classes, &text, 0, 0, &mut failed))
    }

    fn match_from(&self, classes: &[Option<Members>], text: &[char], token: usize, at: usize, failed: &mut [bool]) -> bool {
        let state = token * (text.len() + 1) + at;
        if failed[state] {
            return false;
        }
        let next = text.get(at).copied();
        // As in minimatch, a wildcard never matches a segment's leading `.`.
        let hidden = |index: usize| text[index] == '.' && (index == 0 || text[index - 1] == '/');
        let wild = next.filter(|_| !hidden(at));
        let matched = match self.0.get(token) {
            None => at == text.len(),
            Some(Token::Literal(ch)) => next == Some(*ch) && self.match_from(classes, text, token + 1, at + 1, failed),
            Some(Token::One) => wild.is_some_and(|ch| ch != '/') && self.match_from(classes, text, token + 1, at + 1, failed),
            Some(Token::Class { negated, .. }) => {
                let member = wild.zip(classes[token].as_ref()).is_some_and(|(ch, members)| members.contains(ch) != *negated);
                member && self.match_from(classes, text, token + 1, at + 1, failed)
            }
            Some(Token::Run) => {
                let run = (at..text.len()).position(|index| text[index] == '/' || hidden(index)).unwrap_or(text.len() - at);
                (0..=run).rev().any(|length| self.match_from(classes, text, token + 1, at + length, failed))
            }
            Some(Token::Any) => {
                let run = (at..text.len()).position(|index| text[index] == '\n' || hidden(index)).unwrap_or(text.len() - at);
                (0..=run).rev().any(|length| self.match_from(classes, text, token + 1, at + length, failed))
            }
            Some(Token::Segments) => {
                let segment = text[at..].iter().position(|ch| *ch == '/').filter(|length| *length > 0 && !hidden(at));
                segment.is_some_and(|length| self.match_from(classes, text, token, at + length + 1, failed))
                    || self.match_from(classes, text, token + 1, at, failed)
            }
        };
        if !matched {
            failed[state] = true;
        }
        matched
    }
}

impl Members {
    /// The members of `[members]`, or `None` where a RegExp class rejects them: an empty
    /// class, a nested class, a set operation, or a range running backwards.
    fn new(members: &[char]) -> Option<Self> {
        let operation = members.windows(2).any(|pair| pair[0] == pair[1] && matches!(pair[0], '&' | '-' | '~'));
        if members.is_empty() || members.contains(&'[') || operation {
            return None;
        }
        let mut ranges = Vec::new();
        let mut index = 0;
        while index < members.len() {
            let low = members[index];
            if members.get(index + 1) == Some(&'-') && index + 2 < members.len() {
                let high = members[index + 2];
                if high < low {
                    return None;
                }
                ranges.push((low, high));
                index += 3;
            } else {
                ranges.push((low, low));
                index += 1;
            }
        }
        Some(Self(ranges))
    }

    fn contains(&self, ch: char) -> bool {
        self.0.iter().any(|(low, high)| (*low..=*high).contains(&ch))
    }
}
