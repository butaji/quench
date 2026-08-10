//! Machine-sized runtime values for the residual kernel.

use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use crate::ops::{
    Builtin, Constant, FunctionKind, FunctionStrictness, HostCapabilityRef, Op, RealmId,
};

/// Identity-bearing host capability kept outside the JavaScript value space.
#[derive(Clone, Debug)]
pub struct HostCapabilityValue {
    pub descriptor: HostCapabilityRef,
    identity: Rc<()>,
}

impl HostCapabilityValue {
    pub fn new(descriptor: HostCapabilityRef) -> Self {
        Self {
            descriptor,
            identity: Rc::new(()),
        }
    }

    pub fn realm(&self) -> RealmId {
        self.descriptor.realm
    }

    pub fn same_realm(&self, other: &Self) -> bool {
        self.realm() == other.realm()
    }

    pub fn same_identity(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.identity, &other.identity)
    }
}

impl PartialEq for HostCapabilityValue {
    fn eq(&self, other: &Self) -> bool {
        self.descriptor == other.descriptor && self.same_identity(other)
    }
}

impl Eq for HostCapabilityValue {}

/// Promise state: pending, fulfilled, or rejected.
#[derive(Debug, Clone, PartialEq)]
pub enum PromiseState {
    Pending,
    Fulfilled(Value),
    Rejected(Value),
}

/// Heap-allocated Promise data.
#[derive(Debug, Clone, PartialEq)]
pub struct PromiseData {
    pub state: RefCell<PromiseState>,
    pub result: RefCell<Option<Value>>,
    pub then_actions: RefCell<Vec<(Option<Value>, Option<Value>)>>,
}

impl PromiseData {
    pub fn new(state: PromiseState) -> Self {
        let result = match &state {
            PromiseState::Pending => None,
            PromiseState::Fulfilled(value) | PromiseState::Rejected(value) => Some(value.clone()),
        };
        Self {
            state: RefCell::new(state),
            result: RefCell::new(result),
            then_actions: RefCell::new(Vec::new()),
        }
    }
}

impl Default for PromiseData {
    fn default() -> Self {
        Self::new(PromiseState::Pending)
    }
}

/// Map key-value storage.
#[derive(Debug, Clone, PartialEq)]
pub struct MapData {
    pub keys: VecDeque<Value>,
    pub values: Vec<Value>,
}

/// Set value storage.
#[derive(Debug, Clone, PartialEq)]
pub struct SetData {
    pub values: VecDeque<Value>,
}

/// Shared byte storage for ArrayBuffer and typed-array views.
#[derive(Debug, Clone, PartialEq)]
pub struct ArrayBufferData {
    pub bytes: Rc<RefCell<Vec<u8>>>,
    pub detached: Rc<RefCell<bool>>,
}

impl ArrayBufferData {
    pub fn new(byte_length: usize) -> Self {
        Self {
            bytes: Rc::new(RefCell::new(vec![0; byte_length])),
            detached: Rc::new(RefCell::new(false)),
        }
    }

    pub fn byte_length(&self) -> usize {
        if *self.detached.borrow() {
            0
        } else {
            self.bytes.borrow().len()
        }
    }

    pub fn detach(&self) {
        *self.detached.borrow_mut() = true;
        self.bytes.borrow_mut().clear();
    }
}

/// A DataView over shared ArrayBuffer bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct DataViewData {
    pub buffer: Rc<ArrayBufferData>,
    pub byte_offset: usize,
    pub byte_length: usize,
}

impl DataViewData {
    pub fn new(buffer: Rc<ArrayBufferData>, byte_offset: usize, byte_length: usize) -> Self {
        Self {
            buffer,
            byte_offset,
            byte_length,
        }
    }

    pub fn byte_length(&self) -> usize {
        if self.buffer.byte_length() < self.byte_offset + self.byte_length {
            0
        } else {
            self.byte_length
        }
    }

    pub fn is_detached(&self) -> bool {
        *self.buffer.detached.borrow()
    }

    fn range(&self, offset: usize, width: usize) -> Result<usize, DataViewError> {
        if self.is_detached() {
            return Err(DataViewError::Detached);
        }
        let end = offset
            .checked_add(width)
            .ok_or(DataViewError::OutOfBounds)?;
        if end > self.byte_length {
            return Err(DataViewError::OutOfBounds);
        }
        self.byte_offset
            .checked_add(offset)
            .ok_or(DataViewError::OutOfBounds)
    }

    fn read<const N: usize>(&self, offset: usize) -> Result<[u8; N], DataViewError> {
        let start = self.range(offset, N)?;
        let end = start.checked_add(N).ok_or(DataViewError::OutOfBounds)?;
        self.buffer
            .bytes
            .borrow()
            .get(start..end)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(DataViewError::OutOfBounds)
    }

    fn write<const N: usize>(&self, offset: usize, bytes: [u8; N]) -> Result<(), DataViewError> {
        let start = self.range(offset, N)?;
        let end = start.checked_add(N).ok_or(DataViewError::OutOfBounds)?;
        let mut storage = self.buffer.bytes.borrow_mut();
        let destination = storage
            .get_mut(start..end)
            .ok_or(DataViewError::OutOfBounds)?;
        destination.copy_from_slice(&bytes);
        Ok(())
    }

