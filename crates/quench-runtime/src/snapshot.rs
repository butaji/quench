//! A startup snapshot: a VM's heap, realm and program store written after its host has
//! bootstrapped, and restored by a later process of the same build instead of bootstrapping
//! again (TOD-snapshot). Node's library modules, which every process evaluated at start,
//! then come back as the objects they built.
//!
//! The format keeps every identity space as it was: cell slots, record ids, atom ids, shape
//! ids and program ids are written at their indices and restored at the same ones, so a
//! `Value` is the same 64 bits after restore and no reference needs rewriting. Only the
//! Rust structures behind cells (boxed extras, owned element vectors, strings) are encoded
//! field by field, each by the module that defines it.
//!
//! A snapshot is an optimization: a state it cannot express (a suspended generator, a
//! pending job, WebAssembly, an object an embedder pinned) refuses the write, and the
//! process that wanted it bootstraps as before. Natives and host functions are written as
//! their indices, which are stable only within one build, so the header carries the
//! runtime's ABI fingerprint and the host keys the file by its own build identity.

/// Why a snapshot could not be written or read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SnapshotError(pub(crate) String);

impl SnapshotError {
    pub(crate) fn unsupported(what: &str) -> Self {
        Self(format!("snapshot cannot hold {what}"))
    }
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

pub(crate) type SnapshotResult<T> = Result<T, SnapshotError>;

/// The first bytes of every snapshot.
const MAGIC: &[u8; 8] = b"QUENCHSS";

/// A snapshot's bytes as they are written.
#[derive(Default)]
pub(crate) struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    pub(crate) fn new() -> Self {
        let mut writer = Self { bytes: Vec::with_capacity(1 << 20) };
        writer.bytes.extend_from_slice(MAGIC);
        writer.u64(crate::ResidualProgram::RUNTIME_ABI_FINGERPRINT);
        writer
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.bytes
    }

    pub(crate) fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub(crate) fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    pub(crate) fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn i32(&mut self, value: i32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn f64(&mut self, value: f64) {
        self.u64(value.to_bits());
    }

    /// A length or index, which a snapshot keeps below `u32::MAX`.
    pub(crate) fn len(&mut self, value: usize) {
        self.u32(u32::try_from(value).expect("snapshot lengths fit u32"));
    }

    /// A `Value`, as its bits: every heap index it names is restored at the same index.
    pub(crate) fn value(&mut self, value: crate::value::Value) {
        self.u64(value.to_bits());
    }

    pub(crate) fn values(&mut self, values: &[crate::value::Value]) {
        self.len(values.len());
        for value in values {
            self.value(*value);
        }
    }

    pub(crate) fn option_value(&mut self, value: Option<crate::value::Value>) {
        self.bool(value.is_some());
        if let Some(value) = value {
            self.value(value);
        }
    }

    pub(crate) fn bytes(&mut self, bytes: &[u8]) {
        self.len(bytes.len());
        self.bytes.extend_from_slice(bytes);
    }

    pub(crate) fn str(&mut self, text: &str) {
        self.bytes(text.as_bytes());
    }

    pub(crate) fn option_str(&mut self, text: Option<&str>) {
        self.bool(text.is_some());
        if let Some(text) = text {
            self.str(text);
        }
    }

    pub(crate) fn u16s(&mut self, units: &[u16]) {
        self.len(units.len());
        for unit in units {
            self.u16(*unit);
        }
    }

    pub(crate) fn u32s(&mut self, values: &[u32]) {
        self.len(values.len());
        for value in values {
            self.u32(*value);
        }
    }
}

/// A snapshot's bytes as they are read back.
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
    /// Given each stretch of read bytes once (`Host::kept_bytes_read`).
    release: Option<fn(&[u8])>,
    /// Where the bytes not yet given to `release` start.
    released: usize,
}

/// The read bytes a reader gathers before it gives them to its `release`: enough pages that
/// releasing costs a few system calls per snapshot megabyte.
const RELEASE_STRIDE: usize = 1 << 16;

impl<'a> Reader<'a> {
    /// A reader past the header, when the header is this build's, giving the bytes it has
    /// read to `release` as it goes.
    pub(crate) fn new(bytes: &'a [u8], release: Option<fn(&[u8])>) -> SnapshotResult<Self> {
        let mut reader = Self { bytes, cursor: 0, release, released: 0 };
        if reader.take(MAGIC.len())? != MAGIC {
            return Err(SnapshotError("not a Quench snapshot".into()));
        }
        if reader.u64()? != crate::ResidualProgram::RUNTIME_ABI_FINGERPRINT {
            return Err(SnapshotError("snapshot of another runtime build".into()));
        }
        Ok(reader)
    }

