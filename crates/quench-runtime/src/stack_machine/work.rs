//! Work items for the explicit-stack interpreter.

/// A unit of work for the explicit-stack interpreter.
#[derive(Debug)]
pub enum Work {
    /// Push a literal value onto the operand stack.
    PushValue(crate::Value),
    /// Evaluate an expression and push its value.
    EvalExpr(std::rc::Rc<crate::ast::Expression>),
    /// Evaluate a statement and push its value.
    EvalStmt(std::rc::Rc<crate::ast::Statement>, bool),
    /// Evaluate a slice of statements; `index` is the next statement to run.
    EvalStmts(std::rc::Rc<Vec<crate::ast::Statement>>, bool, usize),

    // -----------------------------------------------------------------------
    // Expression continuations
    // -----------------------------------------------------------------------
    /// Pop two values, apply a binary operator, push the result.
    ApplyBinary(crate::ast::BinaryOp),
    /// Pop one value, apply a unary operator, push the result.
    ApplyUnary(crate::ast::UnaryOp),
    /// Pop a value and assign it to an identifier or member.
    ApplyAssign { target: super::AssignmentTarget },
    /// Pop value, object, and key; assign to member property.
    ApplyMemberAssign,
    /// Pop right, left, apply binary op and assign.
    ApplyCompoundAssign { op: crate::ast::BinaryOp, target: super::AssignmentTarget },
    /// Evaluate a callee expression, leaving (function, this) on the stack.
    EvalCallee(std::rc::Rc<crate::ast::Expression>),
    /// Pop argc arguments, the function, and the `this` binding, then call.
    ApplyCall { argc: usize },
    /// Read a member property.  If `computed`, pop the key string first.
    ApplyMember { property: crate::ast::PropertyKey, computed: bool, callee_mode: bool },
    /// Pop condition and evaluate the chosen branch.
    ApplyConditional { consequent: std::rc::Rc<crate::ast::Expression>, alternate: std::rc::Rc<crate::ast::Expression> },
    /// Pop current/old value, apply update, assign, push result.
    ApplyUpdate { op: crate::ast::UpdateOp, prefix: bool, target: super::AssignmentTarget },
    /// Construct an object.  Pop argc args and the constructor value.
    ApplyNew { argc: usize },
    /// Decide whether to use constructor result or the new object.
    ApplyConstructorResult { new_obj: std::rc::Rc<std::cell::RefCell<crate::value::Object>>, use_constructor_result: bool },
    /// Evaluate remaining expressions in a sequence.
    ApplySequence { exprs: std::rc::Rc<Vec<crate::ast::Expression>>, index: usize },
    /// Evaluate remaining statements in a block expression.
    ApplyBlockExpr { stmts: std::rc::Rc<Vec<crate::ast::Statement>>, index: usize },

    // -----------------------------------------------------------------------
    // Statement continuations
    // -----------------------------------------------------------------------
    ApplyIf { consequent: std::rc::Rc<crate::ast::Statement>, alternate: Option<std::rc::Rc<crate::ast::Statement>>, is_expr_body: bool },
    ApplyWhile { condition: std::rc::Rc<crate::ast::Expression>, body: std::rc::Rc<crate::ast::Statement>, is_expr_body: bool },
    ApplyWhileBody { condition: std::rc::Rc<crate::ast::Expression>, body: std::rc::Rc<crate::ast::Statement>, is_expr_body: bool },
    ApplyFor {
        condition: Option<std::rc::Rc<crate::ast::Expression>>,
        update: Option<std::rc::Rc<crate::ast::Expression>>,
        body: std::rc::Rc<crate::ast::Statement>,
        is_expr_body: bool,
        phase: super::ForPhase,
    },
    ApplyForBody {
        condition: Option<std::rc::Rc<crate::ast::Expression>>,
        update: Option<std::rc::Rc<crate::ast::Expression>>,
        body: std::rc::Rc<crate::ast::Statement>,
        is_expr_body: bool,
    },
    ApplyBlock { stmts: std::rc::Rc<Vec<crate::ast::Statement>>, index: usize, is_expr_body: bool },
    ApplyTryCatch { handler: std::rc::Rc<crate::ast::Statement>, param: Option<String>, is_expr_body: bool },
    ApplyReturn,

    // -----------------------------------------------------------------------
    // Loop helpers
    // -----------------------------------------------------------------------
    ApplyForOf {
        variable: std::rc::Rc<crate::ast::Expression>,
        body: std::rc::Rc<crate::ast::Statement>,
        items: Vec<crate::Value>,
        index: usize,
    },
    ApplyForIn {
        variable: std::rc::Rc<crate::ast::Expression>,
        body: std::rc::Rc<crate::ast::Statement>,
        keys: Vec<String>,
        index: usize,
    },

    // -----------------------------------------------------------------------
    // Object / array literal helpers
    // -----------------------------------------------------------------------
    /// Push a getter/setter/value into the object being built.
    ApplyObjectProperty { key: String, kind: super::ObjectPropertyKind, obj: std::rc::Rc<std::cell::RefCell<crate::value::Object>> },

    // -----------------------------------------------------------------------
    // Misc
    // -----------------------------------------------------------------------
    /// Discard the top value (used for non-final expressions/statements).
    Discard,
    /// Pop a value and store it as a variable declaration.
    VarDecl { kind: crate::ast::VarKind, name: String },
    /// Pop a value and store it as a `for` initializer variable.
    ForInitVar { kind: crate::ast::VarKind, name: String },
    /// Pop the iterable value and start the for-of loop.
    BeginForOf { variable: std::rc::Rc<crate::ast::Expression>, body: std::rc::Rc<crate::ast::Statement> },
    /// Pop the object value and start the for-in loop.
    BeginForIn { variable: std::rc::Rc<crate::ast::Expression>, body: std::rc::Rc<crate::ast::Statement> },
    /// Enter a try block: push a catch handler.
    PushCatch { handler: std::rc::Rc<crate::ast::Statement>, param: Option<String>, env: std::rc::Rc<std::cell::RefCell<crate::env::Environment>>, is_expr_body: bool },
    /// Leave a try block normally: pop the catch handler.
    PopCatch,
    /// Pop the current lexical scope.
    PopScope,
    /// Pop the thrown value and raise an error.
    Throw,
}
