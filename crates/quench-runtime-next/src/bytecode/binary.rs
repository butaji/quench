use super::AtomTable;

pub(super) struct BinaryWriter {
    pub(super) bytes: Vec<u8>,
}

#[rustfmt::skip]
impl BinaryWriter {
    pub(super) fn new() -> Self { Self { bytes: Vec::new() } }
    pub(super) fn u8(&mut self, value: u8) { self.bytes.push(value); }
    pub(super) fn u16(&mut self, value: u16) { self.bytes.extend_from_slice(&value.to_le_bytes()); }
    pub(super) fn u32(&mut self, value: u32) { self.bytes.extend_from_slice(&value.to_le_bytes()); }
    pub(super) fn u64(&mut self, value: u64) { self.bytes.extend_from_slice(&value.to_le_bytes()); }
    pub(super) fn string(&mut self, value: &str) { self.u32(value.len() as u32); self.bytes.extend_from_slice(value.as_bytes()); }
    pub(super) fn strings(&mut self, values: &AtomTable) { self.u32(values.len() as u32); for value in values.iter() { self.string(value); } }
    pub(super) fn u16s(&mut self, values: &[u16]) { self.u32(values.len() as u32); for value in values { self.u16(*value); } }
    pub(super) fn option_u32(&mut self, value: Option<u32>) { self.u32(value.unwrap_or(u32::MAX)); }
    pub(super) fn pair(&mut self, value: (u32, u16)) { self.u32(value.0); self.u16(value.1); }
    pub(super) fn optional_pair(&mut self, value: Option<(u32, u16)>) { self.u8(value.is_some() as u8); if let Some(value) = value { self.pair(value); } }
}

pub(super) struct BinaryReader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

#[rustfmt::skip]
impl<'a> BinaryReader<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self { Self { bytes, cursor: 0 } }
    fn take(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self.cursor.checked_add(len).ok_or("residual overflow")?;
        let value = self.bytes.get(self.cursor..end).ok_or("truncated residual")?;
        self.cursor = end;
        Ok(value)
    }
    pub(super) fn magic(&mut self, value: &[u8]) -> Result<(), String> {
        if self.take(value.len())? == value { Ok(()) } else { Err("invalid residual header".into()) }
    }
    pub(super) fn u8(&mut self) -> Result<u8, String> { Ok(self.take(1)?[0]) }
    pub(super) fn u16(&mut self) -> Result<u16, String> { Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap())) }
    pub(super) fn u32(&mut self) -> Result<u32, String> { Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap())) }
    pub(super) fn u64(&mut self) -> Result<u64, String> { Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap())) }
    pub(super) fn string(&mut self) -> Result<String, String> {
        let len = self.u32()? as usize;
        String::from_utf8(self.take(len)?.to_vec()).map_err(|_| "invalid residual string".into())
    }
    pub(super) fn strings(&mut self) -> Result<AtomTable, String> {
        let count = self.u32()? as usize;
        let mut text = String::new();
        let mut ends = Vec::with_capacity(count);
        for _ in 0..count {
            let len = self.u32()? as usize;
            let value = std::str::from_utf8(self.take(len)?).map_err(|_| "invalid residual string")?;
            text.push_str(value);
            ends.push(text.len() as u32);
        }
        Ok(AtomTable::new(text, ends))
    }
    pub(super) fn u16s(&mut self) -> Result<Vec<u16>, String> { self.list(|input| input.u16()) }
    pub(super) fn option_u32(&mut self) -> Result<Option<u32>, String> {
        let value = self.u32()?;
        Ok((value != u32::MAX).then_some(value))
    }
    pub(super) fn pair(&mut self) -> Result<(u32, u16), String> { Ok((self.u32()?, self.u16()?)) }
    pub(super) fn optional_pair(&mut self) -> Result<Option<(u32, u16)>, String> {
        match self.u8()? { 0 => Ok(None), 1 => Ok(Some(self.pair()?)), _ => Err("invalid residual option".into()) }
    }
    pub(super) fn list<T>(&mut self, mut read: impl FnMut(&mut Self) -> Result<T, String>) -> Result<Vec<T>, String> {
        let len = self.u32()? as usize;
        (0..len).map(|_| read(self)).collect()
    }
    pub(super) fn finish(self) -> Result<(), String> {
        if self.cursor == self.bytes.len() { Ok(()) } else { Err("trailing residual data".into()) }
    }
}
