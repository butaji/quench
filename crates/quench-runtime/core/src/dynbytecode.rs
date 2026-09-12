use super::*;

pub type Register = u16;
const UNRESOLVED_TARGET: usize = usize::MAX;
pub(crate) const THIS_BINDING_NAME: &str = "this";
pub(crate) const ARGUMENTS_BINDING_NAME: &str = "arguments";
pub(crate) const THROW_TYPE_ERROR_ENV_NAME: &str = "\0realm-throw-type-error";
pub(crate) const THROW_TYPE_ERROR_PROP: &str = "\0realm-throw-type-error";
pub(crate) const NON_SIMPLE_ARGUMENTS_ENV_NAME: &str = "\0non-simple-arguments";

#[derive(Clone)]
pub enum Literal {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(String),
}

impl Literal {
    pub const fn profile_kind(&self) -> &'static str {
        match self {
            Self::Undefined => "Undefined",
            Self::Null => "Null",
            Self::Bool(_) => "Bool",
            Self::Number(_) => "Number",
            Self::String(_) => "String",
        }
    }
}

impl DynOp {
    pub fn for_each_read_register(&self, mut read: impl FnMut(Register)) {
        match self {
            Self::LoadLiteral { .. }
            | Self::LoadName { .. }
            | Self::LoadLocal { .. }
            | Self::LoadThis { .. }
            | Self::NewArray { .. }
            | Self::NewObject { .. }
            | Self::MakeClosure { .. }
            | Self::MakeArrow { .. }
            | Self::RegExp { .. }
            | Self::Jump { .. }
            | Self::PushHandler { .. }
            | Self::PopHandler
            | Self::Catch { .. }
            | Self::Rethrow => {}
            Self::NewArrayFromRegisters { elements, .. } => {
                elements.iter().flatten().copied().for_each(read);
            }
            Self::NewObjectFromRegisters { values, .. } => {
                values.iter().copied().for_each(read);
            }
            Self::DeclareName { src, .. }
            | Self::DeclareLocal { src, .. }
            | Self::StoreName { src, .. }
            | Self::StoreLocal { src, .. }
            | Self::Unary { src, .. }
            | Self::Update { src, .. }
            | Self::Throw { src } => read(*src),
            Self::Move { src, .. } => read(*src),
            Self::Binary { left, right, .. }
            | Self::InstanceOf { left, right, .. }
            | Self::In { left, right, .. } => {
                read(*left);
                read(*right);
            }
            Self::GetStatic { object, .. } | Self::DeleteStatic { object, .. } => read(*object),
            Self::GetComputed { object, key, .. } | Self::DeleteComputed { object, key, .. } => {
                read(*object);
                read(*key);
            }
            Self::SetStatic { object, src, .. } => {
                read(*object);
                read(*src);
            }
            Self::SetComputed {
                object, key, src, ..
            } => {
                read(*object);
                read(*key);
                read(*src);
            }
            Self::Call {
                callee,
                receiver,
                args,
                ..
            } => {
                read(*callee);
                read(*receiver);
                args.iter().copied().for_each(read);
            }
            Self::Construct { callee, args, .. } => {
                read(*callee);
                args.iter().copied().for_each(read);
            }
            Self::JumpIfFalse { test, .. } => read(*test),
            Self::ForInInit { object, .. } => read(*object),
            Self::ForInNext { iterator, .. } => read(*iterator),
            Self::Return { src } => src.iter().copied().for_each(read),
        }
    }

    pub fn written_register(&self) -> Option<Register> {
        match self {
            Self::LoadLiteral { dst, .. }
            | Self::LoadName { dst, .. }
            | Self::LoadLocal { dst, .. }
            | Self::LoadThis { dst }
            | Self::Move { dst, .. }
            | Self::NewArray { dst }
            | Self::NewArrayFromRegisters { dst, .. }
            | Self::NewObject { dst }
            | Self::NewObjectFromRegisters { dst, .. }
            | Self::MakeClosure { dst, .. }
            | Self::MakeArrow { dst, .. }
            | Self::Unary { dst, .. }
            | Self::Binary { dst, .. }
            | Self::Update { dst, .. }
            | Self::InstanceOf { dst, .. }
            | Self::In { dst, .. }
            | Self::GetStatic { dst, .. }
            | Self::GetComputed { dst, .. }
            | Self::DeleteStatic { dst, .. }
            | Self::DeleteComputed { dst, .. }
            | Self::Call { dst, .. }
            | Self::Construct { dst, .. }
            | Self::RegExp { dst, .. }
            | Self::ForInNext { dst, .. } => Some(*dst),
            Self::DeclareName { .. }
            | Self::DeclareLocal { .. }
            | Self::StoreName { .. }
            | Self::StoreLocal { .. }
            | Self::SetStatic { .. }
            | Self::SetComputed { .. }
            | Self::Jump { .. }
            | Self::JumpIfFalse { .. }
            | Self::ForInInit { .. }
            | Self::PushHandler { .. }
            | Self::PopHandler
            | Self::Catch { .. }
            | Self::Throw { .. }
            | Self::Rethrow
            | Self::Return { .. } => None,
        }
    }

    pub(crate) fn replace_written_register(
        &mut self,
        expected: Register,
        replacement: Register,
    ) -> bool {
        let destination = match self {
            Self::LoadLiteral { dst, .. }
            | Self::LoadName { dst, .. }
            | Self::LoadLocal { dst, .. }
            | Self::LoadThis { dst }
            | Self::Move { dst, .. }
            | Self::NewArray { dst }
            | Self::NewArrayFromRegisters { dst, .. }
            | Self::NewObject { dst }
            | Self::NewObjectFromRegisters { dst, .. }
            | Self::MakeClosure { dst, .. }
            | Self::MakeArrow { dst, .. }
            | Self::Unary { dst, .. }
            | Self::Binary { dst, .. }
            | Self::Update { dst, .. }
            | Self::InstanceOf { dst, .. }
            | Self::In { dst, .. }
            | Self::GetStatic { dst, .. }
            | Self::GetComputed { dst, .. }
            | Self::DeleteStatic { dst, .. }
            | Self::DeleteComputed { dst, .. }
            | Self::Call { dst, .. }
            | Self::Construct { dst, .. }
            | Self::RegExp { dst, .. }
            | Self::ForInNext { dst, .. } => dst,
            Self::DeclareName { .. }
            | Self::DeclareLocal { .. }
            | Self::StoreName { .. }
            | Self::StoreLocal { .. }
            | Self::SetStatic { .. }
            | Self::SetComputed { .. }
            | Self::Jump { .. }
            | Self::JumpIfFalse { .. }
            | Self::ForInInit { .. }
            | Self::PushHandler { .. }
            | Self::PopHandler
            | Self::Catch { .. }
            | Self::Throw { .. }
            | Self::Rethrow
            | Self::Return { .. } => return false,
        };
        if *destination != expected {
            return false;
        }
        *destination = replacement;
        true
    }

    pub fn reads_register(&self, register: Register) -> bool {
        let mut found = false;
        self.for_each_read_register(|read| found |= read == register);
        found
    }
}

#[derive(Clone, Copy)]
pub enum UnaryKind {
    Plus,
    Negate,
    Not,
    BitNot,
    Typeof,
    Void,
}

#[derive(Clone)]
pub enum CatchBinding {
    Name(String),
    Local(usize),
}

impl UnaryKind {
    pub const fn profile_name(self) -> &'static str {
        match self {
            Self::Plus => "Plus",
            Self::Negate => "Negate",
            Self::Not => "Not",
            Self::BitNot => "BitNot",
            Self::Typeof => "Typeof",
            Self::Void => "Void",
        }
    }
}

