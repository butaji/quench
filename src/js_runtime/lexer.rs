//! Lexer for JavaScript source code

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    Identifier(String),
    String(String),
    Number(f64),

    // Keywords
    Function, Var, Let, Const,
    If, Else, While, For, Return,
    Break, Continue, True, False,
    Null, Undefined, New, This,
    Try, Catch, Throw, Finally, Do,
    Switch, Case, Default, In, Of,
    Instanceof, Typeof, Void, Delete,

    // Operators
    Plus, Minus, Star, Slash, Percent,
    Amp, Pipe, Caret, Tilde, Bang,
    Question, Colon, Dot, LParen, RParen,
    LBrace, RBrace, LBracket, RBracket,
    Semi, Comma,

    // Compound
    PlusPlus, MinusMinus,
    EqEq, Neq, EqEqEq, NeqEq,
    Lt, Gt, Le, Ge,
    AndAnd, OrOr,
    PlusEq, MinusEq, StarEq, SlashEq, PercentEq,
    AmpEq, PipeEq, CaretEq,
    LtLt, GtGt, GtGtGt, LtLtEq, GtGtEq,
    Eq, DotDotDot,

    // End
    Eof,
}

pub struct Lexer<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Lexer { input, pos: 0 }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn peek_n(&self, n: usize) -> Option<char> {
        self.input[self.pos..].chars().nth(n)
    }

    /// Look at the next token without consuming it
    pub fn peek_token(&self) -> Token {
        let mut temp_lexer = Lexer { input: self.input, pos: self.pos };
        let token = temp_lexer.next_token();
        token
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() && ch != '\n' {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn read_string(&mut self, quote: char) -> String {
        self.advance(); // consume quote
        let mut result = String::new();
        while let Some(ch) = self.peek() {
            if ch == quote {
                self.advance();
                break;
            }
            if ch == '\\' {
                self.advance();
                match self.peek() {
                    Some('n') => { result.push('\n'); self.advance(); }
                    Some('r') => { result.push('\r'); self.advance(); }
                    Some('t') => { result.push('\t'); self.advance(); }
                    Some('\\') => { result.push('\\'); self.advance(); }
                    Some('0') => { result.push('\0'); self.advance(); }
                    Some(c) if c == quote => { result.push(c); self.advance(); }
                    Some(c) => { result.push(c); self.advance(); }
                    None => break,
                }
            } else {
                result.push(ch);
                self.advance();
            }
        }
        result
    }

    fn read_number(&mut self) -> f64 {
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() || ch == '.' || ch == 'e' || ch == 'E' || ch == '-' || ch == '+' {
                self.advance();
            } else {
                break;
            }
        }
        let s = &self.input[start..self.pos];
        s.parse().unwrap_or(f64::NAN)
    }

    fn read_identifier(&mut self) -> String {
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' || ch == '$' {
                self.advance();
            } else {
                break;
            }
        }
        self.input[start..self.pos].to_string()
    }

    pub fn next_token(&mut self) -> Token {
        self.skip_whitespace();

        if self.pos >= self.input.len() {
            return Token::Eof;
        }

        let ch = self.peek().unwrap();

        // Line comment
        if ch == '/' && self.peek_n(1) == Some('/') {
            self.advance();
            self.advance();
            while let Some(c) = self.peek() {
                if c == '\n' { break; }
                self.advance();
            }
            return self.next_token();
        }

        // Block comment
        if ch == '/' && self.peek_n(1) == Some('*') {
            self.advance();
            self.advance();
            loop {
                match (self.peek(), self.peek_n(1)) {
                    (Some('*'), Some('/')) => { self.advance(); self.advance(); break; }
                    (Some(_), _) => { self.advance(); }
                    _ => break,
                }
            }
            return self.next_token();
        }

        match ch {
            '"' | '\'' => Token::String(self.read_string(ch)),
            '`' => Token::String(self.read_string('`')),
            '0'..='9' => Token::Number(self.read_number()),
            'a'..='z' | 'A'..='Z' | '_' | '$' => {
                let ident = self.read_identifier();
                let kw = match ident.as_str() {
                    "function" => Token::Function,
                    "var" => Token::Var,
                    "let" => Token::Let,
                    "const" => Token::Const,
                    "if" => Token::If,
                    "else" => Token::Else,
                    "while" => Token::While,
                    "for" => Token::For,
                    "return" => Token::Return,
                    "break" => Token::Break,
                    "continue" => Token::Continue,
                    "true" => Token::True,
                    "false" => Token::False,
                    "null" => Token::Null,
                    "undefined" => Token::Undefined,
                    "new" => Token::New,
                    "this" => Token::This,
                    "try" => Token::Try,
                    "catch" => Token::Catch,
                    "throw" => Token::Throw,
                    "finally" => Token::Finally,
                    "do" => Token::Do,
                    "switch" => Token::Switch,
                    "case" => Token::Case,
                    "default" => Token::Default,
                    "in" => Token::In,
                    "of" => Token::Of,
                    "instanceof" => Token::Instanceof,
                    "typeof" => Token::Typeof,
                    "void" => Token::Void,
                    "delete" => Token::Delete,
                    _ => Token::Identifier(ident),
                };
                kw
            }
            '+' => { self.advance(); if self.peek() == Some('+') { self.advance(); Token::PlusPlus } else if self.peek() == Some('=') { self.advance(); Token::PlusEq } else { Token::Plus } }
            '-' => { self.advance(); if self.peek() == Some('-') { self.advance(); Token::MinusMinus } else if self.peek() == Some('=') { self.advance(); Token::MinusEq } else { Token::Minus } }
            '*' => { self.advance(); if self.peek() == Some('=') { self.advance(); Token::StarEq } else { Token::Star } }
            '/' => { self.advance(); if self.peek() == Some('=') { self.advance(); Token::SlashEq } else { Token::Slash } }
            '%' => { self.advance(); if self.peek() == Some('=') { self.advance(); Token::PercentEq } else { Token::Percent } }
            '&' => { self.advance(); if self.peek() == Some('&') { self.advance(); Token::AndAnd } else if self.peek() == Some('=') { self.advance(); Token::AmpEq } else { Token::Amp } }
            '|' => { self.advance(); if self.peek() == Some('|') { self.advance(); Token::OrOr } else if self.peek() == Some('=') { self.advance(); Token::PipeEq } else { Token::Pipe } }
            '^' => { self.advance(); if self.peek() == Some('=') { self.advance(); Token::CaretEq } else { Token::Caret } }
            '~' => { self.advance(); Token::Tilde }
            '!' => { self.advance(); if self.peek() == Some('=') { self.advance(); if self.peek() == Some('=') { self.advance(); Token::NeqEq } else { Token::Neq } } else { Token::Bang } }
            '=' => { self.advance(); if self.peek() == Some('=') { self.advance(); if self.peek() == Some('=') { self.advance(); Token::EqEqEq } else { Token::EqEq } } else { Token::Eq } }
            '<' => { self.advance(); if self.peek() == Some('<') { self.advance(); if self.peek() == Some('=') { self.advance(); Token::LtLtEq } else { Token::LtLt } } else if self.peek() == Some('=') { self.advance(); Token::Le } else { Token::Lt } }
            '>' => { self.advance(); if self.peek() == Some('>') { self.advance(); if self.peek() == Some('>') { self.advance(); Token::GtGtGt } else if self.peek() == Some('=') { self.advance(); Token::GtGtEq } else { Token::GtGt } } else if self.peek() == Some('=') { self.advance(); Token::Ge } else { Token::Gt } }
            '?' => { self.advance(); Token::Question }
            ':' => { self.advance(); Token::Colon }
            '.' => { self.advance(); if self.peek() == Some('.') && self.peek_n(1) == Some('.') { self.advance(); self.advance(); Token::DotDotDot } else { Token::Dot } }
            ';' => { self.advance(); Token::Semi }
            ',' => { self.advance(); Token::Comma }
            '(' => { self.advance(); Token::LParen }
            ')' => { self.advance(); Token::RParen }
            '{' => { self.advance(); Token::LBrace }
            '}' => { self.advance(); Token::RBrace }
            '[' => { self.advance(); Token::LBracket }
            ']' => { self.advance(); Token::RBracket }
            _ => { self.advance(); self.next_token() }
        }
    }
}

