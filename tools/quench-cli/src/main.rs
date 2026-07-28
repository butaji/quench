//! Quench JS CLI — evaluate JavaScript from the command line.
//!
//! Usage:
//!   quench -e 'code'                     eval inline code
//!   quench -p 'expr'                     eval and print expression
//!   quench --check script.js             syntax check (parse only)
//!   quench script.js [args]              run a file
//!   quench                               read from stdin

use std::fs;
use std::process::ExitCode;
use quench_runtime::{Context, Value, JsError};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    let mut eval_expr = None;   // -e flag
    let mut print_expr = None;  // -p flag
    let mut check_only = false; // --check / -c flag
    let mut script_file = None;

    while i < args.len() {
        match args[i].as_str() {
            "-e" => {
                i += 1;
                eval_expr = args.get(i).cloned();
            }
            "-p" => {
                i += 1;
                print_expr = args.get(i).cloned();
            }
            "--check" | "-c" => {
                check_only = true;
            }
            flag if flag.starts_with('-') && flag != "-e" && flag != "-p" && flag != "--check" && flag != "-c" => {
                eprintln!("Unknown flag: {}", flag);
                eprintln!("Usage: quench [-e code|-p expr|--check file|file.js]");
                return ExitCode::from(1);
            }
            _ => {
                script_file = Some(args[i].clone());
            }
        }
        i += 1;
    }

    // Determine source and mode
    let source: String;
    let mode: &str;

    if let Some(ref code) = eval_expr {
        source = code.clone();
        mode = "eval";
    } else if let Some(ref code) = print_expr {
        source = format!("({})", code);
        mode = "print";
    } else if let Some(ref path) = script_file {
        source = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error: cannot read {}: {}", path, e);
                return ExitCode::from(1);
            }
        };
        mode = "file";
    } else {
        // Read from stdin
        source = match std::io::read_to_string(std::io::stdin()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading stdin: {}", e);
                return ExitCode::from(1);
            }
        };
        mode = "stdin";
    }

    // Syntax check mode: parse only, no execution
    if check_only {
        let mut ctx = match Context::new() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Parse error: {:?}", e);
                return ExitCode::from(1);
            }
        };
        match ctx.parse(&source) {
            Ok(_) => {
                println!("Syntax OK");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("SyntaxError: {}", e.0);
                ExitCode::from(1)
            }
        }
    } else {
        // Create context and register builtins
        let mut ctx = match Context::new() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Internal error: {:?}", e);
                return ExitCode::from(1);
            }
        };
        quench_runtime::builtins::register_builtins(&mut ctx);

        // In print mode, wrap as expression statement so it returns the value
        let code = if mode == "print" {
            format!("({})", source)
        } else {
            source
        };

        match ctx.eval(&code) {
            Ok(val) => {
                if mode == "print" || !matches!(val, Value::Undefined) {
                    println!("{}", fmt_value(&val));
                }
                ExitCode::SUCCESS
            }
            Err(JsError(msg)) => {
                eprintln!("{}", msg);
                ExitCode::from(1)
            }
        }
    }
}

fn fmt_value(v: &Value) -> String {
    match v {
        Value::Undefined => "undefined".into(),
        Value::Null => "null".into(),
        Value::Boolean(b) => b.to_string(),
        Value::Number(n) => {
            if n.is_nan() { "NaN".into() }
            else if *n == f64::INFINITY { "Infinity".into() }
            else if *n == f64::NEG_INFINITY { "-Infinity".into() }
            else { n.to_string() }
        }
        Value::String(s) => s.clone(),
        Value::Object(_) => "[object Object]".into(),
        Value::Function(_) => "[Function]".into(),
        Value::NativeFunction(_) => "[NativeFunction]".into(),
        Value::BigInt(bi) => format!("{}n", bi),
        Value::Symbol(s) => format!("Symbol({})", s.desc.as_deref().unwrap_or("")),
        Value::Class(_) => "[Class]".into(),
        Value::NativeConstructor(_) => "[NativeConstructor]".into(),
        Value::Generator(_) => "[Generator]".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fmt_value_number() {
        assert_eq!(fmt_value(&Value::Number(42.0)), "42");
        assert_eq!(fmt_value(&Value::Number(-1.5)), "-1.5");
        assert_eq!(fmt_value(&Value::Number(f64::NAN)), "NaN");
        assert_eq!(fmt_value(&Value::Number(f64::INFINITY)), "Infinity");
    }

    #[test]
    fn test_fmt_value_special() {
        assert_eq!(fmt_value(&Value::Undefined), "undefined");
        assert_eq!(fmt_value(&Value::Null), "null");
        assert_eq!(fmt_value(&Value::Boolean(true)), "true");
        assert_eq!(fmt_value(&Value::String("hello".into())), "hello");
    }
}