    pub(crate) fn finish(mut self) -> SnapshotResult<()> {
        self.release_read();
        (self.cursor == self.bytes.len())
            .then_some(())
            .ok_or_else(|| SnapshotError("trailing snapshot bytes".into()))
    }

    /// Gives the bytes read since the last release to `release`. A program's encoding, which
    /// its decoder reads past this reader's cursor (`skip`), goes once that decoder is done:
    /// when the next read follows.
    fn release_read(&mut self) {
        if let Some(release) = self.release {
            release(&self.bytes[self.released..self.cursor]);
            self.released = self.cursor;
        }
    }

    #[inline]
    fn take(&mut self, len: usize) -> SnapshotResult<&'a [u8]> {
        if self.cursor - self.released >= RELEASE_STRIDE {
            self.release_read();
        }
        let end = self.cursor.checked_add(len).ok_or_else(|| SnapshotError("snapshot overflow".into()))?;
        let bytes = self.bytes.get(self.cursor..end).ok_or_else(|| SnapshotError("truncated snapshot".into()))?;
        self.cursor = end;
        Ok(bytes)
    }

    /// The next `N` bytes. Read without a call while the read stretch has not reached
    /// `RELEASE_STRIDE` (every fixed-width read of a snapshot is one of these), else through
    /// `take`, which releases the stretch and reports a truncated snapshot.
    #[inline(always)]
    fn array<const N: usize>(&mut self) -> SnapshotResult<[u8; N]> {
        if self.cursor - self.released < RELEASE_STRIDE
            && let Some(bytes) = self.bytes.get(self.cursor..).and_then(<[u8]>::first_chunk::<N>)
        {
            self.cursor += N;
            return Ok(*bytes);
        }
        self.array_through_take()
    }

    #[inline(never)]
    fn array_through_take<const N: usize>(&mut self) -> SnapshotResult<[u8; N]> {
        Ok(self.take(N)?.try_into().expect("take returns N bytes"))
    }

    #[inline]
    pub(crate) fn u8(&mut self) -> SnapshotResult<u8> {
        Ok(self.array::<1>()?[0])
    }

    #[inline]
    pub(crate) fn bool(&mut self) -> SnapshotResult<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(SnapshotError("invalid snapshot flag".into())),
        }
    }

    #[inline]
    pub(crate) fn u16(&mut self) -> SnapshotResult<u16> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    #[inline]
    pub(crate) fn u32(&mut self) -> SnapshotResult<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    #[inline]
    pub(crate) fn u64(&mut self) -> SnapshotResult<u64> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    pub(crate) fn i32(&mut self) -> SnapshotResult<i32> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    pub(crate) fn f64(&mut self) -> SnapshotResult<f64> {
        Ok(f64::from_bits(self.u64()?))
    }

    #[inline]
    pub(crate) fn len(&mut self) -> SnapshotResult<usize> {
        Ok(self.u32()? as usize)
    }

    #[inline]
    pub(crate) fn value(&mut self) -> SnapshotResult<crate::value::Value> {
        Ok(crate::value::Value::from_bits(self.u64()?))
    }

    pub(crate) fn values(&mut self) -> SnapshotResult<Vec<crate::value::Value>> {
        let len = self.len()?;
        let mut values = Vec::with_capacity(len.min(self.remaining() / size_of::<u64>()));
        for _ in 0..len {
            values.push(self.value()?);
        }
        Ok(values)
    }

    pub(crate) fn option_value(&mut self) -> SnapshotResult<Option<crate::value::Value>> {
        Ok(if self.bool()? { Some(self.value()?) } else { None })
    }

    pub(crate) fn bytes(&mut self) -> SnapshotResult<&'a [u8]> {
        let len = self.len()?;
        self.take(len)
    }

    pub(crate) fn str(&mut self) -> SnapshotResult<&'a str> {
        std::str::from_utf8(self.bytes()?).map_err(|_| SnapshotError("invalid snapshot text".into()))
    }

    pub(crate) fn string(&mut self) -> SnapshotResult<String> {
        self.str().map(str::to_owned)
    }

    pub(crate) fn option_string(&mut self) -> SnapshotResult<Option<String>> {
        Ok(if self.bool()? { Some(self.string()?) } else { None })
    }

    pub(crate) fn u16s(&mut self) -> SnapshotResult<Vec<u16>> {
        let len = self.len()?;
        let bytes = self.take(len.checked_mul(2).ok_or_else(|| SnapshotError("snapshot overflow".into()))?)?;
        Ok(bytes.chunks_exact(2).map(|unit| u16::from_le_bytes([unit[0], unit[1]])).collect())
    }

    pub(crate) fn u32s(&mut self) -> SnapshotResult<Vec<u32>> {
        let len = self.len()?;
        let bytes = self.take(len.checked_mul(4).ok_or_else(|| SnapshotError("snapshot overflow".into()))?)?;
        Ok(bytes
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes(word.try_into().expect("chunks of four")))
            .collect())
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.cursor
    }
}

