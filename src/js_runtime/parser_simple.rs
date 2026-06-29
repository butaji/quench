//! Simple recursive descent parser for JavaScript

use crate::js_runtime::lexer::{Lexer, Token};
use crate::js_runtime::ast::*;
use crate::js_runtime::value::JsError;

/// Parse JavaScript source into AST
pub fn parse(source: &str) -> Result<Program, JsError> {
    let mut lexer = Lexer::new(source);
    let mut parser = Parser::new(&mut lexer);
    parser.parse_program()
}

struct Parser<'a> {
    lexer: &'a mut Lexer<'a>,
    current: Token,
}

impl<'a> Parser<'a> {
    fn new(lexer: &'a mut Lexer<'a>) -> Self {
        let current = lexer.next_token();
        Parser { lexer, current }
    }

    fn advance(&mut self) -> Token {
        let prev = self.current.clone();
        self.current = self.lexer.next_token();
        prev
    }

    fn expect(&mut self, expected: &Token) -> Result<Token, JsError> {
        let token = self.advance();
        if std::mem::discriminant(&token) == std::mem::discriminant(expected) {
            Ok(token)
        } else {
            Err(JsError(format!("Expected {:?}, got {:?}", expected, token)))
        }
    }

    fn check(&self, token: &Token) -> bool {
        std::mem::discriminant(&self.current) == std::mem::discriminant(token)
    }

    fn parse_program(&mut self) -> Result<Program, JsError> {
        let mut statements = Vec::new();
        while !self.check(&Token::Eof) {
            statements.push(self.parse_statement()?);
        }
        Ok(Program::Script(statements))
    }

    fn parse_statement(&mut self) -> Result<Statement, JsError> {
        match &self.current {
            Token::Var | Token::Let | Token::Const => self.parse_var_declaration(),
            Token::Function => self.parse_function_declaration(),
            Token::If => self.parse_if_statement(),
            Token::While => self.parse_while_statement(),
            Token::For => self.parse_for_statement(),
            Token::Return => self.parse_return_statement(),
            Token::Break => {
                self.advance();
                Ok(Statement::Break(None))
            }
            Token::Continue => {
                self.advance();
                Ok(Statement::Continue(None))
            }
            Token::Try => self.parse_try_statement(),
            Token::Throw => self.parse_throw_statement(),
            // Note: LBrace in expression context is an object literal, not a block
            // Block statements are handled separately after control flow keywords
            Token::LBrace => self.parse_expression_statement(),
            Token::Semi => {
                self.advance();
                Ok(Statement::Empty)
            }
            Token::Eof => Ok(Statement::Empty),
            _ => self.parse_expression_statement(),
        }
    }

    fn parse_var_declaration(&mut self) -> Result<Statement, JsError> {
        let kind = match self.advance() {
            Token::Var => VarKind::Var,
            Token::Let => VarKind::Let,
            Token::Const => VarKind::Const,
            _ => return Err(JsError("Expected var/let/const".to_string())),
        };

        let name = match self.advance() {
            Token::Identifier(s) => s,
            _ => return Err(JsError("Expected identifier".to_string())),
        };

        let init = if self.check(&Token::Eq) {
            self.advance();
            Some(self.parse_expression()?)
        } else {
            None
        };

        self.consume_semi()?;
        Ok(Statement::VarDeclaration { kind, name, init })
    }

    fn parse_function_declaration(&mut self) -> Result<Statement, JsError> {
        self.advance(); // consume 'function'
        
        let name = match self.advance() {
            Token::Identifier(s) => s,
            _ => return Err(JsError("Expected function name".to_string())),
        };

        self.expect(&Token::LParen)?;
        let params = self.parse_params()?;
        self.expect(&Token::RParen)?;
        
        let body = match self.parse_block() {
            Ok(Statement::Block(stmts)) => stmts,
            Ok(s) => vec![s],
            Err(e) => return Err(e),
        };

        Ok(Statement::FunctionDeclaration { name, params, body })
    }

