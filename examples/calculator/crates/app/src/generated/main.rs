// Module: main.r

use protocol::{AppState, Filter, Task};

#[derive(Debug, Clone)]
pub struct Stack {
    pub items: Vec<T>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Expr {
    Number { value: f64 },
    Add { left: Expr, right: Expr },
    Sub { left: Expr, right: Expr },
    Mul { left: Expr, right: Expr },
    Div { left: Expr, right: Expr },
    Neg { expr: Expr },
    Var { name: String },
}

pub fn parse_number(s: String) -> Option<f64> {
    let n: () = parse_float(s);
    if n == n {
                return n;
    }
        return None;
}

pub fn evaluate(expr: Expr, vars: Record<String>) -> Result<f64, String> {
    match expr.tag {
        "Number" =>  {
                        return Result<f64, String> { ok: true, value: expr.value };
        }
        "Var" =>  {
            let val: () = vars.get(expr.name);
            if val != undefined {
                                return Result<f64, String> { ok: true, value: val };
            }
                        return Result<f64, String> { ok: false, error: format!("{}{}", "Unknown variable: ", expr.name) };
        }
        "Neg" =>  {
            let inner: () = evaluate(expr.expr, vars);
            if !inner.ok {
                                return inner;
            }
                        return Result<f64, String> { ok: true, value: -inner.value };
        }
        "Add" =>  {
                        return eval_binary(expr.left, expr.right, vars, |a, b| a + b);
        }
        "Sub" =>  {
                        return eval_binary(expr.left, expr.right, vars, |a, b| a - b);
        }
        "Mul" =>  {
                        return eval_binary(expr.left, expr.right, vars, |a, b| a * b);
        }
        "Div" =>  {
                        return eval_binary(expr.left, expr.right, vars, |a, b| {             if b == 0i32 {
                                return Result<f64, String> { ok: false, error: "Division by zero" };
            }
                        return Result<f64, String> { ok: true, value: a / b };
 });
        }
    }
}

pub fn eval_binary(left: Expr, right: Expr, vars: Record<String>, op: ()) -> Result<f64, String> {
    let left_result: () = evaluate(left, vars);
    if !left_result.ok {
                return left_result;
    }
    let right_result: () = evaluate(right, vars);
    if !right_result.ok {
                return right_result;
    }
        return op(left_result.value, right_result.value);
}

pub fn to_string(expr: Expr) -> String {
    match expr.tag {
        "Number" =>  {
                        return string(expr.value);
        }
        "Var" =>  {
                        return expr.name;
        }
        "Neg" =>  {
                        return format!("{}{}", "-", to_string(expr.expr));
        }
        "Add" =>  {
                        return format!("{}{}", format!("{}{}", format!("{}{}", format!("{}{}", "(", to_string(expr.left)), " + "), to_string(expr.right)), ")");
        }
        "Sub" =>  {
                        return format!("{}{}", format!("{}{}", format!("{}{}", format!("{}{}", "(", to_string(expr.left)), " - "), to_string(expr.right)), ")");
        }
        "Mul" =>  {
                        return format!("{}{}", format!("{}{}", format!("{}{}", format!("{}{}", "(", to_string(expr.left)), " * "), to_string(expr.right)), ")");
        }
        "Div" =>  {
                        return format!("{}{}", format!("{}{}", format!("{}{}", format!("{}{}", "(", to_string(expr.left)), " / "), to_string(expr.right)), ")");
        }
    }
}

pub fn simplify(expr: Expr) -> Expr {
    match expr.tag {
        "Number" =>  {
        }
        "Var" =>  {
                        return expr;
        }
        "Neg" =>  {
            let neg_inner: () = simplify(expr.expr);
            if neg_inner.tag == "Number" {
                                return Expr { tag: "Number", value: -neg_inner.value };
            }
                        return Expr { tag: "Neg", expr: neg_inner };
        }
        "Add" =>  {
        }
        "Sub" =>  {
        }
        "Mul" =>  {
        }
        "Div" =>  {
            let left: () = simplify(expr.left);
            let right: () = simplify(expr.right);
            if left.tag == "Number" && right.tag == "Number" {
                let result: () = evaluate(Expr { tag: expr.tag, left: left, right: right }, Expr {  });
                if result.ok {
                                        return Expr { tag: "Number", value: result.value };
                }
            }
            if expr.tag == "Add" && left.tag == "Number" && left.value == 0i32 {
                                return right;
            }
            if expr.tag == "Add" && right.tag == "Number" && right.value == 0i32 {
                                return left;
            }
            if expr.tag == "Mul" && left.tag == "Number" && left.value == 1i32 {
                                return right;
            }
            if expr.tag == "Mul" && right.tag == "Number" && right.value == 1i32 {
                                return left;
            }
            if expr.tag == "Mul" && (left.tag == "Number" && left.value == 0i32 || right.tag == "Number" && right.value == 0i32) {
                                return Expr { tag: "Number", value: 0i32 };
            }
                        return Expr { tag: expr.tag, left: left, right: right };
        }
    }
}

pub fn push(stack: Stack<T>, item: T) -> Stack<T> {
        return Stack<T> { items: stack.items.iter().concat(vec![item]) };
}

pub fn pop(stack: Stack<T>) -> __AnonymousStruct1 {
    if stack.items.len() == 0i32 {
                return __AnonymousStruct1 { stack: stack, item: None };
    }
    let new_items: () = stack.items.as_slice()[0i32 as usize..-1i32 as usize];
    let item: () = stack.items.get(stack.items.len() - 1i32);
        return __AnonymousStruct1 { stack: __AnonymousStruct1 { items: new_items }, item: item };
}

pub fn peek(stack: Stack<T>) -> Option<T> {
    if stack.items.len() == 0i32 {
                return None;
    }
        return stack.items.get(stack.items.len() - 1i32);
}

#[derive(Debug, Clone)]
pub struct __AnonymousStruct1 {
    pub stack: Stack<T>,
    pub item: Option<T>,
}