    pub fn get_int8(&self, offset: usize) -> Result<i8, DataViewError> {
        Ok(i8::from_ne_bytes(self.read::<1>(offset)?))
    }

    pub fn get_uint8(&self, offset: usize) -> Result<u8, DataViewError> {
        Ok(self.read::<1>(offset)?[0])
    }

    pub fn get_int16(&self, offset: usize, little_endian: bool) -> Result<i16, DataViewError> {
        Ok(decode::<i16, 2>(self.read::<2>(offset)?, little_endian))
    }

    pub fn get_uint16(&self, offset: usize, little_endian: bool) -> Result<u16, DataViewError> {
        Ok(decode::<u16, 2>(self.read::<2>(offset)?, little_endian))
    }

    pub fn get_int32(&self, offset: usize, little_endian: bool) -> Result<i32, DataViewError> {
        Ok(decode::<i32, 4>(self.read::<4>(offset)?, little_endian))
    }

    pub fn get_uint32(&self, offset: usize, little_endian: bool) -> Result<u32, DataViewError> {
        Ok(decode::<u32, 4>(self.read::<4>(offset)?, little_endian))
    }

    pub fn get_float32(&self, offset: usize, little_endian: bool) -> Result<f32, DataViewError> {
        Ok(decode::<f32, 4>(self.read::<4>(offset)?, little_endian))
    }

    pub fn get_float64(&self, offset: usize, little_endian: bool) -> Result<f64, DataViewError> {
        Ok(decode::<f64, 8>(self.read::<8>(offset)?, little_endian))
    }

    pub fn get_float16(&self, offset: usize, little_endian: bool) -> Result<f64, DataViewError> {
        Ok(half_to_f64(decode::<u16, 2>(
            self.read::<2>(offset)?,
            little_endian,
        )))
    }

    pub fn set_int8(&self, offset: usize, value: i8) -> Result<(), DataViewError> {
        self.write(offset, value.to_ne_bytes())
    }

    pub fn set_uint8(&self, offset: usize, value: u8) -> Result<(), DataViewError> {
        self.write(offset, [value])
    }

    pub fn set_int16(
        &self,
        offset: usize,
        value: i16,
        little_endian: bool,
    ) -> Result<(), DataViewError> {
        encode::<i16, 2>(self, offset, value, little_endian)
    }

    pub fn set_uint16(
        &self,
        offset: usize,
        value: u16,
        little_endian: bool,
    ) -> Result<(), DataViewError> {
        encode::<u16, 2>(self, offset, value, little_endian)
    }

    pub fn set_int32(
        &self,
        offset: usize,
        value: i32,
        little_endian: bool,
    ) -> Result<(), DataViewError> {
        encode::<i32, 4>(self, offset, value, little_endian)
    }

    pub fn set_uint32(
        &self,
        offset: usize,
        value: u32,
        little_endian: bool,
    ) -> Result<(), DataViewError> {
        encode::<u32, 4>(self, offset, value, little_endian)
    }

    pub fn set_float32(
        &self,
        offset: usize,
        value: f32,
        little_endian: bool,
    ) -> Result<(), DataViewError> {
        encode::<f32, 4>(self, offset, value, little_endian)
    }

    pub fn set_float64(
        &self,
        offset: usize,
        value: f64,
        little_endian: bool,
    ) -> Result<(), DataViewError> {
        encode::<f64, 8>(self, offset, value, little_endian)
    }