/// A list of `len` items, each read by `item`.
pub(crate) fn read_list<T>(
    reader: &mut Reader<'_>,
    mut item: impl FnMut(&mut Reader<'_>) -> SnapshotResult<T>,
) -> SnapshotResult<Vec<T>> {
    let len = reader.len()?;
    let mut items = Vec::with_capacity(len.min(1 << 20));
    for _ in 0..len {
        items.push(item(reader)?);
    }
    Ok(items)
}

/// Writes and reads a fieldless enum as its position in `variants`: one list gives both
/// directions, so encoder and decoder cannot disagree.
macro_rules! snapshot_enum {
    ($name:ident: $ty:path { $($variant:ident),+ $(,)? }) => {
        pub(crate) mod $name {
            #[allow(unused_imports)]
            use super::*;
            const VARIANTS: &[$ty] = &[$(<$ty>::$variant),+];

            pub(crate) fn write(writer: &mut $crate::snapshot::Writer, value: $ty) {
                let position = VARIANTS
                    .iter()
                    .position(|variant| std::mem::discriminant(variant) == std::mem::discriminant(&value))
                    .expect("every variant is listed");
                writer.u8(position as u8);
            }

            pub(crate) fn read(reader: &mut $crate::snapshot::Reader<'_>) -> $crate::snapshot::SnapshotResult<$ty> {
                let position = reader.u8()? as usize;
                VARIANTS
                    .get(position)
                    .copied()
                    .ok_or_else(|| $crate::snapshot::SnapshotError(concat!("invalid snapshot ", stringify!($name)).into()))
            }
        }
    };
}
pub(crate) use snapshot_enum;

/// A type a snapshot writes and reads whole. Containers and primitives implement it here; a
/// struct implements it with `snap_struct!`, which destructures every field, so a field added
/// later does not compile until it is given a place in the snapshot.
pub(crate) trait Snap: Sized {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()>;
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self>;
}

/// `Snap` for a struct, field by field; every field must be listed.
macro_rules! snap_struct {
    ($ty:ty { $($field:ident),* $(,)? }) => {
        impl $crate::snapshot::Snap for $ty {
            fn snap(&self, writer: &mut $crate::snapshot::Writer) -> $crate::snapshot::SnapshotResult<()> {
                let Self { $($field),* } = self;
                $( $crate::snapshot::Snap::snap($field, writer)?; )*
                Ok(())
            }
            fn unsnap(reader: &mut $crate::snapshot::Reader<'_>) -> $crate::snapshot::SnapshotResult<Self> {
                Ok(Self { $($field: $crate::snapshot::Snap::unsnap(reader)?),* })
            }
        }
    };
}
pub(crate) use snap_struct;

macro_rules! snap_primitive {
    ($($ty:ty: $write:ident, $read:ident;)*) => {$(
        impl Snap for $ty {
            fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
                writer.$write(*self);
                Ok(())
            }
            fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
                reader.$read()
            }
        }
    )*};
}

snap_primitive! {
    u8: u8, u8;
    u16: u16, u16;
    u32: u32, u32;
    u64: u64, u64;
    i32: i32, i32;
    f64: f64, f64;
    bool: bool, bool;
    crate::value::Value: value, value;
}

impl Snap for usize {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        writer.u64(*self as u64);
        Ok(())
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        usize::try_from(reader.u64()?).map_err(|_| SnapshotError("snapshot size overflow".into()))
    }
}

impl Snap for i64 {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        writer.u64(*self as u64);
        Ok(())
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Ok(reader.u64()? as i64)
    }
}

impl Snap for String {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        writer.str(self);
        Ok(())
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        reader.string()
    }
}