    fn parse_params(&mut self) -> Result<Vec<String>, JsError> {
        let mut params = Vec::new();
        if !self.check(&Token::RParen) {
            loop {
                let param = match self.advance() {
                    Token::Identifier(s) => s,
                    _ => return Err(JsError("Expected parameter".to_string())),
                };
                params.push(param);
                if self.check(&Token::RParen) {
                    break;
                }
                self.expect(&Token::Comma)?;
            }
        }
        Ok(params)
    }

    fn parse_if_statement(&mut self) -> Result<Statement, JsError> {
        self.advance(); // consume 'if'
        self.expect(&Token::LParen)?;
        let condition = self.parse_expression()?;
        self.expect(&Token::RParen)?;
        
        let consequent = Box::new(self.parse_statement()?);
        let alternate = if self.check(&Token::Else) {
            self.advance();
            Some(Box::new(self.parse_statement()?))
        } else {
            None
        };

        Ok(Statement::If { condition: Box::new(condition), consequent, alternate })
    }

    fn parse_while_statement(&mut self) -> Result<Statement, JsError> {
        self.advance(); // consume 'while'
        self.expect(&Token::LParen)?;
        let condition = self.parse_expression()?;
        self.expect(&Token::RParen)?;
        let body = Box::new(self.parse_statement()?);

        Ok(Statement::While { condition: Box::new(condition), body })
    }

    fn parse_for_statement(&mut self) -> Result<Statement, JsError> {
        self.advance(); // consume 'for'
        self.expect(&Token::LParen)?;

        let init = if self.check(&Token::Var) || self.check(&Token::Let) || self.check(&Token::Const) {
            let kind = match self.current {
                Token::Var => VarKind::Var,
                Token::Let => VarKind::Let,
                Token::Const => VarKind::Const,
                _ => VarKind::Var,
            };
            self.advance();
            let name = match self.current.clone() {
                Token::Identifier(s) => { self.advance(); s }
                _ => return Err(JsError("Expected identifier".to_string())),
            };
            let init = if self.check(&Token::Eq) {
                self.advance();
                Some(self.parse_expression()?)
            } else {
                None
            };
            self.consume_semi()?;
            Some(ForInit::VarDeclaration { kind, name, init })
        } else if !self.check(&Token::Semi) {
            let expr = self.parse_expression()?;
            self.consume_semi()?;
            Some(ForInit::Expression(Box::new(expr)))
        } else {
            self.consume_semi()?;
            None
        };

        let condition = if !self.check(&Token::Semi) {
            Some(Box::new(self.parse_expression()?))
        } else {
            None
        };
        self.consume_semi()?;

        let update = if !self.check(&Token::RParen) {
            Some(Box::new(self.parse_expression()?))
        } else {
            None
        };
        self.expect(&Token::RParen)?;
        let body = Box::new(self.parse_statement()?);

        Ok(Statement::For { init, condition, update, body })
    }

    fn parse_return_statement(&mut self) -> Result<Statement, JsError> {
        self.advance(); // consume 'return'
        if self.check(&Token::Semi) || self.check(&Token::RBrace) {
            self.consume_semi()?;
            Ok(Statement::Return(None))
        } else {
            let expr = self.parse_expression()?;
            self.consume_semi()?;
            Ok(Statement::Return(Some(Box::new(expr))))
        }
    }

    fn parse_try_statement(&mut self) -> Result<Statement, JsError> {
        self.advance(); // consume 'try'
        let body = Box::new(self.parse_statement()?);
        
        let param;
        let handler;
        
        if self.check(&Token::Catch) {
            self.advance();
            if self.check(&Token::LParen) {
                self.advance();
                param = match self.advance() {
                    Token::Identifier(s) => Some(s),
                    _ => None,
                };
                self.expect(&Token::RParen)?;
            } else {
                param = None;
            }
            handler = Box::new(self.parse_statement()?);
        } else {
            param = None;
            handler = Box::new(Statement::Empty);
        }

        Ok(Statement::TryCatch { body, param, handler })
    }

