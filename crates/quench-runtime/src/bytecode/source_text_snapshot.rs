//! A program's source text in a startup snapshot.
use super::SourceText;

/// A static item at a fixed place in the executable image, from which a snapshot measures the
/// embedded text a program retains (`SourceText::Static`): within one linked image the offset
/// between two statics is fixed, and a snapshot is read only by the build that wrote it.
static SNAPSHOT_TEXT_ANCHOR: u8 = 0;

/// The bytes at each end of embedded text that a snapshot checks before using the text.
const SNAPSHOT_TEXT_EDGE: usize = 32;

impl SourceText {
    pub(crate) fn write_snapshot(&self, writer: &mut crate::snapshot::Writer) {
        match self {
            // A snapshot outlives the entry's mapping, so it keeps its own copy of kept text.
            Self::Shared(_) | Self::Local(_) | Self::Kept(_) => {
                writer.u8(0);
                writer.str(self);
            }
            Self::Static(text) => {
                writer.u8(1);
                let anchor = std::ptr::addr_of!(SNAPSHOT_TEXT_ANCHOR) as usize as i64;
                writer.u64((text.as_ptr() as usize as i64).wrapping_sub(anchor) as u64);
                writer.len(text.len());
                let edge = SNAPSHOT_TEXT_EDGE.min(text.len());
                writer.bytes(&text.as_bytes()[..edge]);
                writer.bytes(&text.as_bytes()[text.len() - edge..]);
            }
        }
    }

    pub(crate) fn read_snapshot(reader: &mut crate::snapshot::Reader<'_>) -> crate::snapshot::SnapshotResult<Self> {
        Ok(match reader.u8()? {
            0 => Self::Shared(std::sync::Arc::from(reader.str()?)),
            1 => {
                let offset = reader.u64()? as i64;
                let len = reader.len()?;
                let head = reader.bytes()?;
                let tail = reader.bytes()?;
                let anchor = std::ptr::addr_of!(SNAPSHOT_TEXT_ANCHOR) as usize as i64;
                let address = anchor.wrapping_add(offset) as usize as *const u8;
                // SAFETY: the snapshot was written by this build (its header names the runtime
                // ABI and the host keys it by the executable's identity), whose image holds the
                // embedded text at this offset from the anchor; its edges are checked below.
                let bytes = unsafe { std::slice::from_raw_parts(address, len) };
                if &bytes[..head.len()] != head || &bytes[len - tail.len()..] != tail {
                    return Err(crate::snapshot::SnapshotError("snapshot embedded text moved".into()));
                }
                // SAFETY: the text was a `&'static str` of this image when written.
                Self::Static(unsafe { std::str::from_utf8_unchecked(bytes) })
            }
            _ => return Err(crate::snapshot::SnapshotError("invalid snapshot source text".into())),
        })
    }
}
