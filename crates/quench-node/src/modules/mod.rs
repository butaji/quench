//! Node API modules. Each module is implemented as pure Rust
//! objects installed via the runtime's host contract. There is no
//! self-hosted JavaScript builtin layer.

pub mod buffer;
pub mod console;
pub mod dns;
pub mod events;
pub mod fs;
pub mod http;
pub mod net;
pub mod os;
pub mod path;
pub mod process;
pub mod querystring;
pub mod readline;
pub mod require;
pub mod stream;
pub mod string_decoder;
pub mod timers;
pub mod tty;
pub mod url;
pub mod util;
