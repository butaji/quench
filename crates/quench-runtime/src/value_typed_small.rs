/// A Float64Array view over shared ArrayBuffer bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Float64ArrayData {
    pub buffer: Rc<ArrayBufferData>,
    pub byte_offset: usize,
    pub length: usize,
}

/// A Float32Array view over shared ArrayBuffer bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Float32ArrayData {
    pub buffer: Rc<ArrayBufferData>,
    pub byte_offset: usize,
    pub length: usize,
}

impl Float32ArrayData {
    pub const BYTES_PER_ELEMENT: usize = std::mem::size_of::<f32>();

    pub fn new(buffer: Rc<ArrayBufferData>, byte_offset: usize, length: usize) -> Self {
        Self {
            buffer,
            byte_offset,
            length,
        }
    }

    pub fn byte_length(&self) -> usize {
        self.length * Self::BYTES_PER_ELEMENT
    }

    pub fn get(&self, index: usize) -> Option<f32> {
        if index >= self.length || self.buffer.byte_length() < self.byte_offset + self.byte_length()
        {
            return None;
        }
        let start = self.byte_offset + index * Self::BYTES_PER_ELEMENT;
        let end = start + Self::BYTES_PER_ELEMENT;
        let bytes = self.buffer.bytes.borrow();
        let data: [u8; Self::BYTES_PER_ELEMENT] = bytes.get(start..end)?.try_into().ok()?;
        Some(f32::from_ne_bytes(data))
    }

    pub fn set(&self, index: usize, value: f32) -> bool {
        if index >= self.length || self.buffer.byte_length() < self.byte_offset + self.byte_length()
        {
            return false;
        }
        let start = self.byte_offset + index * Self::BYTES_PER_ELEMENT;
        let end = start + Self::BYTES_PER_ELEMENT;
        let mut bytes = self.buffer.bytes.borrow_mut();
        let Some(destination) = bytes.get_mut(start..end) else {
            return false;
        };
        destination.copy_from_slice(&value.to_ne_bytes());
        true
    }
}

impl Float64ArrayData {
    pub const BYTES_PER_ELEMENT: usize = std::mem::size_of::<f64>();

    pub fn new(buffer: Rc<ArrayBufferData>, byte_offset: usize, length: usize) -> Self {
        Self {
            buffer,
            byte_offset,
            length,
        }
    }

    pub fn byte_length(&self) -> usize {
        self.length * Self::BYTES_PER_ELEMENT
    }

    pub fn get(&self, index: usize) -> Option<f64> {
        if index >= self.length || self.buffer.byte_length() < self.byte_offset + self.byte_length()
        {
            return None;
        }
        let start = self.byte_offset + index * Self::BYTES_PER_ELEMENT;
        let end = start + Self::BYTES_PER_ELEMENT;
        let bytes = self.buffer.bytes.borrow();
        let data: [u8; Self::BYTES_PER_ELEMENT] = bytes.get(start..end)?.try_into().ok()?;
        Some(f64::from_ne_bytes(data))
    }

    pub fn set(&self, index: usize, value: f64) -> bool {
        if index >= self.length || self.buffer.byte_length() < self.byte_offset + self.byte_length()
        {
            return false;
        }
        let start = self.byte_offset + index * Self::BYTES_PER_ELEMENT;
        let end = start + Self::BYTES_PER_ELEMENT;
        let mut bytes = self.buffer.bytes.borrow_mut();
        let Some(destination) = bytes.get_mut(start..end) else {
            return false;
        };
        destination.copy_from_slice(&value.to_ne_bytes());
        true
    }
}

/// A signed 8-bit integer view over shared ArrayBuffer bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Int8ArrayData {
    pub buffer: Rc<ArrayBufferData>,
    pub byte_offset: usize,
    pub length: usize,
}

/// A signed 16-bit integer view over shared ArrayBuffer bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Int16ArrayData {
    pub buffer: Rc<ArrayBufferData>,
    pub byte_offset: usize,
    pub length: usize,
}

impl Int16ArrayData {
    pub const BYTES_PER_ELEMENT: usize = std::mem::size_of::<i16>();

    pub fn new(buffer: Rc<ArrayBufferData>, byte_offset: usize, length: usize) -> Self {
        Self {
            buffer,
            byte_offset,
            length,
        }
    }