#[derive(Clone)]
pub enum DynOp {
    LoadLiteral {
        dst: Register,
        value: Literal,
    },
    LoadName {
        dst: Register,
        name: String,
    },
    LoadLocal {
        dst: Register,
        slot: usize,
    },
    DeclareName {
        name: String,
        src: Register,
    },
    DeclareLocal {
        slot: usize,
        src: Register,
    },
    StoreName {
        name: String,
        src: Register,
    },
    StoreLocal {
        slot: usize,
        src: Register,
    },
    LoadThis {
        dst: Register,
    },
    Move {
        dst: Register,
        src: Register,
    },
    NewArray {
        dst: Register,
    },
    NewArrayFromRegisters {
        dst: Register,
        elements: Box<[Option<Register>]>,
    },
    NewObject {
        dst: Register,
    },
    NewObjectFromRegisters {
        dst: Register,
        keys: Box<[String]>,
        values: Box<[Register]>,
    },
    MakeClosure {
        dst: Register,
        function: *const Function<'static>,
    },
    MakeArrow {
        dst: Register,
        function: *const ArrowFunctionExpression<'static>,
    },
    Unary {
        dst: Register,
        src: Register,
        kind: UnaryKind,
    },
    Binary {
        dst: Register,
        left: Register,
        right: Register,
        kind: Op,
    },
    Update {
        dst: Register,
        src: Register,
        increment: bool,
    },
    InstanceOf {
        dst: Register,
        left: Register,
        right: Register,
    },
    In {
        dst: Register,
        left: Register,
        right: Register,
    },
    GetStatic {
        dst: Register,
        object: Register,
        key: String,
    },
    GetComputed {
        dst: Register,
        object: Register,
        key: Register,
    },
    SetStatic {
        object: Register,
        key: String,
        src: Register,
        strict: bool,
    },
    SetComputed {
        object: Register,
        key: Register,
        src: Register,
        accessor: Option<AccessorKind>,
        strict: bool,
    },
    DeleteStatic {
        dst: Register,
        object: Register,
        key: String,
        strict: bool,
    },
    DeleteComputed {
        dst: Register,
        object: Register,
        key: Register,
        strict: bool,
    },
    Call {
        dst: Register,
        callee: Register,
        receiver: Register,
        args: Vec<Register>,
    },
    Construct {
        dst: Register,
        callee: Register,
        args: Vec<Register>,
    },
    RegExp {
        dst: Register,
        global: bool,
        kernel: RegExpLiteralKernel,
    },
    Jump {
        target: usize,
    },
    JumpIfFalse {
        test: Register,
        target: usize,
    },
    ForInInit {
        iterator: Register,
        object: Register,
    },
    ForInNext {
        iterator: Register,
        dst: Register,
        done: usize,
    },
    PushHandler {
        target: usize,
    },
    PopHandler,
    Catch {
        binding: Option<CatchBinding>,
    },
    Throw {
        src: Register,
    },
    Rethrow,
    Return {
        src: Option<Register>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AccessorKind {
    Getter,
    Setter,
}

// One active opcode table drives naming and stencil selection. Keep semantic
// families here, beside the bytecode data, rather than duplicating matches in
// the compiler, linker, coverage, and profiler.
macro_rules! define_dyn_op_metadata {
    ($( $kind:ident: $pattern:pat => $name:literal, $family:ident ),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(usize)]
        pub enum DynOpcode {
            $( $kind, )+
        }

        impl DynOpcode {
            pub const ALL: [Self; Self::COUNT] = [$( Self::$kind, )+];
            pub const COUNT: usize = [$( stringify!($kind), )+].len();

            pub const fn name(self) -> &'static str {
                match self { $( Self::$kind => $name, )+ }
            }
        }

        impl DynOp {
            pub fn name(&self) -> &'static str {
                self.opcode().name()
            }

            pub fn opcode(&self) -> DynOpcode {
                match self { $( $pattern => DynOpcode::$kind, )+ }
            }

            pub fn stencil_family(&self) -> StencilFamily {
                match self { $( $pattern => StencilFamily::$family, )+ }
            }
        }
    };
}

define_dyn_op_metadata! {
    LoadLiteral: Self::LoadLiteral { .. } => "LoadLiteral", Constant,
    LoadName: Self::LoadName { .. } => "LoadName", Local,
    LoadLocal: Self::LoadLocal { .. } => "LoadLocal", Local,
    DeclareName: Self::DeclareName { .. } => "DeclareName", Local,
    DeclareLocal: Self::DeclareLocal { .. } => "DeclareLocal", Local,
    StoreName: Self::StoreName { .. } => "StoreName", Local,
    StoreLocal: Self::StoreLocal { .. } => "StoreLocal", Local,
    LoadThis: Self::LoadThis { .. } => "LoadThis", Argument,
    Move: Self::Move { .. } => "Move", Local,
    NewArray: Self::NewArray { .. } => "NewArray", Element,
    NewArrayFromRegisters: Self::NewArrayFromRegisters { .. } => "NewArrayFromRegisters", Element,
    NewObject: Self::NewObject { .. } => "NewObject", Property,
    NewObjectFromRegisters: Self::NewObjectFromRegisters { .. } => "NewObjectFromRegisters", Property,
    MakeClosure: Self::MakeClosure { .. } => "MakeClosure", Closure,
    MakeArrow: Self::MakeArrow { .. } => "MakeArrow", Closure,
    Unary: Self::Unary { .. } => "Unary", Arithmetic,
    Binary: Self::Binary { .. } => "Binary", Arithmetic,
    Update: Self::Update { .. } => "Update", Arithmetic,
    InstanceOf: Self::InstanceOf { .. } => "InstanceOf", Compare,
    In: Self::In { .. } => "In", Property,
    GetStatic: Self::GetStatic { .. } => "GetStatic", Property,
    GetComputed: Self::GetComputed { .. } => "GetComputed", Property,
    SetStatic: Self::SetStatic { .. } => "SetStatic", Property,
    SetComputed: Self::SetComputed { .. } => "SetComputed", Property,
    DeleteStatic: Self::DeleteStatic { .. } => "DeleteStatic", Property,
    DeleteComputed: Self::DeleteComputed { .. } => "DeleteComputed", Property,
    Call: Self::Call { .. } => "Call", Call,
    Construct: Self::Construct { .. } => "Construct", Construct,
    RegExp: Self::RegExp { .. } => "RegExp", Constant,
    Jump: Self::Jump { .. } => "Jump", Branch,
    JumpIfFalse: Self::JumpIfFalse { .. } => "JumpIfFalse", Branch,
    ForInInit: Self::ForInInit { .. } => "ForInInit", Iterator,
    ForInNext: Self::ForInNext { .. } => "ForInNext", Iterator,
    PushHandler: Self::PushHandler { .. } => "PushHandler", Exception,
    PopHandler: Self::PopHandler => "PopHandler", Exception,
    Catch: Self::Catch { .. } => "Catch", Exception,
    Throw: Self::Throw { .. } => "Throw", Exception,
    Rethrow: Self::Rethrow => "Rethrow", Exception,
    Return: Self::Return { .. } => "Return", Return,
}

#[derive(Clone)]
pub struct DynInstr {
    pub op: DynOp,
    pub span: Span,
}

