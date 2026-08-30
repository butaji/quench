//! Unfold folded legacy `(try (do …) (catch …))` so wast 247 can parse it.

use std::borrow::Cow;

pub(crate) fn unfold_if_legacy<'a>(filename: &str, source: &'a str) -> Cow<'a, str> {
    let path = filename.replace('\\', "/");
    if !path.contains("legacy/") {
        return Cow::Borrowed(source);
    }
    match unfold_source(source) {
        Some(out) => Cow::Owned(out),
        None => Cow::Borrowed(source),
    }
}

fn unfold_source(source: &str) -> Option<String> {
    let items = parse_file(source)?;
    Some(
        items
            .into_iter()
            .map(|s| print_sexp(&rewrite(s)))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

#[derive(Clone, Debug)]
enum Sexp {
    Atom(String),
    List(Vec<Sexp>),
}

fn rewrite(s: Sexp) -> Sexp {
    match s {
        Sexp::Atom(a) => Sexp::Atom(a),
        Sexp::List(items) => Sexp::List(rewrite_seq(items)),
    }
}

fn rewrite_seq(items: Vec<Sexp>) -> Vec<Sexp> {
    let mut out = Vec::new();
    for item in items {
        match item {
            Sexp::List(inner) if matches!(inner.first(), Some(Sexp::Atom(a)) if a == "try") => {
                let inner = rewrite_seq(inner);
                out.extend(unfold_try(inner));
            }
            Sexp::List(inner) => out.push(Sexp::List(rewrite_seq(inner))),
            atom => out.push(atom),
        }
    }
    out
}

fn unfold_try(items: Vec<Sexp>) -> Vec<Sexp> {
    let mut rest = items.into_iter();
    rest.next();
    let mut prefix = Vec::new();
    let mut do_body = Vec::new();
    let mut catches = Vec::new();
    let mut catch_all = None;
    let mut delegate = None;
    for item in rest {
        match &item {
            Sexp::List(inner) if matches!(inner.first(), Some(Sexp::Atom(a)) if a == "do") => {
                do_body = rewrite_seq(inner[1..].to_vec());
            }
            Sexp::List(inner) if matches!(inner.first(), Some(Sexp::Atom(a)) if a == "catch") => {
                catches.push(rewrite_seq(inner[1..].to_vec()));
            }
            Sexp::List(inner) if matches!(inner.first(), Some(Sexp::Atom(a)) if a == "catch_all") =>
            {
                catch_all = Some(rewrite_seq(inner[1..].to_vec()));
            }
            Sexp::List(inner) if matches!(inner.first(), Some(Sexp::Atom(a)) if a == "delegate") => {
                delegate = Some(rewrite_seq(inner[1..].to_vec()));
            }
            _ => prefix.push(item),
        }
    }

    let mut out = vec![Sexp::Atom("try".into())];
    out.extend(prefix);
    out.extend(do_body);
    if let Some(del) = delegate {
        out.push(Sexp::Atom("delegate".into()));
        out.extend(del);
        return out;
    }
    for catch in catches {
        out.push(Sexp::Atom("catch".into()));
        out.extend(catch);
    }
    if let Some(body) = catch_all {
        out.push(Sexp::Atom("catch_all".into()));
        out.extend(body);
    }
    out.push(Sexp::Atom("end".into()));
    out
}

fn print_sexp(s: &Sexp) -> String {
    match s {
        Sexp::Atom(a) => a.clone(),
        Sexp::List(items) => {
            let inner = items.iter().map(print_sexp).collect::<Vec<_>>().join(" ");
            format!("({inner})")
        }
    }
}

fn parse_file(src: &str) -> Option<Vec<Sexp>> {
    let mut p = Parser {
        src,
        i: 0,
        len: src.len(),
    };
    let mut items = Vec::new();
    loop {
        p.skip_ws_and_comments();
        if p.i >= p.len {
            break;
        }
        items.push(p.parse_sexp()?);
    }
    Some(items)
}

struct Parser<'a> {
    src: &'a str,
    i: usize,
    len: usize,
}

impl<'a> Parser<'a> {
    fn skip_ws_and_comments(&mut self) {
        loop {
            let bytes = self.src.as_bytes();
            while self.i < self.len && bytes[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            if self.i + 1 < self.len && bytes[self.i] == b';' && bytes[self.i + 1] == b';' {
                while self.i < self.len && bytes[self.i] != b'\n' {
                    self.i += 1;
                }
                continue;
            }
            if self.i + 1 < self.len && bytes[self.i] == b'(' && bytes[self.i + 1] == b';' {
                self.i += 2;
                let mut depth = 1;
                while self.i + 1 < self.len && depth > 0 {
                    if bytes[self.i] == b'(' && bytes[self.i + 1] == b';' {
                        depth += 1;
                        self.i += 2;
                    } else if bytes[self.i] == b';' && bytes[self.i + 1] == b')' {
                        depth -= 1;
                        self.i += 2;
                    } else {
                        self.i += 1;
                    }
                }
                continue;
            }
            break;
        }
    }

    fn parse_sexp(&mut self) -> Option<Sexp> {
        self.skip_ws_and_comments();
        let bytes = self.src.as_bytes();
        if self.i >= self.len {
            return None;
        }
        if bytes[self.i] == b'(' {
            self.i += 1;
            let mut items = Vec::new();
            loop {
                self.skip_ws_and_comments();
                if self.i >= self.len {
                    return None;
                }
                if bytes.get(self.i) == Some(&b')') {
                    self.i += 1;
                    break;
                }
                items.push(self.parse_sexp()?);
            }
            return Some(Sexp::List(items));
        }
        if bytes[self.i] == b'"' {
            let start = self.i;
            self.i += 1;
            while self.i < self.len {
                match bytes[self.i] {
                    b'\\' => self.i += 2,
                    b'"' => {
                        self.i += 1;
                        break;
                    }
                    _ => self.i += 1,
                }
            }
            return Some(Sexp::Atom(self.src[start..self.i].to_string()));
        }
        let start = self.i;
        while self.i < self.len {
            let c = bytes[self.i];
            if c.is_ascii_whitespace() || c == b'(' || c == b')' {
                break;
            }
            self.i += 1;
        }
        if start == self.i {
            return None;
        }
        Some(Sexp::Atom(self.src[start..self.i].to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::unfold_source;

    #[test]
    fn unfolds_folded_try_catch() {
        let src = r#"(func (export "empty-catch") (try (do) (catch $e0)))"#;
        let out = unfold_source(src).expect("unfold");
        assert!(out.contains("catch $e0"), "{out}");
        assert!(out.contains(" end)"), "{out}");
        assert!(!out.contains("(do)"), "{out}");
        assert!(!out.contains("(try"), "{out}");
    }
}
