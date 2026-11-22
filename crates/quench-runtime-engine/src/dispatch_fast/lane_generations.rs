//! The generation a lane stack resolves under (`LaneStack::generation`): every cached target and
//! memo names the one it was resolved in, and a new one retires them all at once.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;


