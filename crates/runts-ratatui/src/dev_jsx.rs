//!
//! JSX-to-JS transformer for the `runts dev --ink` path.
//!
//! The user's `.tsx` is a plain Ink source: it has
//! `import { Box, Text } from 'ink'` and uses JSX
//! (`<Box>...</Box>`). rquickjs runs the file in the
//! `runts dev` path — but rquickjs has no JSX
//! transformer. So we read the file and lower the JSX
//! to plain JS that calls into the `runts_ink`
//! namespace installed by the FFI bridge.

pub use dev_jsx::Transformed;
pub use dev_jsx::transform;