    fn parse_throw_statement(&mut self) -> Result<Statement, JsError> {
        self.advance(); // consume 'throw'
        let expr = self.parse_expression()?;
        self.consume_semi()?;
        Ok(Statement::Throw(Box::new(expr)))
    }

    fn parse_block(&mut self) -> Result<Statement, JsError> {
        self.expect(&Token::LBrace)?;
        let mut statements = Vec::new();
        while !self.check(&Token::RBrace) && !self.check(&Token::Eof) {
            statements.push(self.parse_statement()?);
        }
        self.expect(&Token::RBrace)?;
        Ok(Statement::Block(statements))
    }

    fn parse_expression_statement(&mut self) -> Result<Statement, JsError> {
        let expr = self.parse_expression()?;
        self.consume_semi()?;
        Ok(Statement::Expression(Box::new(expr)))
    }

    fn consume_semi(&mut self) -> Result<(), JsError> {
        if self.check(&Token::Semi) {
            self.advance();
        }
        Ok(())
    }

    fn parse_expression(&mut self) -> Result<Expression, JsError> {
        self.parse_assignment()
    }
    
    /// Parse a brace that could be either an object literal or block
    /// Returns (is_object, result)
    fn parse_brace_expr(&mut self) -> Result<Expression, JsError> {
        // We know current is LBrace
        self.advance(); // consume LBrace
        
        // Empty object {}
        if self.check(&Token::RBrace) {
            self.advance();
            return Ok(Expression::Object(vec![]));
        }
        
        // Look ahead past whitespace/newlines to find first meaningful token
        let first_meaningful = self.lexer.peek_token();
        
        // If first meaningful token after { is }, this is an empty object
        if matches!(first_meaningful, Token::RBrace) {
            self.advance(); // consume }
            return Ok(Expression::Object(vec![]));
        }
        
        // Look at first token to determine if this is an object literal
        let is_object = match &self.current {
            Token::Identifier(_) => {
                // Check if next token is : or (
                let next = self.lexer.peek_token();
                matches!(next, Token::Colon | Token::LParen)
            }
            Token::String(_) | Token::Number(_) => true,
            Token::LBracket => true, // [ computed property
            Token::DotDotDot => true, // spread
            _ => false, // It's a block
        };
        
        if is_object {
            // Parse as object literal
            self.parse_object_literal()
        } else {
            // Parse as block expression (value of last statement)
            let stmts = self.parse_block_body()?;
            Ok(Expression::BlockExpr(stmts))
        }
    }
    
    fn parse_block_body(&mut self) -> Result<Vec<Statement>, JsError> {
        let mut stmts = Vec::new();
        while !self.check(&Token::RBrace) && !self.check(&Token::Eof) {
            stmts.push(self.parse_statement()?);
        }
        self.expect(&Token::RBrace)?;
        Ok(stmts)
    }

    fn parse_assignment(&mut self) -> Result<Expression, JsError> {
        let left = self.parse_conditional()?;
        
        if matches!(self.current, Token::Eq) {
            self.advance();
            let right = self.parse_assignment()?;
            return Ok(Expression::Assignment {
                left: Box::new(left),
                right: Box::new(right),
            });
        }
        
        // Compound assignments
        let comp_op = match self.current {
            Token::PlusEq => Some(CompoundOp::Add),
            Token::MinusEq => Some(CompoundOp::Sub),
            Token::StarEq => Some(CompoundOp::Mul),
            Token::SlashEq => Some(CompoundOp::Div),
            Token::PercentEq => Some(CompoundOp::Mod),
            _ => None,
        };
        
        if let Some(op) = comp_op {
            self.advance();
            let right = self.parse_assignment()?;
            return Ok(Expression::CompoundAssignment {
                op,
                left: Box::new(left),
                right: Box::new(right),
            });
        }

        Ok(left)
    }