impl Snap for Box<str> {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        writer.str(self);
        Ok(())
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Ok(reader.string()?.into_boxed_str())
    }
}

impl Snap for std::rc::Rc<str> {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        writer.str(self);
        Ok(())
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Ok(std::rc::Rc::from(reader.str()?))
    }
}

impl Snap for std::path::PathBuf {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        writer.bytes(self.as_os_str().as_encoded_bytes());
        Ok(())
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        // SAFETY: the bytes came from `as_encoded_bytes` of this platform's paths.
        Ok(unsafe { std::ffi::OsStr::from_encoded_bytes_unchecked(reader.bytes()?) }.into())
    }
}

impl<T: Snap> Snap for Vec<T> {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        writer.len(self.len());
        self.iter().try_for_each(|item| item.snap(writer))
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        read_list(reader, T::unsnap)
    }
}

impl<T: Snap> Snap for Box<[T]> {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        writer.len(self.len());
        self.iter().try_for_each(|item| item.snap(writer))
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Ok(read_list(reader, T::unsnap)?.into_boxed_slice())
    }
}

impl<T: Snap> Snap for std::collections::VecDeque<T> {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        writer.len(self.len());
        self.iter().try_for_each(|item| item.snap(writer))
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Ok(read_list(reader, T::unsnap)?.into())
    }
}

impl<T: Snap> Snap for Option<T> {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        writer.bool(self.is_some());
        self.as_ref().map_or(Ok(()), |value| value.snap(writer))
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Ok(if reader.bool()? { Some(T::unsnap(reader)?) } else { None })
    }
}

impl<T: Snap> Snap for Box<T> {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        (**self).snap(writer)
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Ok(Box::new(T::unsnap(reader)?))
    }
}

impl<T: Snap, const N: usize> Snap for [T; N] {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        self.iter().try_for_each(|item| item.snap(writer))
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        let items = (0..N).map(|_| T::unsnap(reader)).collect::<SnapshotResult<Vec<T>>>()?;
        Ok(items.try_into().unwrap_or_else(|_| unreachable!("N items were read")))
    }
}

impl<A: Snap, B: Snap> Snap for (A, B) {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        self.0.snap(writer)?;
        self.1.snap(writer)
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Ok((A::unsnap(reader)?, B::unsnap(reader)?))
    }
}

impl<A: Snap, B: Snap, C: Snap> Snap for (A, B, C) {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        self.0.snap(writer)?;
        self.1.snap(writer)?;
        self.2.snap(writer)
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Ok((A::unsnap(reader)?, B::unsnap(reader)?, C::unsnap(reader)?))
    }
}

impl<K: Snap + Eq + std::hash::Hash, V: Snap, S: std::hash::BuildHasher + Default> Snap for std::collections::HashMap<K, V, S> {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        writer.len(self.len());
        self.iter().try_for_each(|(key, value)| {
            key.snap(writer)?;
            value.snap(writer)
        })
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        let len = reader.len()?;
        let mut map = Self::with_capacity_and_hasher(len.min(1 << 20), S::default());
        for _ in 0..len {
            let key = K::unsnap(reader)?;
            map.insert(key, V::unsnap(reader)?);
        }
        Ok(map)
    }
}

impl<K: Snap + Eq + std::hash::Hash, S: std::hash::BuildHasher + Default> Snap for std::collections::HashSet<K, S> {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        writer.len(self.len());
        self.iter().try_for_each(|key| key.snap(writer))
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        let len = reader.len()?;
        let mut set = Self::with_capacity_and_hasher(len.min(1 << 20), S::default());
        for _ in 0..len {
            set.insert(K::unsnap(reader)?);
        }
        Ok(set)
    }
}

impl<K: Snap + Ord, V: Snap> Snap for std::collections::BTreeMap<K, V> {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        writer.len(self.len());
        self.iter().try_for_each(|(key, value)| {
            key.snap(writer)?;
            value.snap(writer)
        })
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        let len = reader.len()?;
        let mut map = Self::new();
        for _ in 0..len {
            let key = K::unsnap(reader)?;
            map.insert(key, V::unsnap(reader)?);
        }
        Ok(map)
    }
}

/// Part of a snapshot's bytes, which a restored program keeps for its records decoded on first
/// read (`crate::KeptBytes`).
pub(crate) struct Window {
    bytes: crate::KeptBytes,
    start: usize,
    len: usize,
}