pub struct DynCode {
    pub ops: Vec<DynInstr>,
    pub registers: usize,
    pub params: Vec<String>,
    pub hoisted: Vec<(String, *const Function<'static>)>,
    pub source_id: Option<usize>,
    pub blocks: Vec<(usize, usize, bool)>,
    pub bindings: Vec<String>,
    pub is_script: bool,
    pub strict: bool,
}

pub struct CompileGap {
    pub span: Span,
    pub reason: &'static str,
}

struct Control {
    breaks: Vec<usize>,
    continues: Vec<usize>,
}

enum Lvalue {
    Name(String),
    Static { object: Register, key: String },
    Computed { object: Register, key: Register },
}

impl Lvalue {
    fn object(&self) -> Register {
        match self {
            Self::Static { object, .. } | Self::Computed { object, .. } => *object,
            Self::Name(_) => unreachable!("a name has no method receiver"),
        }
    }
}

pub struct Compiler {
    ops: Vec<DynInstr>,
    next_register: Register,
    params: Vec<String>,
    local_bindings: Option<Vec<String>>,
    hoisted: Vec<(String, *const Function<'static>)>,
    controls: Vec<Control>,
    source_id: Option<usize>,
    strict: bool,
}

impl Compiler {
    pub fn compile_script(
        statements: &[Statement<'static>],
        source_id: usize,
        span: Span,
        strict: bool,
    ) -> Result<DynCode, CompileGap> {
        let mut compiler = Self {
            ops: Vec::new(),
            next_register: 0,
            params: Vec::new(),
            local_bindings: None,
            hoisted: Vec::new(),
            controls: Vec::new(),
            source_id: Some(source_id),
            strict,
        };
        compiler.collect_hoisted(statements);
        for (index, statement) in statements.iter().enumerate() {
            if index + 1 == statements.len()
                && let Statement::ExpressionStatement(item) = statement
            {
                // Script completion values are observable through direct and
                // indirect eval.  Preserve the final expression in the same
                // stencil instead of routing eval through the legacy VM.
                let src = compiler.expression(&item.expression)?;
                compiler.emit(DynOp::Return { src: Some(src) }, item.span);
            } else {
                compiler.statement(statement)?;
            }
        }
        Ok(compiler.finish(span, true))
    }

    pub fn compile(
        function: &Function<'static>,
        source_id: Option<usize>,
        strict: bool,
    ) -> Result<DynCode, CompileGap> {
        let body = function.body.as_ref().ok_or(CompileGap {
            span: function.span,
            reason: "function has no body",
        })?;
        let params = function
            .params
            .items
            .iter()
            .map(|param| {
                pattern_name(&param.pattern).ok_or(CompileGap {
                    span: param.span,
                    reason: "unsupported parameter pattern",
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut compiler = Self {
            ops: Vec::new(),
            next_register: 0,
            params,
            local_bindings: Some(Vec::new()),
            hoisted: Vec::new(),
            controls: Vec::new(),
            source_id,
            strict,
        };
        compiler.collect_hoisted(&body.statements);
        for statement in &body.statements {
            compiler.statement(statement)?;
        }
        Ok(compiler.finish(function.span, false))
    }

    pub fn compile_arrow(
        function: &ArrowFunctionExpression<'static>,
        source_id: Option<usize>,
        strict: bool,
    ) -> Result<DynCode, CompileGap> {
        let params = function
            .params
            .items
            .iter()
            .map(|param| {
                pattern_name(&param.pattern).ok_or(CompileGap {
                    span: param.span,
                    reason: "unsupported arrow parameter pattern",
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut compiler = Self {
            ops: Vec::new(),
            next_register: 0,
            params,
            local_bindings: Some(Vec::new()),
            hoisted: Vec::new(),
            controls: Vec::new(),
            source_id,
            strict,
        };
        if let ArrowFunctionBody::FunctionBody(body) = &function.body {
            compiler.collect_hoisted(&body.statements);
            for statement in &body.statements {
                compiler.statement(statement)?;
            }
        } else if let Some(expression) = function.body.as_expression() {
            let src = compiler.expression(expression)?;
            compiler.emit(DynOp::Return { src: Some(src) }, function.span);
        }
        Ok(compiler.finish(function.span, false))
    }

    fn finish(mut self, span: Span, is_script: bool) -> DynCode {
        if !matches!(
            self.ops.last().map(|item| &item.op),
            Some(DynOp::Return { .. })
        ) {
            self.emit(DynOp::Return { src: None }, span);
        }
        let bindings = if is_script {
            Vec::new()
        } else {
            lower_function_bindings(
                &mut self.ops,
                &self.params,
                &self.hoisted,
                self.local_bindings.as_deref().unwrap_or_default(),
            )
        };
        let blocks = derive_blocks(&self.ops);
        let mut code = DynCode {
            ops: self.ops,
            registers: self.next_register as usize,
            params: self.params,
            hoisted: self.hoisted,
            source_id: self.source_id,
            blocks,
            bindings,
            is_script,
            strict: self.strict,
        };
        super::region_plan::simplify_register_code(&mut code)
            .expect("compiler-produced bytecode has a valid register CFG");
        code
    }

    fn collect_hoisted(&mut self, statements: &[Statement<'static>]) {
        for statement in statements {
            if let Statement::FunctionDeclaration(function) = statement
                && let Some(id) = &function.id
            {
                self.hoisted
                    .push((id.name.to_string(), &**function as *const Function<'static>));
            }
        }
    }

    fn alloc(&mut self) -> Result<Register, CompileGap> {
        let register = self.next_register;
        self.next_register = self.next_register.checked_add(1).ok_or(CompileGap {
            span: Span::default(),
            reason: "too many registers",
        })?;
        Ok(register)
    }

    fn emit(&mut self, op: DynOp, span: Span) -> usize {
        let pc = self.ops.len();
        self.ops.push(DynInstr { op, span });
        pc
    }

    fn patch(&mut self, pc: usize, target: usize) {
        match &mut self.ops[pc].op {
            DynOp::Jump { target: slot }
            | DynOp::JumpIfFalse { target: slot, .. }
            | DynOp::PushHandler { target: slot } => *slot = target,
            DynOp::ForInNext { done, .. } => *done = target,
            _ => unreachable!(),
        }
    }

    fn statement(&mut self, statement: &Statement<'static>) -> Result<(), CompileGap> {
        use Statement::*;
        match statement {
            EmptyStatement(_) | DebuggerStatement(_) | FunctionDeclaration(_) => Ok(()),
            ExpressionStatement(item) => {
                self.expression(&item.expression)?;
                Ok(())
            }
            BlockStatement(block) => {
                for item in &block.body {
                    self.statement(item)?;
                }
                Ok(())
            }
            VariableDeclaration(declaration) => self.variable_declaration(declaration),
            ReturnStatement(item) => {
                let src = item
                    .argument
                    .as_ref()
                    .map(|value| self.expression(value))
                    .transpose()?;
                self.emit(DynOp::Return { src }, item.span);
                Ok(())
            }
            ThrowStatement(item) => {
                let src = self.expression(&item.argument)?;
                self.emit(DynOp::Throw { src }, item.span);
                Ok(())
            }
            BreakStatement(item) => {
                let jump = self.emit(
                    DynOp::Jump {
                        target: UNRESOLVED_TARGET,
                    },
                    item.span,
                );
                self.controls
                    .last_mut()
                    .ok_or(CompileGap {
                        span: item.span,
                        reason: "break outside control region",
                    })?
                    .breaks
                    .push(jump);
                Ok(())
            }
            ContinueStatement(item) => {
                let jump = self.emit(
                    DynOp::Jump {
                        target: UNRESOLVED_TARGET,
                    },
                    item.span,
                );
                self.controls
                    .last_mut()
                    .ok_or(CompileGap {
                        span: item.span,
                        reason: "continue outside loop",
                    })?
                    .continues
                    .push(jump);
                Ok(())
            }
            IfStatement(item) => self.if_statement(item),
            WhileStatement(item) => self.while_statement(item),
            DoWhileStatement(item) => self.do_while_statement(item),
            ForStatement(item) => self.for_statement(item),
            ForInStatement(item) => self.for_in_statement(item),
            ForOfStatement(item) => self.for_of_statement(item),
            SwitchStatement(item) => self.switch_statement(item),
            TryStatement(item) => self.try_statement(item),
            _ => Err(CompileGap {
                span: statement.span(),
                reason: "unsupported statement",
            }),
        }
    }

    fn variable_declaration(
        &mut self,
        declaration: &VariableDeclaration<'static>,
    ) -> Result<(), CompileGap> {
        for declarator in &declaration.declarations {
            if let Some(value) = &declarator.init {
                let src = self.expression(value)?;
                self.bind_pattern(&declarator.id, src, declarator.span)?;
            } else if self.local_bindings.is_none() {
                let src = self.literal(Literal::Undefined, declarator.span)?;
                self.bind_pattern(&declarator.id, src, declarator.span)?;
            }
        }
        Ok(())
    }

    fn declare_binding(&mut self, name: String, src: Register, span: Span) {
        if let Some(bindings) = &mut self.local_bindings
            && !bindings.contains(&name)
        {
            bindings.push(name.clone());
        }
        self.emit(DynOp::DeclareName { name, src }, span);
    }

    fn bind_pattern(
        &mut self,
        pattern: &BindingPattern<'static>,
        src: Register,
        span: Span,
    ) -> Result<(), CompileGap> {
        match pattern {
            BindingPattern::BindingIdentifier(identifier) => {
                self.declare_binding(identifier.name.to_string(), src, span);
                Ok(())
            }
            BindingPattern::AssignmentPattern(assignment) => {
                let undefined = self.literal(Literal::Undefined, assignment.span)?;
                let test = self.alloc()?;
                self.emit(
                    DynOp::Binary {
                        dst: test,
                        left: src,
                        right: undefined,
                        kind: Op::StrictEq,
                    },
                    assignment.span,
                );
                let use_source = self.emit(
                    DynOp::JumpIfFalse {
                        test,
                        target: UNRESOLVED_TARGET,
                    },
                    assignment.span,
                );
                let selected = self.alloc()?;
                let default_value = self.expression(&assignment.right)?;
                self.emit(
                    DynOp::Move {
                        dst: selected,
                        src: default_value,
                    },
                    assignment.span,
                );
                let done = self.emit(
                    DynOp::Jump {
                        target: UNRESOLVED_TARGET,
                    },
                    assignment.span,
                );
                self.patch(use_source, self.ops.len());
                self.emit(DynOp::Move { dst: selected, src }, assignment.span);
                self.patch(done, self.ops.len());
                self.bind_pattern(&assignment.left, selected, span)
            }
            BindingPattern::ObjectPattern(object) => self.bind_object_pattern(object, src, span),
            BindingPattern::ArrayPattern(array) => self.bind_array_pattern(array, src, span),
        }
    }

    fn bind_object_pattern(
        &mut self,
        pattern: &ObjectPattern<'static>,
        src: Register,
        span: Span,
    ) -> Result<(), CompileGap> {
        if let Some(rest) = &pattern.rest {
            return Err(CompileGap {
                span: rest.span,
                reason: "object rest binding stencil missing",
            });
        }
        for property in &pattern.properties {
            let key = match &property.key {
                PropertyKey::StaticIdentifier(identifier) => identifier.name.to_string(),
                PropertyKey::StringLiteral(value) => value.value.to_string(),
                _ if !property.computed => {
                    return Err(CompileGap {
                        span: property.span,
                        reason: "unsupported object binding key",
                    });
                }
                _ => {
                    return Err(CompileGap {
                        span: property.span,
                        reason: "computed object binding key stencil missing",
                    });
                }
            };
            let value = self.alloc()?;
            self.emit(
                DynOp::GetStatic {
                    dst: value,
                    object: src,
                    key,
                },
                property.span,
            );
            self.bind_pattern(&property.value, value, span)?;
        }
        Ok(())
    }

    fn bind_array_pattern(
        &mut self,
        pattern: &ArrayPattern<'static>,
        src: Register,
        span: Span,
    ) -> Result<(), CompileGap> {
        if let Some(rest) = &pattern.rest {
            return Err(CompileGap {
                span: rest.span,
                reason: "array rest binding stencil missing",
            });
        }
        for (index, element) in pattern.elements.iter().enumerate() {
            let Some(element) = element else { continue };
            let key = self.literal(Literal::Number(index as f64), element.span())?;
            let value = self.alloc()?;
            self.emit(
                DynOp::GetComputed {
                    dst: value,
                    object: src,
                    key,
                },
                element.span(),
            );
            self.bind_pattern(element, value, span)?;
        }
        Ok(())
    }

    fn if_statement(&mut self, item: &IfStatement<'static>) -> Result<(), CompileGap> {
        let test = self.expression(&item.test)?;
        let otherwise = self.emit(
            DynOp::JumpIfFalse {
                test,
                target: UNRESOLVED_TARGET,
            },
            item.test.span(),
        );
        self.statement(&item.consequent)?;
        if let Some(alternate) = &item.alternate {
            let end = self.emit(
                DynOp::Jump {
                    target: UNRESOLVED_TARGET,
                },
                item.span,
            );
            self.patch(otherwise, self.ops.len());
            self.statement(alternate)?;
            self.patch(end, self.ops.len());
        } else {
            self.patch(otherwise, self.ops.len());
        }
        Ok(())
    }

    fn while_statement(&mut self, item: &WhileStatement<'static>) -> Result<(), CompileGap> {
        let top = self.ops.len();
        let test = self.expression(&item.test)?;
        let exit = self.emit(
            DynOp::JumpIfFalse {
                test,
                target: UNRESOLVED_TARGET,
            },
            item.test.span(),
        );
        self.controls.push(Control {
            breaks: Vec::new(),
            continues: Vec::new(),
        });
        self.statement(&item.body)?;
        let control = self.controls.pop().unwrap();
        for jump in control.continues {
            self.patch(jump, top);
        }
        self.emit(DynOp::Jump { target: top }, item.span);
        let end = self.ops.len();
        self.patch(exit, end);
        for jump in control.breaks {
            self.patch(jump, end);
        }
        Ok(())
    }

    fn do_while_statement(&mut self, item: &DoWhileStatement<'static>) -> Result<(), CompileGap> {
        let top = self.ops.len();
        self.controls.push(Control {
            breaks: Vec::new(),
            continues: Vec::new(),
        });
        self.statement(&item.body)?;
        let test_pc = self.ops.len();
        let control = self.controls.pop().unwrap();
        for jump in control.continues {
            self.patch(jump, test_pc);
        }
        let test = self.expression(&item.test)?;
        let exit = self.emit(
            DynOp::JumpIfFalse {
                test,
                target: UNRESOLVED_TARGET,
            },
            item.test.span(),
        );
        self.emit(DynOp::Jump { target: top }, item.span);
        let end = self.ops.len();
        self.patch(exit, end);
        for jump in control.breaks {
            self.patch(jump, end);
        }
        Ok(())
    }

    fn for_statement(&mut self, item: &ForStatement<'static>) -> Result<(), CompileGap> {
        if let Some(init) = &item.init {
            if let ForStatementInit::VariableDeclaration(value) = init {
                self.variable_declaration(value)?;
            } else {
                self.expression(init.as_expression().ok_or(CompileGap {
                    span: item.span,
                    reason: "unsupported for initializer",
                })?)?;
            }
        }
        let top = self.ops.len();
        let exit = if let Some(test) = &item.test {
            let register = self.expression(test)?;
            Some(self.emit(
                DynOp::JumpIfFalse {
                    test: register,
                    target: UNRESOLVED_TARGET,
                },
                test.span(),
            ))
        } else {
            None
        };
        self.controls.push(Control {
            breaks: Vec::new(),
            continues: Vec::new(),
        });
        self.statement(&item.body)?;
        let update = self.ops.len();
        let control = self.controls.pop().unwrap();
        for jump in control.continues {
            self.patch(jump, update);
        }
        if let Some(expression) = &item.update {
            self.expression(expression)?;
        }
        self.emit(DynOp::Jump { target: top }, item.span);
        let end = self.ops.len();
        if let Some(exit) = exit {
            self.patch(exit, end);
        }
        for jump in control.breaks {
            self.patch(jump, end);
        }
        Ok(())
    }

    fn for_in_statement(&mut self, item: &ForInStatement<'static>) -> Result<(), CompileGap> {
        let object = self.expression(&item.right)?;
        let iterator = self.alloc()?;
        self.emit(DynOp::ForInInit { iterator, object }, item.right.span());
        let top = self.ops.len();
        let key = self.alloc()?;
        let next = self.emit(
            DynOp::ForInNext {
                iterator,
                dst: key,
                done: UNRESOLVED_TARGET,
            },
            item.span,
        );
        self.store_for_left(&item.left, key, item.span)?;
        self.controls.push(Control {
            breaks: Vec::new(),
            continues: Vec::new(),
        });
        self.statement(&item.body)?;
        let control = self.controls.pop().unwrap();
        for jump in control.continues {
            self.patch(jump, top);
        }
        self.emit(DynOp::Jump { target: top }, item.span);
        let end = self.ops.len();
        self.patch(next, end);
        for jump in control.breaks {
            self.patch(jump, end);
        }
        Ok(())
    }

    fn for_of_statement(&mut self, item: &ForOfStatement<'static>) -> Result<(), CompileGap> {
        if item.r#await {
            return Err(CompileGap {
                span: item.span,
                reason: "async for-of stencil missing",
            });
        }
        let object = self.expression(&item.right)?;
        let index = self.literal(Literal::Number(0.0), item.span)?;
        let length = self.alloc()?;
        self.emit(
            DynOp::GetStatic {
                dst: length,
                object,
                key: "length".to_owned(),
            },
            item.right.span(),
        );
        let top = self.ops.len();
        let test = self.alloc()?;
        self.emit(
            DynOp::Binary {
                dst: test,
                left: index,
                right: length,
                kind: Op::Lt,
            },
            item.span,
        );
        let exit = self.emit(
            DynOp::JumpIfFalse {
                test,
                target: UNRESOLVED_TARGET,
            },
            item.span,
        );
        let value = self.alloc()?;
        self.emit(
            DynOp::GetComputed {
                dst: value,
                object,
                key: index,
            },
            item.span,
        );
        self.store_for_left(&item.left, value, item.span)?;
        self.controls.push(Control {
            breaks: Vec::new(),
            continues: Vec::new(),
        });
        self.statement(&item.body)?;
        let control = self.controls.pop().unwrap();
        for jump in control.continues {
            self.patch(jump, top);
        }
        let one = self.literal(Literal::Number(1.0), item.span)?;
        let next = self.alloc()?;
        self.emit(
            DynOp::Binary {
                dst: next,
                left: index,
                right: one,
                kind: Op::Add,
            },
            item.span,
        );
        self.emit(
            DynOp::Move {
                dst: index,
                src: next,
            },
            item.span,
        );
        self.emit(DynOp::Jump { target: top }, item.span);
        let end = self.ops.len();
        self.patch(exit, end);
        for jump in control.breaks {
            self.patch(jump, end);
        }
        Ok(())
    }

    fn store_for_left(
        &mut self,
        left: &ForStatementLeft<'static>,
        src: Register,
        span: Span,
    ) -> Result<(), CompileGap> {
        match left {
            ForStatementLeft::VariableDeclaration(value) => {
                let declarator = value.declarations.first().ok_or(CompileGap {
                    span,
                    reason: "empty for-in declaration",
                })?;
                self.bind_pattern(&declarator.id, src, span)
            }
            _ => {
                let target = left
                    .as_assignment_target()
                    .and_then(AssignmentTarget::as_simple_assignment_target)
                    .ok_or(CompileGap {
                        span,
                        reason: "unsupported for-in target",
                    })?;
                let lvalue = self.lvalue(target)?;
                self.store(lvalue, src, span)
            }
        }
    }

    fn switch_statement(&mut self, item: &SwitchStatement<'static>) -> Result<(), CompileGap> {
        let discriminant = self.expression(&item.discriminant)?;
        let dispatch = self.emit(
            DynOp::Jump {
                target: UNRESOLVED_TARGET,
            },
            item.span,
        );
        self.controls.push(Control {
            breaks: Vec::new(),
            continues: Vec::new(),
        });
        let mut bodies = Vec::new();
        for case in &item.cases {
            bodies.push(self.ops.len());
            for statement in &case.consequent {
                self.statement(statement)?;
            }
        }
        let end_jump = self.emit(
            DynOp::Jump {
                target: UNRESOLVED_TARGET,
            },
            item.span,
        );
        let tests = self.ops.len();
        self.patch(dispatch, tests);
        let mut default = end_jump;
        for (index, case) in item.cases.iter().enumerate() {
            if let Some(test) = &case.test {
                let right = self.expression(test)?;
                let equal = self.alloc()?;
                self.emit(
                    DynOp::Binary {
                        dst: equal,
                        left: discriminant,
                        right,
                        kind: Op::StrictEq,
                    },
                    test.span(),
                );
                let miss = self.emit(
                    DynOp::JumpIfFalse {
                        test: equal,
                        target: UNRESOLVED_TARGET,
                    },
                    test.span(),
                );
                self.emit(
                    DynOp::Jump {
                        target: bodies[index],
                    },
                    case.span,
                );
                self.patch(miss, self.ops.len());
            } else {
                default = bodies[index];
            }
        }
        self.emit(DynOp::Jump { target: default }, item.span);
        let end = self.ops.len();
        self.patch(end_jump, end);
        let control = self.controls.pop().unwrap();
        for jump in control.breaks {
            self.patch(jump, end);
        }
        if !control.continues.is_empty() {
            let Some(outer) = self.controls.last_mut() else {
                return Err(CompileGap {
                    span: item.span,
                    reason: "continue inside switch requires an enclosing loop",
                });
            };
            outer.continues.extend(control.continues);
        }
        Ok(())
    }

    fn try_statement(&mut self, item: &TryStatement<'static>) -> Result<(), CompileGap> {
        if item.handler.is_none() {
            let handler = self.emit(
                DynOp::PushHandler {
                    target: UNRESOLVED_TARGET,
                },
                item.span,
            );
            for statement in &item.block.body {
                self.statement(statement)?;
            }
            self.emit(DynOp::PopHandler, item.span);
            if let Some(block) = &item.finalizer {
                for statement in &block.body {
                    self.statement(statement)?;
                }
            }
            let done = self.emit(
                DynOp::Jump {
                    target: UNRESOLVED_TARGET,
                },
                item.span,
            );
            self.patch(handler, self.ops.len());
            if let Some(block) = &item.finalizer {
                for statement in &block.body {
                    self.statement(statement)?;
                }
            }
            self.emit(DynOp::Rethrow, item.span);
            self.patch(done, self.ops.len());
            return Ok(());
        }
        let handler = self.emit(
            DynOp::PushHandler {
                target: UNRESOLVED_TARGET,
            },
            item.span,
        );
        for statement in &item.block.body {
            self.statement(statement)?;
        }
        self.emit(DynOp::PopHandler, item.span);
        let skip = self.emit(
            DynOp::Jump {
                target: UNRESOLVED_TARGET,
            },
            item.span,
        );
        self.patch(handler, self.ops.len());
        if let Some(catch) = &item.handler {
            let binding = catch
                .param
                .as_ref()
                .and_then(|param| pattern_name(&param.pattern))
                .map(CatchBinding::Name);
            self.emit(DynOp::Catch { binding }, catch.span);
            for statement in &catch.body.body {
                self.statement(statement)?;
            }
        } else {
            return Err(CompileGap {
                span: item.span,
                reason: "try without catch is not yet representable",
            });
        }
        let finalizer = self.ops.len();
        self.patch(skip, finalizer);
        if let Some(block) = &item.finalizer {
            for statement in &block.body {
                self.statement(statement)?;
            }
        }
        Ok(())
    }

    fn expression(&mut self, expression: &Expression<'static>) -> Result<Register, CompileGap> {
        use Expression::*;
        match expression {
            BooleanLiteral(value) => self.literal(Literal::Bool(value.value), value.span),
            NullLiteral(value) => self.literal(Literal::Null, value.span),
            NumericLiteral(value) => self.literal(Literal::Number(value.value), value.span),
            // Keep the source kind explicit at the semantic boundary. Generic
            // numeric operations consume this marker through `Value::number`,
            // while APIs that reject BigInt can still observe the type.
            BigIntLiteral(value) => self.literal(
                Literal::String(format!("\0bigint:{}", value.value)),
                value.span,
            ),
            StringLiteral(value) => {
                self.literal(Literal::String(value.value.to_string()), value.span)
            }
            Identifier(value) => {
                let dst = self.alloc()?;
                self.emit(
                    DynOp::LoadName {
                        dst,
                        name: value.name.to_string(),
                    },
                    value.span,
                );
                Ok(dst)
            }
            ThisExpression(value) => {
                let dst = self.alloc()?;
                self.emit(DynOp::LoadThis { dst }, value.span);
                Ok(dst)
            }
            ParenthesizedExpression(value) => self.expression(&value.expression),
            ArrayExpression(value) => self.array_expression(value),
            ObjectExpression(value) => self.object_expression(value),
            FunctionExpression(value) => {
                let dst = self.alloc()?;
                let function = unsafe {
                    std::mem::transmute::<*const Function<'_>, *const Function<'static>>(
                        &**value as *const _,
                    )
                };
                self.emit(DynOp::MakeClosure { dst, function }, value.span);
                Ok(dst)
            }
            ArrowFunctionExpression(value) => {
                let dst = self.alloc()?;
                let function = unsafe {
                    std::mem::transmute::<
                        *const oxc_ast::ast::ArrowFunctionExpression<'_>,
                        *const oxc_ast::ast::ArrowFunctionExpression<'static>,
                    >(&**value as *const _)
                };
                self.emit(DynOp::MakeArrow { dst, function }, value.span);
                Ok(dst)
            }
            TemplateLiteral(value) => self.template_literal(value),
            SequenceExpression(value) => {
                let mut result = self.literal(Literal::Undefined, value.span)?;
                for item in &value.expressions {
                    result = self.expression(item)?;
                }
                Ok(result)
            }
            UnaryExpression(value) => self.unary_expression(value),
            BinaryExpression(value) => self.binary_expression(value),
            LogicalExpression(value) => self.logical_expression(value),
            ConditionalExpression(value) => self.conditional_expression(value),
            AssignmentExpression(value) => self.assignment_expression(value),
            UpdateExpression(value) => self.update_expression(value),
            StaticMemberExpression(value) => {
                let object = self.expression(&value.object)?;
                let dst = self.alloc()?;
                self.emit(
                    DynOp::GetStatic {
                        dst,
                        object,
                        key: value.property.name.to_string(),
                    },
                    value.span,
                );
                Ok(dst)
            }
            ComputedMemberExpression(value) => {
                let object = self.expression(&value.object)?;
                let key = self.expression(&value.expression)?;
                let dst = self.alloc()?;
                self.emit(DynOp::GetComputed { dst, object, key }, value.span);
                Ok(dst)
            }
            CallExpression(value) => self.call_expression(value),
            NewExpression(value) => self.new_expression(value),
            RegExpLiteral(value) => {
                let dst = self.alloc()?;
                self.emit(
                    DynOp::RegExp {
                        dst,
                        global: value.regex.flags.contains(RegExpFlags::G),
                        kernel: RegExpLiteralKernel::compile(
                            value.regex.pattern.text.as_str(),
                            value.regex.flags.contains(RegExpFlags::I),
                        ),
                    },
                    value.span,
                );
                Ok(dst)
            }
            _ => Err(CompileGap {
                span: expression.span(),
                reason: "unsupported expression",
            }),
        }
    }

    fn template_literal(
        &mut self,
        value: &oxc_ast::ast::TemplateLiteral<'static>,
    ) -> Result<Register, CompileGap> {
        let first = value
            .quasis
            .first()
            .and_then(|quasi| quasi.value.cooked.as_ref())
            .map(|text| text.to_string())
            .unwrap_or_default();
        let mut result = self.literal(Literal::String(first), value.span)?;
        for (index, expression) in value.expressions.iter().enumerate() {
            let value_register = self.expression(expression)?;
            // Template interpolation performs ToString (string hint), which
            // is distinct from the default-hint coercion used by `+`.
            let string_constructor = self.alloc()?;
            self.emit(
                DynOp::LoadName {
                    dst: string_constructor,
                    name: "String".into(),
                },
                expression.span(),
            );
            let receiver = self.literal(Literal::Undefined, expression.span())?;
            let string_value = self.alloc()?;
            self.emit(
                DynOp::Call {
                    dst: string_value,
                    callee: string_constructor,
                    receiver,
                    args: vec![value_register],
                },
                expression.span(),
            );
            let dst = self.alloc()?;
            self.emit(
                DynOp::Binary {
                    dst,
                    left: result,
                    right: string_value,
                    kind: Op::Add,
                },
                expression.span(),
            );
            result = dst;
            if let Some(quasi) = value.quasis.get(index + 1) {
                let text = quasi
                    .value
                    .cooked
                    .as_ref()
                    .map(|text| text.to_string())
                    .unwrap_or_default();
                let quasi_register = self.literal(Literal::String(text), quasi.span)?;
                let dst = self.alloc()?;
                self.emit(
                    DynOp::Binary {
                        dst,
                        left: result,
                        right: quasi_register,
                        kind: Op::Add,
                    },
                    quasi.span,
                );
                result = dst;
            }
        }
        Ok(result)
    }

    fn literal(&mut self, value: Literal, span: Span) -> Result<Register, CompileGap> {
        let dst = self.alloc()?;
        self.emit(DynOp::LoadLiteral { dst, value }, span);
        Ok(dst)
    }

    fn array_expression(
        &mut self,
        value: &ArrayExpression<'static>,
    ) -> Result<Register, CompileGap> {
        let dst = self.alloc()?;
        let mut elements = Vec::with_capacity(value.elements.len());
        for element in &value.elements {
            match element {
                ArrayExpressionElement::SpreadElement(spread) => {
                    return Err(CompileGap {
                        span: spread.span,
                        reason: "array spread stencil missing",
                    });
                }
                ArrayExpressionElement::Elision(_) => elements.push(None),
                expression => elements.push(Some(
                    self.expression(
                        expression
                            .as_expression()
                            .expect("non-spread array element is an expression"),
                    )?,
                )),
            }
        }
        self.emit(
            DynOp::NewArrayFromRegisters {
                dst,
                elements: elements.into_boxed_slice(),
            },
            value.span,
        );
        Ok(dst)
    }

    fn object_expression(
        &mut self,
        value: &ObjectExpression<'static>,
    ) -> Result<Register, CompileGap> {
        let aggregate_keys = value
            .properties
            .iter()
            .map(|property| match property {
                ObjectPropertyKind::ObjectProperty(property)
                    if property.kind == PropertyKind::Init && !property.computed =>
                {
                    Some(prop_key(&property.key))
                }
                _ => None,
            })
            .collect::<Option<Vec<_>>>();
        let dst = self.alloc()?;
        if let Some(aggregate_keys) = aggregate_keys {
            let mut keys = Vec::<String>::new();
            let mut sources = Vec::<Register>::new();
            for (property, key) in value.properties.iter().zip(aggregate_keys) {
                let ObjectPropertyKind::ObjectProperty(property) = property else {
                    unreachable!("aggregate eligibility accepts only ordinary properties")
                };
                let source = self.expression(&property.value)?;
                if let Some(slot) = keys.iter().position(|existing| existing == &key) {
                    sources[slot] = source;
                } else {
                    keys.push(key);
                    sources.push(source);
                }
            }
            self.emit(
                DynOp::NewObjectFromRegisters {
                    dst,
                    keys: keys.into_boxed_slice(),
                    values: sources.into_boxed_slice(),
                },
                value.span,
            );
            return Ok(dst);
        }
        self.emit(DynOp::NewObject { dst }, value.span);
        for property in &value.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                return Err(CompileGap {
                    span: property.span(),
                    reason: "object spread unsupported",
                });
            };
            if property.computed {
                let key = self.expression(property.key.as_expression().ok_or(CompileGap {
                    span: property.span,
                    reason: "computed object key missing expression",
                })?)?;
                let src = self.expression(&property.value)?;
                self.emit(
                    DynOp::SetComputed {
                        object: dst,
                        key,
                        src,
                        accessor: match property.kind {
                            PropertyKind::Get => Some(AccessorKind::Getter),
                            PropertyKind::Set => Some(AccessorKind::Setter),
                            _ => None,
                        },
                        strict: self.strict,
                    },
                    property.span,
                );
            } else {
                let src = self.expression(&property.value)?;
                let key = prop_key(&property.key);
                let key = match property.kind {
                    PropertyKind::Get => super::accessor_slot("get", &key),
                    PropertyKind::Set => super::accessor_slot("set", &key),
                    _ => key,
                };
                self.emit(
                    DynOp::SetStatic {
                        object: dst,
                        key,
                        src,
                        strict: self.strict,
                    },
                    property.span,
                );
            }
        }
        Ok(dst)
    }

    fn unary_expression(
        &mut self,
        value: &UnaryExpression<'static>,
    ) -> Result<Register, CompileGap> {
        use oxc_syntax::operator::UnaryOperator::*;
        if value.operator == Delete {
            let dst = self.alloc()?;
            // Global immutable bindings are not deletable. Keep this fact in
            // the stencil IR instead of folding every identifier delete to
            // `true`; ordinary unresolved names retain the spec's sloppy-mode
            // behavior below.
            if let Expression::Identifier(identifier) = &value.argument
                && matches!(identifier.name.as_str(), "NaN" | "Infinity" | "undefined")
            {
                self.emit(
                    DynOp::LoadLiteral {
                        dst,
                        value: Literal::Bool(false),
                    },
                    value.span,
                );
                return Ok(dst);
            }
            if let Some(member) = value.argument.as_member_expression() {
                match self.member_lvalue(member)? {
                    Lvalue::Static { object, key } => self.emit(
                        DynOp::DeleteStatic {
                            dst,
                            object,
                            key,
                            strict: self.strict,
                        },
                        value.span,
                    ),
                    Lvalue::Computed { object, key } => self.emit(
                        DynOp::DeleteComputed {
                            dst,
                            object,
                            key,
                            strict: self.strict,
                        },
                        value.span,
                    ),
                    Lvalue::Name(_) => unreachable!(),
                };
            } else {
                self.emit(
                    DynOp::LoadLiteral {
                        dst,
                        value: Literal::Bool(true),
                    },
                    value.span,
                );
            }
            return Ok(dst);
        }
        let src = self.expression(&value.argument)?;
        let dst = self.alloc()?;
        let kind = match value.operator {
            UnaryPlus => UnaryKind::Plus,
            UnaryNegation => UnaryKind::Negate,
            LogicalNot => UnaryKind::Not,
            BitwiseNot => UnaryKind::BitNot,
            Typeof => UnaryKind::Typeof,
            Void => UnaryKind::Void,
            Delete => unreachable!(),
        };
        self.emit(DynOp::Unary { dst, src, kind }, value.span);
        Ok(dst)
    }

    fn binary_expression(
        &mut self,
        value: &BinaryExpression<'static>,
    ) -> Result<Register, CompileGap> {
        use oxc_syntax::operator::BinaryOperator::*;
        let left = self.expression(&value.left)?;
        let right = self.expression(&value.right)?;
        let dst = self.alloc()?;
        let op = match value.operator {
            Addition => DynOp::Binary {
                dst,
                left,
                right,
                kind: Op::Add,
            },
            Subtraction => DynOp::Binary {
                dst,
                left,
                right,
                kind: Op::Sub,
            },
            Multiplication => DynOp::Binary {
                dst,
                left,
                right,
                kind: Op::Mul,
            },
            Division => DynOp::Binary {
                dst,
                left,
                right,
                kind: Op::Div,
            },
            Remainder => DynOp::Binary {
                dst,
                left,
                right,
                kind: Op::Rem,
            },
            Exponential => DynOp::Binary {
                dst,
                left,
                right,
                kind: Op::Pow,
            },
            Equality => DynOp::Binary {
                dst,
                left,
                right,
                kind: Op::Eq,
            },
            Inequality => DynOp::Binary {
                dst,
                left,
                right,
                kind: Op::Ne,
            },
            StrictEquality => DynOp::Binary {
                dst,
                left,
                right,
                kind: Op::StrictEq,
            },
            StrictInequality => DynOp::Binary {
                dst,
                left,
                right,
                kind: Op::StrictNe,
            },
            LessThan => DynOp::Binary {
                dst,
                left,
                right,
                kind: Op::Lt,
            },
            LessEqualThan => DynOp::Binary {
                dst,
                left,
                right,
                kind: Op::Le,
            },
            GreaterThan => DynOp::Binary {
                dst,
                left,
                right,
                kind: Op::Gt,
            },
            GreaterEqualThan => DynOp::Binary {
                dst,
                left,
                right,
                kind: Op::Ge,
            },
            ShiftLeft => DynOp::Binary {
                dst,
                left,
                right,
                kind: Op::Shl,
            },
            ShiftRight => DynOp::Binary {
                dst,
                left,
                right,
                kind: Op::Shr,
            },
            ShiftRightZeroFill => DynOp::Binary {
                dst,
                left,
                right,
                kind: Op::Ushr,
            },
            BitwiseOR => DynOp::Binary {
                dst,
                left,
                right,
                kind: Op::Or,
            },
            BitwiseXOR => DynOp::Binary {
                dst,
                left,
                right,
                kind: Op::Xor,
            },
            BitwiseAnd => DynOp::Binary {
                dst,
                left,
                right,
                kind: Op::And,
            },
            Instanceof => DynOp::InstanceOf { dst, left, right },
            In => DynOp::In { dst, left, right },
        };
        self.emit(op, value.span);
        Ok(dst)
    }

    fn logical_expression(
        &mut self,
        value: &LogicalExpression<'static>,
    ) -> Result<Register, CompileGap> {
        let left = self.expression(&value.left)?;
        let dst = self.alloc()?;
        self.emit(DynOp::Move { dst, src: left }, value.left.span());
        let test = match value.operator {
            oxc_syntax::operator::LogicalOperator::And => self.emit(
                DynOp::JumpIfFalse {
                    test: left,
                    target: UNRESOLVED_TARGET,
                },
                value.span,
            ),
            oxc_syntax::operator::LogicalOperator::Or => {
                let not = self.alloc()?;
                self.emit(
                    DynOp::Unary {
                        dst: not,
                        src: left,
                        kind: UnaryKind::Not,
                    },
                    value.span,
                );
                self.emit(
                    DynOp::JumpIfFalse {
                        test: not,
                        target: UNRESOLVED_TARGET,
                    },
                    value.span,
                )
            }
            oxc_syntax::operator::LogicalOperator::Coalesce => {
                return Err(CompileGap {
                    span: value.span,
                    reason: "nullish coalescing stencil missing",
                });
            }
        };
        let right = self.expression(&value.right)?;
        self.emit(DynOp::Move { dst, src: right }, value.right.span());
        self.patch(test, self.ops.len());
        Ok(dst)
    }

    fn conditional_expression(
        &mut self,
        value: &ConditionalExpression<'static>,
    ) -> Result<Register, CompileGap> {
        let test = self.expression(&value.test)?;
        let otherwise = self.emit(
            DynOp::JumpIfFalse {
                test,
                target: UNRESOLVED_TARGET,
            },
            value.test.span(),
        );
        let dst = self.alloc()?;
        let yes = self.expression(&value.consequent)?;
        self.emit(DynOp::Move { dst, src: yes }, value.consequent.span());
        let end = self.emit(
            DynOp::Jump {
                target: UNRESOLVED_TARGET,
            },
            value.span,
        );
        self.patch(otherwise, self.ops.len());
        let no = self.expression(&value.alternate)?;
        self.emit(DynOp::Move { dst, src: no }, value.alternate.span());
        self.patch(end, self.ops.len());
        Ok(dst)
    }

    fn assignment_expression(
        &mut self,
        value: &AssignmentExpression<'static>,
    ) -> Result<Register, CompileGap> {
        let target = value.left.as_simple_assignment_target().ok_or(CompileGap {
            span: value.span,
            reason: "unsupported assignment target",
        })?;
        let lvalue = self.lvalue(target)?;
        let right = self.expression(&value.right)?;
        use oxc_syntax::operator::AssignmentOperator::*;
        let result = if value.operator == Assign {
            right
        } else {
            let old = self.load(&lvalue, value.span)?;
            let dst = self.alloc()?;
            let kind = match value.operator {
                Addition => Op::Add,
                Subtraction => Op::Sub,
                Multiplication => Op::Mul,
                Division => Op::Div,
                Remainder => Op::Rem,
                Exponential => Op::Pow,
                ShiftLeft => Op::Shl,
                ShiftRight => Op::Shr,
                ShiftRightZeroFill => Op::Ushr,
                BitwiseOR => Op::Or,
                BitwiseXOR => Op::Xor,
                BitwiseAnd => Op::And,
                _ => {
                    return Err(CompileGap {
                        span: value.span,
                        reason: "logical assignment stencil missing",
                    });
                }
            };
            self.emit(
                DynOp::Binary {
                    dst,
                    left: old,
                    right,
                    kind,
                },
                value.span,
            );
            dst
        };
        self.store(lvalue, result, value.span)?;
        Ok(result)
    }

    fn update_expression(
        &mut self,
        value: &UpdateExpression<'static>,
    ) -> Result<Register, CompileGap> {
        let target = self.lvalue(&value.argument)?;
        let old = self.load(&target, value.span)?;
        let updated = self.alloc()?;
        self.emit(
            DynOp::Update {
                dst: updated,
                src: old,
                increment: value.operator == oxc_syntax::operator::UpdateOperator::Increment,
            },
            value.span,
        );
        self.store(target, updated, value.span)?;
        Ok(if value.prefix { updated } else { old })
    }

    fn call_expression(&mut self, value: &CallExpression<'static>) -> Result<Register, CompileGap> {
        let (receiver, callee) = if let Some(member) = value.callee.as_member_expression() {
            let target = self.member_lvalue(member)?;
            let receiver = target.object();
            let callee = self.load(&target, value.span)?;
            (receiver, callee)
        } else {
            let receiver = self.literal(Literal::Undefined, value.span)?;
            let callee = self.expression(&value.callee)?;
            (receiver, callee)
        };
        if let [Argument::SpreadElement(spread)] = value.arguments.as_slice() {
            let spread_value = self.expression(&spread.argument)?;
            let apply = self.alloc()?;
            self.emit(
                DynOp::GetStatic {
                    dst: apply,
                    object: callee,
                    key: "apply".to_owned(),
                },
                spread.span,
            );
            let dst = self.alloc()?;
            self.emit(
                DynOp::Call {
                    dst,
                    callee: apply,
                    receiver: callee,
                    args: vec![receiver, spread_value],
                },
                value.span,
            );
            return Ok(dst);
        }
        let args = value
            .arguments
            .iter()
            .map(|argument| {
                argument
                    .as_expression()
                    .ok_or(CompileGap {
                        span: argument.span(),
                        reason: "spread call argument unsupported",
                    })
                    .and_then(|item| self.expression(item))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let dst = self.alloc()?;
        self.emit(
            DynOp::Call {
                dst,
                callee,
                receiver,
                args,
            },
            value.span,
        );
        Ok(dst)
    }

    fn new_expression(&mut self, value: &NewExpression<'static>) -> Result<Register, CompileGap> {
        let callee = self.expression(&value.callee)?;
        let args = value
            .arguments
            .iter()
            .map(|argument| {
                argument
                    .as_expression()
                    .ok_or(CompileGap {
                        span: argument.span(),
                        reason: "spread constructor argument unsupported",
                    })
                    .and_then(|item| self.expression(item))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let dst = self.alloc()?;
        self.emit(DynOp::Construct { dst, callee, args }, value.span);
        Ok(dst)
    }

    fn member_lvalue(&mut self, member: &MemberExpression<'static>) -> Result<Lvalue, CompileGap> {
        match member {
            MemberExpression::StaticMemberExpression(value) => Ok(Lvalue::Static {
                object: self.expression(&value.object)?,
                key: value.property.name.to_string(),
            }),
            MemberExpression::ComputedMemberExpression(value) => Ok(Lvalue::Computed {
                object: self.expression(&value.object)?,
                key: self.expression(&value.expression)?,
            }),
            _ => Err(CompileGap {
                span: member.span(),
                reason: "private member unsupported",
            }),
        }
    }

    fn lvalue(&mut self, target: &SimpleAssignmentTarget<'static>) -> Result<Lvalue, CompileGap> {
        match target {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(value) => {
                Ok(Lvalue::Name(value.name.to_string()))
            }
            SimpleAssignmentTarget::StaticMemberExpression(value) => Ok(Lvalue::Static {
                object: self.expression(&value.object)?,
                key: value.property.name.to_string(),
            }),
            SimpleAssignmentTarget::ComputedMemberExpression(value) => Ok(Lvalue::Computed {
                object: self.expression(&value.object)?,
                key: self.expression(&value.expression)?,
            }),
            _ => Err(CompileGap {
                span: target.span(),
                reason: "unsupported lvalue",
            }),
        }
    }

    fn load(&mut self, target: &Lvalue, span: Span) -> Result<Register, CompileGap> {
        let dst = self.alloc()?;
        match target {
            Lvalue::Name(name) => self.emit(
                DynOp::LoadName {
                    dst,
                    name: name.clone(),
                },
                span,
            ),
            Lvalue::Static { object, key } => self.emit(
                DynOp::GetStatic {
                    dst,
                    object: *object,
                    key: key.clone(),
                },
                span,
            ),
            Lvalue::Computed { object, key } => self.emit(
                DynOp::GetComputed {
                    dst,
                    object: *object,
                    key: *key,
                },
                span,
            ),
        };
        Ok(dst)
    }

    fn store(&mut self, target: Lvalue, src: Register, span: Span) -> Result<(), CompileGap> {
        match target {
            Lvalue::Name(name) => {
                if self.strict && matches!(name.as_str(), "undefined" | "NaN" | "Infinity") {
                    let constructor = self.alloc()?;
                    self.emit(
                        DynOp::LoadName {
                            dst: constructor,
                            name: "TypeError".into(),
                        },
                        span,
                    );
                    let message = self.literal(
                        Literal::String("Assignment to read-only global binding".into()),
                        span,
                    )?;
                    let error = self.alloc()?;
                    self.emit(
                        DynOp::Construct {
                            dst: error,
                            callee: constructor,
                            args: vec![message],
                        },
                        span,
                    );
                    self.emit(DynOp::Throw { src: error }, span);
                    return Ok(());
                }
                self.emit(DynOp::StoreName { name, src }, span);
            }
            Lvalue::Static { object, key } => {
                self.emit(
                    DynOp::SetStatic {
                        object,
                        key,
                        src,
                        strict: self.strict,
                    },
                    span,
                );
            }
            Lvalue::Computed { object, key } => {
                self.emit(
                    DynOp::SetComputed {
                        object,
                        key,
                        src,
                        accessor: None,
                        strict: self.strict,
                    },
                    span,
                );
            }
        }
        Ok(())
    }
}

fn lower_function_bindings(
    ops: &mut [DynInstr],
    params: &[String],
    hoisted: &[(String, *const Function<'static>)],
    declared: &[String],
) -> Vec<String> {
    let mut names = Vec::<String>::new();
    let mut slots = HashMap::<String, usize>::new();
    let mut add = |name: &str| {
        if !slots.contains_key(name) {
            let slot = names.len();
            names.push(name.to_owned());
            slots.insert(name.to_owned(), slot);
        }
    };
    add(THIS_BINDING_NAME);
    add(ARGUMENTS_BINDING_NAME);
    for parameter in params {
        add(parameter);
    }
    for (name, _) in hoisted {
        add(name);
    }
    for name in declared {
        add(name);
    }
    for instruction in ops.iter() {
        match &instruction.op {
            DynOp::DeclareName { name, .. } => add(name),
            DynOp::Catch {
                binding: Some(CatchBinding::Name(name)),
            } => add(name),
            _ => {}
        }
    }
    for instruction in ops {
        let replacement = match &instruction.op {
            DynOp::LoadName { dst, name } => slots.get(name).map(|slot| DynOp::LoadLocal {
                dst: *dst,
                slot: *slot,
            }),
            DynOp::DeclareName { name, src } => slots.get(name).map(|slot| DynOp::DeclareLocal {
                slot: *slot,
                src: *src,
            }),
            DynOp::StoreName { name, src } => slots.get(name).map(|slot| DynOp::StoreLocal {
                slot: *slot,
                src: *src,
            }),
            DynOp::LoadThis { dst } => Some(DynOp::LoadLocal {
                dst: *dst,
                slot: slots[THIS_BINDING_NAME],
            }),
            DynOp::Catch {
                binding: Some(CatchBinding::Name(name)),
            } => Some(DynOp::Catch {
                binding: Some(CatchBinding::Local(slots[name])),
            }),
            _ => None,
        };
        if let Some(replacement) = replacement {
            instruction.op = replacement;
        }
    }
    names
}

pub(crate) fn derive_blocks(ops: &[DynInstr]) -> Vec<(usize, usize, bool)> {
    let mut starts = BTreeSet::from([0usize]);
    for (pc, instruction) in ops.iter().enumerate() {
        let target = match instruction.op {
            DynOp::Jump { target }
            | DynOp::JumpIfFalse { target, .. }
            | DynOp::PushHandler { target } => Some(target),
            DynOp::ForInNext { done, .. } => Some(done),
            _ => None,
        };
        if let Some(target) = target {
            starts.insert(target);
            starts.insert(pc + 1);
        }
    }
    let starts = starts
        .into_iter()
        .filter(|pc| *pc < ops.len())
        .collect::<Vec<_>>();
    starts
        .iter()
        .enumerate()
        .map(|(index, start)| {
            let end = starts.get(index + 1).copied().unwrap_or(ops.len());
            let loop_region = ops[*start..end].iter().any(
                |instruction| matches!(instruction.op, DynOp::Jump { target } if target <= *start),
            );
            (*start, end, loop_region)
        })
        .collect()
}
