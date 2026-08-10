//! Machine-sized runtime values for the residual kernel.

use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use crate::ops::{Builtin, Constant, HostCapabilityRef, Op, RealmId};

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
