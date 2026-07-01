//! TypeScript emit helpers as Rust native functions
//!
//! TypeScript injects helper functions into emitted JavaScript. These helpers
//! are normally provided by the TypeScript runtime, but we need to implement
//! them as native Rust functions so that baseline JS can run in quench-runtime.
//!
//! The helpers are:
//! - `__extends` - prototype-chain setup for class inheritance
//! - `__assign` - Object.assign polyfill
//! - `__awaiter` - async function state machine
//! - `__decorate` - class decorator helper
//! - `__param` - parameter decorator helper
//! - `__metadata` - metadata decorator helper
//! - `__importStar` - CommonJS interop for `import * as`
//! - `__importDefault` - CommonJS interop for `import x from`
//! - `__createBinding` - ES module binding creation
//! - `__export` - ES module export helper

/// Map of helper name to JavaScript implementation code.
/// These are registered as globals in the Context before running test cases.
pub static EMIT_HELPERS: &[(&str, &str)] = &[
    ("__extends", r#"
__extends = function(d, b) {
    __extends = Object.setPrototypeOf ||
        ({ __proto__: [] } instanceof Array && function(d, b) { d.__proto__ = b; }) ||
        function(d, b) { for (var p in b) if (Object.prototype.hasOwnProperty.call(b, p)) d[p] = b[p]; };
    __extends(d, b);
};
"#),
    ("__assign", r#"
__assign = function() {
    __assign = Object.assign || function(t) {
        for (var s, i = 1, n = arguments.length; i < n; i++) {
            s = arguments[i];
            for (var p in s) if (Object.prototype.hasOwnProperty.call(s, p)) t[p] = s[p];
        }
        return t;
    };
    return __assign.apply(this, arguments);
};
"#),
    ("__awaiter", r#"
__awaiter = function(thisArg, _arguments, P, generator) {
    function adopt(value) { return value instanceof P ? value : new P(function(resolve) { resolve(value); }); }
    return new (P || (P = Promise))(function(resolve, reject) {
        function fulfilled(value) { try { step(generator.next(value)); } catch (e) { reject(e); } }
        function rejected(value) { try { step(generator.throw(value)); } catch (e) { reject(e); } }
        function step(result) { result.done ? resolve(result.value) : adopt(result.value).then(fulfilled, rejected); }
        step((generator = generator.apply(thisArg, _arguments || [])).next());
    });
};
"#),
    ("__decorate", r#"
__decorate = function(decorators, target, key, desc) {
    var c = arguments.length, r = c < 3 ? target : desc === null ? desc = Object.getOwnPropertyDescriptor(target, key) : desc, d;
    if (typeof Reflect === "object" && typeof Reflect.decorate === "function") r = Reflect.decorate(decorators, target, key, desc);
    else for (var i = decorators.length - 1; i >= 0; i--) if (d = decorators[i]) r = (c < 3 ? d(r) : c > 3 ? d(target, key, r) : d(target, key)) || r;
    return c > 3 && r && Object.defineProperty(target, key, r), r;
};
"#),
    ("__param", r#"
__param = function(paramIndex, decorator) {
    return function(target, key) { decorator(target, key, paramIndex); };
};
"#),
    ("__metadata", r#"
__metadata = function(metadataKey, metadataValue) {
    if (typeof Reflect === "object" && typeof Reflect.metadata === "function") return Reflect.metadata(metadataKey, metadataValue);
};
"#),
    ("__importStar", r#"
__importStar = function(mod) {
    var result = {};
    if (mod != null) for (var k in mod) if (k !== "default") Object.defineProperty(result, k, { get: function() { return mod[k]; }, enumerable: true });
    Object.defineProperty(result, "__esModule", { value: true });
    return result;
};
"#),
    ("__importDefault", r#"
__importDefault = function(mod) {
    return (mod && mod.__esModule) ? mod : { default: mod };
};
"#),
    ("__createBinding", r#"
__createBinding = function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    Object.defineProperty(o, k2, { enumerable: true, get: function() { return m[k]; } });
};
"#),
    ("__export", r#"
__export = function(target, all) {
    for (var p in target) if (!all || Object.prototype.hasOwnProperty.call(target, p)) if (!exports.hasOwnProperty(p)) Object.defineProperty(exports, p, { enumerable: true, get: function() { return target[p]; } });
};
"#),
];

/// Register all emit helpers in a Context
pub fn register_helpers(ctx: &mut crate::Context) -> Result<(), String> {
    for (name, code) in EMIT_HELPERS {
        ctx.eval(code).map_err(|e| format!("Failed to register {}: {}", name, e))?;
    }
    Ok(())
}