impl Window {
    pub(crate) fn new(bytes: crate::KeptBytes, start: usize, len: usize) -> SnapshotResult<Self> {
        let total = (*bytes).as_ref().len();
        if start.checked_add(len).is_none_or(|end| end > total) {
            return Err(SnapshotError("snapshot window outside its bytes".into()));
        }
        Ok(Self { bytes, start, len })
    }
}

impl AsRef<[u8]> for Window {
    fn as_ref(&self) -> &[u8] {
        &(*self.bytes).as_ref()[self.start..self.start + self.len]
    }
}

impl<'a> Reader<'a> {
    /// The reader's position in its bytes.
    pub(crate) fn position(&self) -> usize {
        self.cursor
    }

    /// Moves past `len` bytes another reader decodes (a program's encoding).
    pub(crate) fn skip(&mut self, len: usize) -> SnapshotResult<()> {
        self.take(len).map(drop)
    }
}

/// Where a host writes its own state into a snapshot, after the VM's: plain data, and the
/// roots it holds into the heap the snapshot keeps.
pub struct SnapshotWriter<'a> {
    pub(crate) writer: &'a mut Writer,
    pub(crate) heap: &'a crate::heap::Heap,
}

impl SnapshotWriter<'_> {
    pub fn u8(&mut self, value: u8) {
        self.writer.u8(value);
    }
    pub fn bool(&mut self, value: bool) {
        self.writer.bool(value);
    }
    pub fn u32(&mut self, value: u32) {
        self.writer.u32(value);
    }
    pub fn u64(&mut self, value: u64) {
        self.writer.u64(value);
    }
    pub fn f64(&mut self, value: f64) {
        self.writer.f64(value);
    }
    pub fn len(&mut self, value: usize) {
        self.writer.len(value);
    }
    pub fn str(&mut self, text: &str) {
        self.writer.str(text);
    }
    pub fn bytes(&mut self, bytes: &[u8]) {
        self.writer.bytes(bytes);
    }
    /// A root the host holds, which `SnapshotReader::root` gives back in the restored heap.
    pub fn root(&mut self, root: crate::RootId) -> Result<(), String> {
        let (slot, generation) =
            self.heap.snapshot_root(root).ok_or("a snapshot root belongs to another heap")?;
        self.writer.u32(slot);
        self.writer.u32(generation);
        Ok(())
    }
    pub fn option_root(&mut self, root: Option<crate::RootId>) -> Result<(), String> {
        self.writer.bool(root.is_some());
        root.map_or(Ok(()), |root| self.root(root))
    }
    pub fn weak_root(&mut self, root: &crate::HostWeakRoot) -> Result<(), String> {
        self.root(root.0)
    }
}

/// Where a host reads back what it wrote through `SnapshotWriter`.
pub struct SnapshotReader<'a, 'b> {
    pub(crate) reader: &'a mut Reader<'b>,
    pub(crate) heap: &'a crate::heap::Heap,
}

impl SnapshotReader<'_, '_> {
    pub fn u8(&mut self) -> Result<u8, String> {
        self.reader.u8().map_err(|error| error.0)
    }
    pub fn bool(&mut self) -> Result<bool, String> {
        self.reader.bool().map_err(|error| error.0)
    }
    pub fn u32(&mut self) -> Result<u32, String> {
        self.reader.u32().map_err(|error| error.0)
    }
    pub fn u64(&mut self) -> Result<u64, String> {
        self.reader.u64().map_err(|error| error.0)
    }
    pub fn f64(&mut self) -> Result<f64, String> {
        self.reader.f64().map_err(|error| error.0)
    }
    pub fn len(&mut self) -> Result<usize, String> {
        self.reader.len().map_err(|error| error.0)
    }
    pub fn string(&mut self) -> Result<String, String> {
        self.reader.string().map_err(|error| error.0)
    }
    pub fn bytes(&mut self) -> Result<Vec<u8>, String> {
        self.reader.bytes().map(<[u8]>::to_vec).map_err(|error| error.0)
    }
    pub fn root(&mut self) -> Result<crate::RootId, String> {
        let slot = self.u32()?;
        let generation = self.u32()?;
        self.heap.restored_root(slot, generation).ok_or_else(|| "snapshot root is not live".into())
    }
    pub fn option_root(&mut self) -> Result<Option<crate::RootId>, String> {
        if self.bool()? { self.root().map(Some) } else { Ok(None) }
    }
    pub fn weak_root(&mut self) -> Result<crate::HostWeakRoot, String> {
        self.root().map(crate::HostWeakRoot)
    }
}