    pub fn set_float16(
        &self,
        offset: usize,
        value: f64,
        little_endian: bool,
    ) -> Result<(), DataViewError> {
        encode::<u16, 2>(self, offset, f64_to_half(value), little_endian)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataViewError {
    Detached,
    OutOfBounds,
}

fn decode<T, const N: usize>(bytes: [u8; N], little_endian: bool) -> T
where
    T: DataViewDecode<N>,
{
    T::decode(bytes, little_endian)
}

fn encode<T, const N: usize>(
    view: &DataViewData,
    offset: usize,
    value: T,
    little_endian: bool,
) -> Result<(), DataViewError>
where
    T: DataViewEncode<N>,
{
    view.write(offset, value.encode(little_endian))
}

trait DataViewDecode<const N: usize> {
    fn decode(bytes: [u8; N], little_endian: bool) -> Self;
}

trait DataViewEncode<const N: usize> {
    fn encode(self, little_endian: bool) -> [u8; N];
}

macro_rules! data_view_codec {
    ($type:ty, $size:literal) => {
        impl DataViewDecode<$size> for $type {
            fn decode(bytes: [u8; $size], little_endian: bool) -> Self {
                if little_endian {
                    <$type>::from_le_bytes(bytes)
                } else {
                    <$type>::from_be_bytes(bytes)
                }
            }
        }

        impl DataViewEncode<$size> for $type {
            fn encode(self, little_endian: bool) -> [u8; $size] {
                if little_endian {
                    self.to_le_bytes()
                } else {
                    self.to_be_bytes()
                }
            }
        }
    };
}

data_view_codec!(i16, 2);
data_view_codec!(u16, 2);
data_view_codec!(i32, 4);
data_view_codec!(u32, 4);
data_view_codec!(f32, 4);
data_view_codec!(f64, 8);

fn half_to_f64(bits: u16) -> f64 {
    let sign = ((bits & 0x8000) as u64) << 48;
    let exponent = (bits >> 10) & 0x1f;
    let fraction = bits & 0x03ff;
    match (exponent, fraction) {
        (0, 0) => f64::from_bits(sign),
        (0, fraction) => (fraction as f64 * 2f64.powi(-24)) * sign_factor(sign),
        (0x1f, 0) => f64::from_bits(sign | 0x7ff0_0000_0000_0000),
        (0x1f, fraction) => f64::from_bits(sign | 0x7ff0_0000_0000_0000 | (fraction as u64) << 42),
        (exponent, fraction) => {
            let value = (1.0 + fraction as f64 / 1024.0) * 2f64.powi(exponent as i32 - 15);
            value * sign_factor(sign)
        }
    }
}

fn f64_to_half(value: f64) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 63) as u16) << 15;
    let exponent = ((bits >> 52) & 0x7ff) as u16;
    let fraction = bits & 0x000f_ffff_ffff_ffff;
    if exponent == 0x7ff {
        return sign
            | if fraction == 0 {
                0x7c00
            } else {
                0x7c00 | ((fraction >> 42) as u16).max(1)
            };
    }
    let absolute = f64::from_bits(bits & 0x7fff_ffff_ffff_ffff);
    if absolute < 2f64.powi(-14) {
        let rounded = round_half(absolute * 2f64.powi(24));
        return sign | if rounded >= 0x0400 { 0x0400 } else { rounded };
    }
    let unbiased = exponent as i32 - 1023;
    if unbiased > 15 {
        return sign | 0x7c00;
    }
    let mut significand = (fraction >> 42) as u16;
    let remainder = fraction & ((1u64 << 42) - 1);
    if remainder > (1u64 << 41) || (remainder == (1u64 << 41) && significand & 1 != 0) {
        significand += 1;
    }
    let half_exponent = (unbiased + 15) as u16;
    if significand == 0x0400 {
        return sign | ((half_exponent + 1) << 10);
    }
    sign | (half_exponent << 10) | significand
}

fn round_half(value: f64) -> u16 {
    let lower = value.floor() as u64;
    let fraction = value - lower as f64;
    let rounded = lower + u64::from(fraction > 0.5 || (fraction == 0.5 && lower & 1 != 0));
    rounded as u16
}

fn sign_factor(sign: u64) -> f64 {
    if sign == 0 {
        1.0
    } else {
        -1.0
    }
}

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

/// A Proxy value wrapping a target and handler.
#[derive(Debug, Clone, PartialEq)]
pub struct ProxyValue {
    pub target: Value,
    pub handler: Value,
    pub revoked: Rc<RefCell<bool>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Boolean(bool),
    String(String),
    BigInt(String),
    Array(Rc<Vec<Value>>),
    Object(Rc<Vec<(String, Value)>>),
    ArrayBuffer(Rc<ArrayBufferData>),
    Float64Array(Rc<Float64ArrayData>),
    Float32Array(Rc<Float32ArrayData>),
    Int8Array(Rc<Int8ArrayData>),
    Int16Array(Rc<Int16ArrayData>),
    Int32Array(Rc<Int32ArrayData>),
    Uint32Array(Rc<Uint32ArrayData>),
    Uint8Array(Rc<Uint8ArrayData>),
    Uint8ClampedArray(Rc<Uint8ClampedArrayData>),
    Uint16Array(Rc<Uint16ArrayData>),
    DataView(Rc<DataViewData>),
    Builtin(Builtin),
    Function(Rc<FunctionValue>),
    BoundFunction(Rc<BoundFunctionValue>),
    Proxy(Rc<ProxyValue>),
    Promise(Rc<PromiseData>),
    Map(Rc<MapData>),
    Set(Rc<SetData>),
    Null,
    Undefined,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionValue {
    pub body: Vec<Op>,
    pub params: u16,
    pub captures: Rc<RefCell<Vec<Value>>>,
    pub properties: Rc<RefCell<Vec<(String, Value)>>>,
    pub kind: FunctionKind,
    pub strictness: FunctionStrictness,
    /// Whether invocation produces an async completion and Promise result.
    pub is_async: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoundFunctionValue {
    pub target: Value,
    pub receiver: Value,
    pub arguments: Vec<Value>,
}

impl From<&Constant> for Value {
    fn from(value: &Constant) -> Self {
        match value {
            Constant::Number(value) => Self::Number(*value),
            Constant::Boolean(value) => Self::Boolean(*value),
            Constant::String(value) => Self::String(value.clone()),
            Constant::BigInt(value) => Self::BigInt(value.clone()),
            Constant::Null => Self::Null,
            Constant::Undefined => Self::Undefined,
        }
    }
}
