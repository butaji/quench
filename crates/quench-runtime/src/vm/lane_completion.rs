//! How a lane run left its last activation, which the code it exits to takes once: one state
//! where four separate slots on the VM let two completions stand at once.

use crate::{JsError, Value};

/// The completion a lane run left for the code it exits to (`Vm::lane_completion`).
#[derive(Debug, Default)]
pub enum LaneCompletion {
    /// No completion waits.
    #[default]
    None,
    /// A frame's own call record returned this value (`dispatch_fast::frame_return`): the
    /// general loop returns it from the frame at that instruction.
    Returned(Value),
    /// A native's frameless callback completed with this value (`Completion::ReturnsToNative`):
    /// its run hands it to the native.
    NativeReturned(Value),
    /// The activation a lane job ran awaited or yielded this value (`dispatch_fast::await_miss`,
    /// `dispatch_fast::lane_yield`): its run hands it to the await's reactions, or to the native
    /// `next` that stepped a sync generator.
    Suspended(Value),
    /// A call the lane made in place raised this error (`dispatch_fast::call_native_in_lane`): the
    /// general loop raises it at that call's instruction.
    Raised(JsError),
}

impl LaneCompletion {
    /// Leaves `exit` for the code the lane exits to; a raise replaces a completion not taken.
    #[inline(always)]
    pub fn set(&mut self, exit: LaneCompletion) {
        debug_assert!(matches!(self, Self::None) || matches!(exit, Self::Raised(_)), "two lane completions: {self:?} then {exit:?}");
        *self = exit;
    }

    #[inline(always)]
    pub fn raise(&mut self, error: JsError) {
        self.set(Self::Raised(error));
    }

    #[inline(always)]
    pub fn is_raised(&self) -> bool {
        matches!(self, Self::Raised(_))
    }

    #[inline(always)]
    pub(crate) fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    pub fn raised_mut(&mut self) -> Option<&mut JsError> {
        match self {
            Self::Raised(error) => Some(error),
            _ => None,
        }
    }

    #[inline(always)]
    pub fn take_raised(&mut self) -> Option<JsError> {
        match std::mem::take(self) {
            Self::Raised(error) => Some(error),
            other => {
                *self = other;
                None
            }
        }
    }

    #[inline(always)]
    pub(crate) fn take_returned(&mut self) -> Option<Value> {
        self.take_value(|exit| matches!(exit, Self::Returned(_)))
    }

    #[inline(always)]
    pub fn take_native_returned(&mut self) -> Option<Value> {
        self.take_value(|exit| matches!(exit, Self::NativeReturned(_)))
    }

    #[inline(always)]
    pub fn take_suspended(&mut self) -> Option<Value> {
        self.take_value(|exit| matches!(exit, Self::Suspended(_)))
    }

    /// The value of a completion `kind` accepts, taken; any other stays.
    #[inline(always)]
    fn take_value(&mut self, kind: impl Fn(&Self) -> bool) -> Option<Value> {
        if !kind(self) {
            return None;
        }
        match std::mem::take(self) {
            Self::Returned(value) | Self::NativeReturned(value) | Self::Suspended(value) => Some(value),
            _ => None,
        }
    }
}
