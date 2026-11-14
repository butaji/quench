//! A plan's trees as nodes: each operand form picks the monomorphic node that reads it.

use super::ints::{Bits, IntConstant, IntNode, IntOf, MoveInt, NumOf, ToInt32};
use super::nodes::{
    Arith, Constant, ConstantKey, DoubleKey, DoubleSlot, IntKey, IntSlot, IntStmt, MoveDouble, Num, NumNode, NumStmt, Numeric,
    Read, Stmt, Write,
};
use super::plan::{KeyForm, Statement, Tree};

/// What a built Number node becomes: an operand subtree or a statement.
trait Sink {
    type Out;
    fn num<N: Num + 'static>(node: N) -> Self::Out;
}

struct AsNode;
struct AsStmt;

impl Sink for AsNode {
    type Out = Box<dyn NumNode>;
    fn num<N: Num + 'static>(node: N) -> Box<dyn NumNode> {
        Box::new(node)
    }
}

impl Sink for AsStmt {
    type Out = Box<dyn Stmt>;
    fn num<N: Num + 'static>(node: N) -> Box<dyn Stmt> {
        Box::new(NumStmt(node))
    }
}

/// Binds `$x` to the Number operand form of `$tree` and evaluates `$body`: a slot, a
/// constant or an int32-keyed read inline, anything else as a boxed subtree.
macro_rules! num_operand {
    ($tree:expr, $x:ident => $body:expr) => {
        match $tree {
            Tree::Double(register) => { let $x = DoubleSlot(register); $body }
            Tree::Int(register) => { let $x = IntSlot(register); $body }
            Tree::Constant(number) => { let $x = Constant(number); $body }
            Tree::Read { dst, array, key: KeyForm::Int(key), at } => { let $x = Read { dst, array, key: IntKey(key), at }; $body }
            tree => { let $x = node(tree); $body }
        }
    };
}

/// Binds `$x` to the int32 operand form of `$tree`.
macro_rules! int_operand {
    ($tree:expr, $x:ident => $body:expr) => {
        match $tree {
            Tree::Int(register) => { let $x = IntOf(register); $body }
            Tree::Constant(number) if super::plan::i32_of(number).is_some() => {
                let $x = IntConstant(super::plan::i32_of(number).unwrap_or_default());
                $body
            }
            Tree::Double(register) => { let $x = ToInt32(DoubleSlot(register)); $body }
            tree if tree.is_int() => { let $x = int_node(tree); $body }
            tree => { let $x = ToInt32(node(tree)); $body }
        }
    };
}

/// Binds `$x` to the key form.
macro_rules! key_form {
    ($key:expr, $x:ident => $body:expr) => {
        match $key {
            KeyForm::Int(register) => { let $x = IntKey(register); $body }
            KeyForm::Double(register) => { let $x = DoubleKey(register); $body }
            KeyForm::Constant(index) => { let $x = ConstantKey(index); $body }
        }
    };
}

/// A statement's nodes.
pub(super) fn statement(statement: Statement) -> Box<dyn Stmt> {
    match statement {
        Statement::Write { array, key, value, int, at } => {
            key_form!(key, key => num_operand!(value, value => Box::new(Write { array, key, value, int, at }) as Box<dyn Stmt>))
        }
        Statement::Tree(tree) if tree.is_int() => Box::new(IntStmt(int_node(tree))),
        Statement::Tree(tree) => number::<AsStmt>(tree),
    }
}

/// A Number tree as a subtree.
pub(super) fn node(tree: Tree) -> Box<dyn NumNode> {
    if tree.is_int() {
        return Box::new(NumOf(int_node(tree)));
    }
    number::<AsNode>(tree)
}

fn number<S: Sink>(tree: Tree) -> S::Out {
    match tree {
        Tree::Arith { op, dst, left, right } => arith::<S>(op, dst, *left, *right),
        Tree::Numeric { operator, dst, left, right } => S::num(Numeric { dst, operator, left: node(*left), right: node(*right) }),
        Tree::Move { dst, src, .. } => num_operand!(*src, src => S::num(MoveDouble { dst, src })),
        Tree::Read { dst, array, key, at } => key_form!(key, key => S::num(Read { dst, array, key, at })),
        Tree::Double(register) => S::num(DoubleSlot(register)),
        Tree::Int(register) => S::num(IntSlot(register)),
        Tree::Constant(number) => S::num(Constant(number)),
        Tree::Bits { .. } => S::num(NumOf(int_node(tree))),
    }
}

fn arith<S: Sink>(op: u8, dst: u16, left: Tree, right: Tree) -> S::Out {
    match op {
        super::nodes::ADD => arith_left::<S, { super::nodes::ADD }>(dst, left, right),
        super::nodes::SUB => arith_left::<S, { super::nodes::SUB }>(dst, left, right),
        super::nodes::MUL => arith_left::<S, { super::nodes::MUL }>(dst, left, right),
        _ => arith_left::<S, { super::nodes::DIV }>(dst, left, right),
    }
}

fn arith_left<S: Sink, const OP: u8>(dst: u16, left: Tree, right: Tree) -> S::Out {
    num_operand!(left, left => arith_right::<S, OP, _>(dst, left, right))
}

fn arith_right<S: Sink, const OP: u8, L: Num + 'static>(dst: u16, left: L, right: Tree) -> S::Out {
    num_operand!(right, right => S::num(Arith::<OP, L, _> { dst, left, right }))
}

/// An int32 tree as a subtree.
fn int_node(tree: Tree) -> Box<dyn IntNode> {
    match tree {
        Tree::Bits { op, dst, left, right } => bits(op, dst, *left, *right),
        Tree::Move { dst, src, int: true } => int_operand!(*src, src => Box::new(MoveInt { dst, src }) as Box<dyn IntNode>),
        Tree::Int(register) => Box::new(IntOf(register)),
        tree => int_operand!(tree, value => Box::new(value) as Box<dyn IntNode>),
    }
}

fn bits(op: u8, dst: u16, left: Tree, right: Tree) -> Box<dyn IntNode> {
    use super::ints::{AND, OR, SAR, SHL, XOR};
    match op {
        OR => bits_left::<OR>(dst, left, right),
        AND => bits_left::<AND>(dst, left, right),
        XOR => bits_left::<XOR>(dst, left, right),
        SHL => bits_left::<SHL>(dst, left, right),
        _ => bits_left::<SAR>(dst, left, right),
    }
}

fn bits_left<const OP: u8>(dst: u16, left: Tree, right: Tree) -> Box<dyn IntNode> {
    int_operand!(left, left => bits_right::<OP, _>(dst, left, right))
}

fn bits_right<const OP: u8, L: super::nodes::Int + 'static>(dst: u16, left: L, right: Tree) -> Box<dyn IntNode> {
    int_operand!(right, right => Box::new(Bits::<OP, L, _> { dst, left, right }) as Box<dyn IntNode>)
}
