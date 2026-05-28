//! Simple lexer for TypeScript parser

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    String, Number, Ident, Keyword,
    Plus, Minus, Star, Slash, Percent, Eq, Lt, Gt, Bang,
    AndAnd, OrOr, LParen, RParen, LBrace, RBrace,
    LBracket, RBracket, Comma, Dot, Semicolon, Colon,
    Question, Arrow, TemplateHead, EOF, Unknown,
}

#[derive(Debug, Clone)]
pub struct Token { pub kind: TokenKind, pub value: String, pub start: usize, pub end: usize }

pub struct Lexer { source: String, pos: usize }

impl Lexer {
    pub fn new(source: &str) -> Self { Self { source: source.to_string(), pos: 0 } }
    pub fn tokenize(&mut self) -> Vec<Token> { let mut ts = Vec::new(); loop { let t = self.next_token(); ts.push(t.clone()); if t.kind == TokenKind::EOF { break; } } ts }
    pub fn next_token(&mut self) -> Token { let s = self.pos; let c = self.next_char(); let k = self.kind(c, s); self.make_token(k, s) }
    fn kind(&mut self, c: char, s: usize) -> TokenKind { match c { '\0' => TokenKind::EOF, '(' => TokenKind::LParen, ')' => TokenKind::RParen, '{' => TokenKind::LBrace, '}' => TokenKind::RBrace, '[' => TokenKind::LBracket, ']' => TokenKind::RBracket, ',' => TokenKind::Comma, ';' => TokenKind::Semicolon, ':' => TokenKind::Colon, '+' => TokenKind::Plus, '*' => TokenKind::Star, '%' => TokenKind::Percent, '<' => TokenKind::Lt, '>' => TokenKind::Gt, '=' => self.eq_tok(), '!' => self.bang_tok(), '&' => self.and_tok(), '|' => self.or_tok(), '-' => TokenKind::Minus, '/' => TokenKind::Slash, '"' | '\'' => { self.str_tok(c); TokenKind::String } '`' => { self.tmpl_tok(); TokenKind::TemplateHead } _ if c.is_digit(10) => { self.digits(); TokenKind::Number } _ if c.is_alphabetic() || c == '_' || c == '$' => { self.alphanum(); TokenKind::Ident } _ => TokenKind::Unknown } }
    fn eq_tok(&mut self) -> TokenKind { if self.peek() == '=' { self.pos += 1; TokenKind::Eq } else if self.peek() == '>' { self.pos += 1; TokenKind::Arrow } else { TokenKind::Eq } }
    fn bang_tok(&mut self) -> TokenKind { if self.peek() == '=' { self.pos += 1; TokenKind::Unknown } else { TokenKind::Bang } }
    fn and_tok(&mut self) -> TokenKind { if self.peek() == '&' { self.pos += 1; TokenKind::AndAnd } else { TokenKind::Unknown } }
    fn or_tok(&mut self) -> TokenKind { if self.peek() == '|' { self.pos += 1; TokenKind::OrOr } else { TokenKind::Unknown } }
    fn sym(c: char) -> TokenKind { match c { '(' => TokenKind::LParen, ')' => TokenKind::RParen, '{' => TokenKind::LBrace, '}' => TokenKind::RBrace, '[' => TokenKind::LBracket, ']' => TokenKind::RBracket, ',' => TokenKind::Comma, ';' => TokenKind::Semicolon, ':' => TokenKind::Colon, '+' => TokenKind::Plus, '-' => TokenKind::Minus, '*' => TokenKind::Star, '/' => TokenKind::Slash, '%' => TokenKind::Percent, '<' => TokenKind::Lt, '>' => TokenKind::Gt, _ => TokenKind::Unknown } }
    fn next_char(&mut self) -> char { loop { if self.pos >= self.source.len() { return '\0'; } let c = self.source[self.pos..].chars().next().unwrap_or('\0'); self.pos += 1; if c == ' ' || c == '\t' || c == '\n' || c == '\r' { continue; } if c == '/' { let p = self.peek(); if p == '/' || p == '*' { self.skip_comment(); continue; } } return c; } }
    fn peek(&self) -> char { self.source[self.pos..].chars().next().unwrap_or('\0') }
    fn skip_comment(&mut self) { if self.pos >= self.source.len() { return; } let c = self.source[self.pos..].chars().next().unwrap_or('\0'); self.pos += 1; if c == '/' { while self.pos < self.source.len() && self.source[self.pos..].chars().next().unwrap_or('\0') != '\n' { self.pos += 1; } } else if c == '*' { loop { self.pos += 1; if self.pos >= self.source.len() { break; } if self.source[self.pos..].chars().next().unwrap_or('\0') == '*' && self.peek() == '/' { self.pos += 2; break; } } } }
    fn str_tok(&mut self, q: char) { while self.pos < self.source.len() { let c = self.source[self.pos..].chars().next().unwrap_or('\0'); self.pos += 1; if c == q { break; } if c == '\\' && self.pos < self.source.len() { self.pos += 1; } } }
    fn tmpl_tok(&mut self) { while self.pos < self.source.len() { let c = self.source[self.pos..].chars().next().unwrap_or('\0'); self.pos += 1; if c == '`' { break; } if c == '\\' && self.pos < self.source.len() { self.pos += 1; } } }
    fn digits(&mut self) { while self.pos < self.source.len() { let c = self.source[self.pos..].chars().next().unwrap_or('\0'); if c.is_digit(10) || c == '.' { self.pos += 1; } else { break; } } }
    fn alphanum(&mut self) { while self.pos < self.source.len() { let c = self.source[self.pos..].chars().next().unwrap_or('\0'); if c.is_alphanumeric() || c == '_' || c == '$' { self.pos += 1; } else { break; } } }
    fn make_token(&self, kind: TokenKind, start: usize) -> Token { Token { kind, value: self.source[start..self.pos].to_string(), start, end: self.pos } }
}
