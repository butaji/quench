/// A signed 32-bit integer view over shared ArrayBuffer bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Int32ArrayData {
    pub buffer: Rc<ArrayBufferData>,
    pub byte_offset: usize,
    pub length: usize,
}

impl Int32ArrayData {
    pub const BYTES_PER_ELEMENT: usize = std::mem::size_of::<i32>();

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

    pub fn get(&self, index: usize) -> Option<i32> {
        if index >= self.length || self.is_out_of_bounds() {
            return None;
        }
        let start = self.byte_offset + index * Self::BYTES_PER_ELEMENT;
        let end = start + Self::BYTES_PER_ELEMENT;
        let bytes = self.buffer.bytes.borrow();
        let data: [u8; Self::BYTES_PER_ELEMENT] = bytes.get(start..end)?.try_into().ok()?;
        Some(i32::from_ne_bytes(data))
    }

    pub fn set(&self, index: usize, value: i32) -> bool {
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

/// An unsigned 32-bit integer view over shared ArrayBuffer bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Uint32ArrayData {
    pub buffer: Rc<ArrayBufferData>,
    pub byte_offset: usize,
    pub length: usize,
}

impl Uint32ArrayData {
    pub const BYTES_PER_ELEMENT: usize = std::mem::size_of::<u32>();

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

    pub fn get(&self, index: usize) -> Option<u32> {
        if index >= self.length || self.is_out_of_bounds() {
            return None;
        }
        let start = self.byte_offset + index * Self::BYTES_PER_ELEMENT;
        let end = start + Self::BYTES_PER_ELEMENT;
        let bytes = self.buffer.bytes.borrow();
        let data: [u8; Self::BYTES_PER_ELEMENT] = bytes.get(start..end)?.try_into().ok()?;
        Some(u32::from_ne_bytes(data))
    }

    pub fn set(&self, index: usize, value: u32) -> bool {
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

/// An unsigned 8-bit integer view over shared ArrayBuffer bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Uint8ArrayData {
    pub buffer: Rc<ArrayBufferData>,
    pub byte_offset: usize,
    pub length: usize,
}

/// An unsigned 8-bit clamped view over shared ArrayBuffer bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Uint8ClampedArrayData {
    pub buffer: Rc<ArrayBufferData>,
    pub byte_offset: usize,
    pub length: usize,
}

impl Uint8ClampedArrayData {
    pub const BYTES_PER_ELEMENT: usize = std::mem::size_of::<u8>();

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

    pub fn get(&self, index: usize) -> Option<u8> {
        if index >= self.length || self.is_out_of_bounds() {
            return None;
        }
        let offset = self.byte_offset + index;
        self.buffer.bytes.borrow().get(offset).copied()
    }

    pub fn set(&self, index: usize, value: f64) -> bool {
        if index >= self.length || self.is_out_of_bounds() {
            return false;
        }
        let offset = self.byte_offset + index;
        let mut bytes = self.buffer.bytes.borrow_mut();
        let Some(byte) = bytes.get_mut(offset) else {
            return false;
        };
        *byte = to_uint8_clamp(value);
        true
    }

    fn is_out_of_bounds(&self) -> bool {
        self.buffer.byte_length() < self.byte_offset + self.byte_length()
    }
}

/// Implements ECMAScript's ToUint8Clamp conversion, including ties-to-even.
pub fn to_uint8_clamp(value: f64) -> u8 {
    if value.is_nan() || value <= 0.0 {
        return 0;
    }
    if value >= 255.0 {
        return 255;
    }
    let floor = value.floor();
    let fraction = value - floor;
    let rounded = if fraction < 0.5 {
        floor
    } else if fraction > 0.5 || floor % 2.0 != 0.0 {
        floor + 1.0
    } else {
        floor
    };
    rounded as u8
}

impl Uint8ArrayData {
    pub const BYTES_PER_ELEMENT: usize = std::mem::size_of::<u8>();

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

    pub fn get(&self, index: usize) -> Option<u8> {
        if index >= self.length || self.is_out_of_bounds() {
            return None;
        }
        let offset = self.byte_offset + index;
        self.buffer.bytes.borrow().get(offset).copied()
    }

    pub fn set(&self, index: usize, value: u8) -> bool {
        if index >= self.length || self.is_out_of_bounds() {
            return false;
        }
        let offset = self.byte_offset + index;
        let mut bytes = self.buffer.bytes.borrow_mut();
        let Some(byte) = bytes.get_mut(offset) else {
            return false;
        };
        *byte = value;
        true
    }

    fn is_out_of_bounds(&self) -> bool {
        self.buffer.byte_length() < self.byte_offset + self.byte_length()
    }
}