    pub fn byte_length(&self) -> usize {
        self.length * Self::BYTES_PER_ELEMENT
    }

    pub fn get(&self, index: usize) -> Option<i16> {
        if index >= self.length || self.is_out_of_bounds() {
            return None;
        }
        let start = self.byte_offset + index * Self::BYTES_PER_ELEMENT;
        let end = start + Self::BYTES_PER_ELEMENT;
        let bytes = self.buffer.bytes.borrow();
        let data: [u8; Self::BYTES_PER_ELEMENT] = bytes.get(start..end)?.try_into().ok()?;
        Some(i16::from_ne_bytes(data))
    }

    pub fn set(&self, index: usize, value: i16) -> bool {
        if index >= self.length || self.is_out_of_bounds() {
            return false;
        }
        let start = self.byte_offset + index * Self::BYTES_PER_ELEMENT;
        let end = start + Self::BYTES_PER_ELEMENT;
        let mut bytes = self.buffer.bytes.borrow_mut();
        let Some(destination) = bytes.get_mut(start..end) else {
            return false;
        };
        destination.copy_from_slice(&value.to_ne_bytes());
        true
    }

    fn is_out_of_bounds(&self) -> bool {
        self.buffer.byte_length() < self.byte_offset + self.byte_length()
    }
}

/// An unsigned 16-bit integer view over shared ArrayBuffer bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Uint16ArrayData {
    pub buffer: Rc<ArrayBufferData>,
    pub byte_offset: usize,
    pub length: usize,
}

impl Uint16ArrayData {
    pub const BYTES_PER_ELEMENT: usize = std::mem::size_of::<u16>();

    pub fn new(buffer: Rc<ArrayBufferData>, byte_offset: usize, length: usize) -> Self {
        Self {
            buffer,
            byte_offset,
            length,
        }
    }

    pub fn byte_length(&self) -> usize {
        self.length * Self::BYTES_PER_ELEMENT
    }

    pub fn get(&self, index: usize) -> Option<u16> {
        if index >= self.length || self.is_out_of_bounds() {
            return None;
        }
        let start = self.byte_offset + index * Self::BYTES_PER_ELEMENT;
        let end = start + Self::BYTES_PER_ELEMENT;
        let bytes = self.buffer.bytes.borrow();
        let data: [u8; Self::BYTES_PER_ELEMENT] = bytes.get(start..end)?.try_into().ok()?;
        Some(u16::from_ne_bytes(data))
    }

    pub fn set(&self, index: usize, value: u16) -> bool {
        if index >= self.length || self.is_out_of_bounds() {
            return false;
        }
        let start = self.byte_offset + index * Self::BYTES_PER_ELEMENT;
        let end = start + Self::BYTES_PER_ELEMENT;
        let mut bytes = self.buffer.bytes.borrow_mut();
        let Some(destination) = bytes.get_mut(start..end) else {
            return false;
        };
        destination.copy_from_slice(&value.to_ne_bytes());
        true
    }

    fn is_out_of_bounds(&self) -> bool {
        self.buffer.byte_length() < self.byte_offset + self.byte_length()
    }
}

impl Int8ArrayData {
    pub const BYTES_PER_ELEMENT: usize = std::mem::size_of::<i8>();

    pub fn new(buffer: Rc<ArrayBufferData>, byte_offset: usize, length: usize) -> Self {
        Self {
            buffer,
            byte_offset,
            length,
        }
    }

    pub fn byte_length(&self) -> usize {
        self.length * Self::BYTES_PER_ELEMENT
    }

    pub fn get(&self, index: usize) -> Option<i8> {
        if index >= self.length || self.is_out_of_bounds() {
            return None;
        }
        let offset = self.byte_offset + index;
        self.buffer
            .bytes
            .borrow()
            .get(offset)
            .copied()
            .map(|byte| i8::from_ne_bytes([byte]))
    }

    pub fn set(&self, index: usize, value: i8) -> bool {
        if index >= self.length || self.is_out_of_bounds() {
            return false;
        }
        let offset = self.byte_offset + index;
        let mut bytes = self.buffer.bytes.borrow_mut();
        let Some(byte) = bytes.get_mut(offset) else {
            return false;
        };
        *byte = value.to_ne_bytes()[0];
        true
    }

    fn is_out_of_bounds(&self) -> bool {
        self.buffer.byte_length() < self.byte_offset + self.byte_length()
    }
}