    fn parse_conditional(&mut self) -> Result<Expression, JsError> {
        let condition = self.parse_or()?;
        
        if self.check(&Token::Question) {
            self.advance();
            let consequent = self.parse_expression()?;
            self.expect(&Token::Colon)?;
            let alternate = self.parse_conditional()?;
            return Ok(Expression::Conditional {
                condition: Box::new(condition),
                consequent: Box::new(consequent),
                alternate: Box::new(alternate),
            });
        }

        Ok(condition)
    }

    fn parse_or(&mut self) -> Result<Expression, JsError> {
        let mut left = self.parse_and()?;
        
        while self.check(&Token::OrOr) {
            self.advance();
            let right = self.parse_and()?;
            left = Expression::Binary {
                op: BinaryOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expression, JsError> {
        let mut left = self.parse_bitwise_or()?;
        
        while self.check(&Token::AndAnd) {
            self.advance();
            let right = self.parse_bitwise_or()?;
            left = Expression::Binary {
                op: BinaryOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_bitwise_or(&mut self) -> Result<Expression, JsError> {
        let mut left = self.parse_bitwise_xor()?;
        
        while self.check(&Token::Pipe) {
            self.advance();
            let right = self.parse_bitwise_xor()?;
            left = Expression::Binary {
                op: BinaryOp::BitOr,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_bitwise_xor(&mut self) -> Result<Expression, JsError> {
        let mut left = self.parse_bitwise_and()?;
        
        while self.check(&Token::Caret) {
            self.advance();
            let right = self.parse_bitwise_and()?;
            left = Expression::Binary {
                op: BinaryOp::BitXor,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_bitwise_and(&mut self) -> Result<Expression, JsError> {
        let mut left = self.parse_equality()?;
        
        while self.check(&Token::Amp) {
            self.advance();
            let right = self.parse_equality()?;
            left = Expression::Binary {
                op: BinaryOp::BitAnd,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_equality(&mut self) -> Result<Expression, JsError> {
        let mut left = self.parse_comparison()?;
        
        loop {
            let op = match self.current {
                Token::EqEq => Some(BinaryOp::Eq),
                Token::Neq => Some(BinaryOp::Neq),
                Token::EqEqEq => Some(BinaryOp::StrictEq),
                Token::NeqEq => Some(BinaryOp::StrictNeq),
                _ => None,
            };
            
            if let Some(op) = op {
                self.advance();
                let right = self.parse_comparison()?;
                left = Expression::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }

        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expression, JsError> {
        let mut left = self.parse_shift()?;
        
        loop {
            let op = match self.current {
                Token::Lt => Some(BinaryOp::Lt),
                Token::Gt => Some(BinaryOp::Gt),
                Token::Le => Some(BinaryOp::Le),
                Token::Ge => Some(BinaryOp::Ge),
                _ => None,
            };
            
            if let Some(op) = op {
                self.advance();
                let right = self.parse_shift()?;
                left = Expression::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }

        Ok(left)
    }

    fn parse_shift(&mut self) -> Result<Expression, JsError> {
        let mut left = self.parse_additive()?;
        
        loop {
            let op = match self.current {
                Token::LtLt => Some(BinaryOp::Shl),
                Token::GtGt => Some(BinaryOp::Shr),
                Token::GtGtGt => Some(BinaryOp::Ushr),
                _ => None,
            };
            
            if let Some(op) = op {
                self.advance();
                let right = self.parse_additive()?;
                left = Expression::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }

        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expression, JsError> {
        let mut left = self.parse_multiplicative()?;
        
        loop {
            let op = match self.current {
                Token::Plus => Some(BinaryOp::Add),
                Token::Minus => Some(BinaryOp::Sub),
                _ => None,
            };
            
            if let Some(op) = op {
                self.advance();
                let right = self.parse_multiplicative()?;
                left = Expression::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }

        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expression, JsError> {
        let mut left = self.parse_unary()?;
        
        loop {
            let op = match self.current {
                Token::Star => Some(BinaryOp::Mul),
                Token::Slash => Some(BinaryOp::Div),
                Token::Percent => Some(BinaryOp::Mod),
                _ => None,
            };
            
            if let Some(op) = op {
                self.advance();
                let right = self.parse_unary()?;
                left = Expression::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }

        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expression, JsError> {
        match self.current {
            Token::Bang => {
                self.advance();
                let arg = self.parse_unary()?;
                Ok(Expression::Unary {
                    op: UnaryOp::Not,
                    argument: Box::new(arg),
                })
            }
            Token::Minus => {
                self.advance();
                let arg = self.parse_unary()?;
                Ok(Expression::Unary {
                    op: UnaryOp::Neg,
                    argument: Box::new(arg),
                })
            }
            Token::Plus => {
                self.advance();
                self.parse_unary()
            }
            Token::Tilde => {
                self.advance();
                let arg = self.parse_unary()?;
                Ok(Expression::Unary {
                    op: UnaryOp::BitNot,
                    argument: Box::new(arg),
                })
            }
            Token::Typeof => {
                self.advance();
                let arg = self.parse_unary()?;
                Ok(Expression::Unary {
                    op: UnaryOp::Typeof,
                    argument: Box::new(arg),
                })
            }
            Token::Void => {
                self.advance();
                let arg = self.parse_unary()?;
                Ok(Expression::Unary {
                    op: UnaryOp::Void,
                    argument: Box::new(arg),
                })
            }
            Token::PlusPlus => {
                self.advance();
                let arg = self.parse_unary()?;
                Ok(Expression::Update {
                    op: UpdateOp::Increment,
                    argument: Box::new(arg),
                    prefix: true,
                })
            }
            Token::MinusMinus => {
                self.advance();
                let arg = self.parse_unary()?;
                Ok(Expression::Update {
                    op: UpdateOp::Decrement,
                    argument: Box::new(arg),
                    prefix: true,
                })
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expression, JsError> {
        let mut expr = self.parse_call()?; // Start from call to allow member access first
        
        if matches!(self.current, Token::PlusPlus | Token::MinusMinus) {
            let op = match self.advance() {
                Token::PlusPlus => UpdateOp::Increment,
                Token::MinusMinus => UpdateOp::Decrement,
                _ => return Err(JsError("Invalid postfix op".to_string())),
            };
            expr = Expression::Update {
                op,
                argument: Box::new(expr),
                prefix: false,
            };
        }
        
        Ok(expr)
    }

    fn parse_call(&mut self) -> Result<Expression, JsError> {
        let mut expr = self.parse_primary()?;
        
        loop {
            if self.check(&Token::LParen) {
                self.advance();
                let mut args = Vec::new();
                if !self.check(&Token::RParen) {
                    loop {
                        args.push(self.parse_expression()?);
                        if self.check(&Token::RParen) {
                            break;
                        }
                        self.expect(&Token::Comma)?;
                    }
                }
                self.expect(&Token::RParen)?;
                expr = Expression::Call {
                    callee: Box::new(expr),
                    arguments: args,
                };
            } else if self.check(&Token::Dot) {
                self.advance();
                let property = match self.advance() {
                    Token::Identifier(s) => PropertyKey::Ident(s),
                    _ => return Err(JsError("Expected property name".to_string())),
                };
                expr = Expression::Member {
                    object: Box::new(expr),
                    property,
                    computed: false,
                };
            } else if self.check(&Token::LBracket) {
                self.advance();
                let prop = self.parse_expression()?;
                self.expect(&Token::RBracket)?;
                expr = Expression::Member {
                    object: Box::new(expr),
                    property: PropertyKey::Computed(Box::new(prop)),
                    computed: true,
                };
            } else {
                break;
            }
        }
        
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expression, JsError> {
        let token = self.current.clone();
        match &token {
            Token::Number(n) => { self.advance(); Ok(Expression::Number(*n)) }
            Token::String(s) => { self.advance(); Ok(Expression::String(s.clone())) }
            Token::True => { self.advance(); Ok(Expression::Boolean(true)) }
            Token::False => { self.advance(); Ok(Expression::Boolean(false)) }
            Token::Null => { self.advance(); Ok(Expression::Null) }
            Token::Undefined => { self.advance(); Ok(Expression::Undefined) }
            Token::Identifier(s) => { self.advance(); Ok(Expression::Identifier(s.clone())) }
            Token::LParen => {
                self.advance();
                let expr = self.parse_expression()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            Token::LBracket => {
                self.advance();
                self.parse_array_literal()
            }
            Token::LBrace => self.parse_brace_expr(),
            Token::Function => self.parse_function_expression(),
            Token::New => {
                let constructor = self.parse_call()?;
                Ok(Expression::New {
                    constructor: Box::new(constructor),
                    arguments: vec![],
                })
            }
            _ => Err(JsError(format!("Unexpected token: {:?}", self.current))),
        }
    }

    fn parse_array_literal(&mut self) -> Result<Expression, JsError> {
        let mut elements = Vec::new();
        self.advance(); // consume the opening bracket
        if !self.check(&Token::RBracket) {
            loop {
                elements.push(self.parse_expression()?);
                if self.check(&Token::RBracket) {
                    break;
                }
                self.expect(&Token::Comma)?;
            }
        }
        self.expect(&Token::RBracket)?;
        Ok(Expression::Array(elements))
    }

    fn parse_object_literal(&mut self) -> Result<Expression, JsError> {
        // Note: LBrace has already been consumed by parse_brace_expr
        // We start directly with the first property (or RBrace for empty object)
        let mut props = Vec::new();
        loop {
            // Check for closing brace FIRST (handles empty object and end of properties)
            if self.check(&Token::RBrace) {
                self.advance();
                break;
            }
            let key_name = match self.current.clone() {
                Token::Identifier(s) => {
                    self.advance();
                    s
                }
                Token::String(s) => {
                    self.advance();
                    props.push((PropertyKey::String(s.clone()), Expression::String(s)));
                    if self.check(&Token::RBrace) { break; }
                    self.expect(&Token::Comma)?;
                    continue;
                }
                Token::Number(n) => {
                    self.advance();
                    let n_val = n;
                    props.push((PropertyKey::Number(n_val), Expression::Number(n_val)));
                    if self.check(&Token::RBrace) { break; }
                    self.expect(&Token::Comma)?;
                    continue;
                }
                _ => return Err(JsError("Expected property key".to_string())),
            };
            
            let key = PropertyKey::Ident(key_name.clone());
            
            let value = if self.check(&Token::Colon) {
                self.advance();
                self.parse_expression()?
            } else if self.check(&Token::LParen) {
                // Shorthand method: name() { ... }
                self.advance(); // consume LParen
                let params = self.parse_params()?;
                self.expect(&Token::RParen)?;
                let body = match self.parse_block() {
                    Ok(Statement::Block(stmts)) => stmts,
                    Ok(s) => vec![s],
                    Err(e) => return Err(e),
                };
                Expression::FunctionExpression { name: Some(key_name), params, body }
            } else {
                // Shorthand property: { name }
                Expression::Identifier(key_name)
            };
            
            props.push((key, value));
            
            // Check for comma and continue
            if self.check(&Token::Comma) {
                self.advance();
                // Check for trailing comma (RBrace immediately after comma)
                if self.check(&Token::RBrace) {
                    self.advance();
                    break;
                }
            }
        }
        Ok(Expression::Object(props))
    }

    fn parse_function_expression(&mut self) -> Result<Expression, JsError> {
        // Consume 'function' keyword if present
        if matches!(self.current, Token::Function) {
            self.advance();
        }
        
        let name = if let Token::Identifier(_) = self.current {
            match self.advance() {
                Token::Identifier(s) => Some(s),
                _ => None,
            }
        } else {
            None
        };

        self.expect(&Token::LParen)?;
        let params = self.parse_params()?;
        self.expect(&Token::RParen)?;
        
        let body = match self.parse_block() {
            Ok(Statement::Block(stmts)) => stmts,
            Ok(s) => vec![s],
            Err(e) => return Err(e),
        };

        Ok(Expression::FunctionExpression { name, params, body })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_number() {
        let result = parse("42");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_string() {
        let result = parse("\"hello\"");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_var() {
        let result = parse("var x = 42;");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_function() {
        let result = parse("function add(a, b) { return a + b; }");
        assert!(result.is_ok());
    }
}

#[cfg(test)]
mod shorthand_tests {
    use super::*;

    #[test]
    fn test_shorthand_method() {
        let result = parse("{ foo() { return 1; } }");
        assert!(result.is_ok(), "Failed to parse shorthand method: {:?}", result);
    }
    
    #[test]
    fn test_shorthand_methods_in_object() {
        // Test simple object
        eprintln!("Test 1: parsing object literal");
        let result = parse("{ a: 1 }");
        eprintln!("Result 1: {:?}", result);
        assert!(result.is_ok(), "Single property failed: {:?}", result);
        
        // Test with comma
        eprintln!("Test 2: parsing object with comma");
        let result2 = parse("{ a: 1, b: 2 }");
        eprintln!("Result 2: {:?}", result2);
        assert!(result2.is_ok(), "Two properties with comma failed: {:?}", result2);
    }
    
    #[test]
    fn test_shorthand_method_no_comma() {
        // Two methods without comma between them
        let result = parse("{ foo() { return 1; } bar() { return 2; } }");
        assert!(result.is_ok(), "Two methods no comma failed: {:?}", result);
    }
}

#[cfg(test)]
mod debug_tests {
    #[test]
    fn debug_parse() {
        let source = "{ a: 1 }";
        eprintln!("Parsing: {}", source);
        let mut lexer = crate::js_runtime::lexer::Lexer::new(source);
        
        // Create parser BEFORE consuming all tokens
        let mut parser = crate::js_runtime::parser_simple::Parser::new(&mut lexer);
        eprintln!("  Parser initial current token: {:?}", parser.current);
        let result = parser.parse_object_literal();
        eprintln!("  After parse_object_literal, current: {:?}", parser.current);
        eprintln!("  Result: {:?}", result);
        assert!(result.is_ok(), "Failed: {:?}", result);
    }
}

#[cfg(test)]
mod object_literal_tests {
    use super::*;

    #[test]
    fn test_parse_object_with_numbers() {
        let source = r#"const x = { a: 1, b: 2 };"#;
        let result = parse(source);
        println!("Result: {:?}", result);
        assert!(result.is_ok(), "Failed to parse: {:?}", result);
    }
    
    #[test]
    fn test_parse_object_literal_from_runtime() {
        let source = r#"const __INK_FAST_METHOD_IDS = { is_dirty: 1, clear_dirty: 2 };"#;
        let result = parse(source);
        println!("Result: {:?}", result);
        assert!(result.is_ok(), "Failed to parse: {:?}", result);
    }
}

#[cfg(test)]
mod runtime_parse_tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_parse_runtime_js() {
        // Parse the entire runtime.js file at once
        let runtime = fs::read_to_string("src/runtime.js").unwrap();
        
        match parse(&runtime) {
            Ok(_) => {
                println!("Runtime.js parsed successfully! ({} chars)", runtime.len());
            }
            Err(e) => {
                panic!("Failed to parse runtime.js: {}", e);
            }
        }
    }
}