impl<'a> Iterator for Lexer<'a> {
    type Item = Token;
    fn next(&mut self) -> Option<Self::Item> {
        let t = self.next_token();
        if t == Token::Eof { None } else { Some(t) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keywords() {
        let mut lex = Lexer::new("function var if else while for return");
        assert_eq!(lex.next_token(), Token::Function);
        assert_eq!(lex.next_token(), Token::Var);
        assert_eq!(lex.next_token(), Token::If);
        assert_eq!(lex.next_token(), Token::Else);
        assert_eq!(lex.next_token(), Token::While);
        assert_eq!(lex.next_token(), Token::For);
        assert_eq!(lex.next_token(), Token::Return);
    }

    #[test]
    fn test_operators() {
        let mut lex = Lexer::new("=== !== <= >= && || ++ --");
        assert_eq!(lex.next_token(), Token::EqEqEq);
        assert_eq!(lex.next_token(), Token::NeqEq);
        assert_eq!(lex.next_token(), Token::Le);
        assert_eq!(lex.next_token(), Token::Ge);
        assert_eq!(lex.next_token(), Token::AndAnd);
        assert_eq!(lex.next_token(), Token::OrOr);
        assert_eq!(lex.next_token(), Token::PlusPlus);
        assert_eq!(lex.next_token(), Token::MinusMinus);
    }
}
