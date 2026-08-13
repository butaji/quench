/// Shared byte storage for ArrayBuffer and typed-array views.
#[derive(Debug, Clone, PartialEq)]
pub struct ArrayBufferData {
    pub shared: bool,
    pub bytes: Rc<RefCell<Vec<u8>>>,
    pub detached: Rc<RefCell<bool>>,
    pub max_byte_length: Option<usize>,
    pub immutable: bool,
    prototype: RefCell<Option<Value>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizeError;

impl ArrayBufferData {
    pub fn new(byte_length: usize) -> Self {
        Self {
            shared: false,
            bytes: Rc::new(RefCell::new(vec![0; byte_length])),
            detached: Rc::new(RefCell::new(false)),
            max_byte_length: None,
            immutable: false,
            prototype: RefCell::new(None),
        }
    }

    pub fn new_resizable(byte_length: usize, max_byte_length: usize) -> Self {
        let mut buffer = Self::new(byte_length);
        buffer.max_byte_length = Some(max_byte_length);
        buffer
    }

    pub fn byte_length(&self) -> usize {
        if *self.detached.borrow() {
            0
        } else {
            self.bytes.borrow().len()
        }
    }

    pub(crate) fn prototype(&self) -> Option<Value> {
        self.prototype.borrow().clone()
    }

    pub(crate) fn set_prototype(&self, prototype: Value) {
        self.prototype.replace(Some(prototype));
    }

    pub fn detach(&self) {
        *self.detached.borrow_mut() = true;
        self.bytes.borrow_mut().clear();
    }

    pub fn resize(&self, byte_length: usize) -> Result<(), ResizeError> {
        let exceeds_maximum = self.max_byte_length.map_or(true, |max| byte_length > max);
        if *self.detached.borrow() || self.immutable || exceeds_maximum {
            return Err(ResizeError);
        }
        self.bytes.borrow_mut().resize(byte_length, 0);
        Ok(())
    }

    pub fn transfer_to_immutable(&self) -> ArrayBufferData {
        let bytes = std::mem::take(&mut *self.bytes.borrow_mut());
        *self.detached.borrow_mut() = true;
        let mut buffer = ArrayBufferData::new(0);
        buffer.bytes = Rc::new(RefCell::new(bytes));
        buffer.immutable = true;
        buffer
    }
}

/// A DataView over shared ArrayBuffer bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct DataViewData {
    pub buffer: Rc<ArrayBufferData>,
    pub byte_offset: usize,
    pub byte_length: usize,
    prototype: RefCell<Option<Value>>,
}

impl DataViewData {
    pub fn new(buffer: Rc<ArrayBufferData>, byte_offset: usize, byte_length: usize) -> Self {
        Self {
            buffer,
            byte_offset,
            byte_length,
            prototype: RefCell::new(None),
        }
    }

    pub fn byte_length(&self) -> usize {
        if self.is_length_tracking() {
            return self.buffer.byte_length().saturating_sub(self.byte_offset);
        }
        if self.buffer.byte_length() < self.byte_offset + self.byte_length {
            0
        } else {
            self.byte_length
        }
    }

    pub fn is_length_tracking(&self) -> bool {
        self.byte_length == usize::MAX
    }

    pub fn is_out_of_bounds(&self) -> bool {
        let required = if self.is_length_tracking() {
            self.byte_offset
        } else {
            self.byte_offset.saturating_add(self.byte_length)
        };
        self.buffer.byte_length() < required
    }

    pub(crate) fn prototype(&self) -> Option<Value> {
        self.prototype.borrow().clone()
    }

    pub(crate) fn set_prototype(&self, prototype: Value) {
        self.prototype.replace(Some(prototype));
    }

    pub fn is_detached(&self) -> bool {
        *self.buffer.detached.borrow()
    }

    fn range(&self, offset: usize, width: usize) -> Result<usize, DataViewError> {
        if self.is_detached() {
            return Err(DataViewError::Detached);
        }
        if self.is_out_of_bounds() {
            return Err(DataViewError::ViewOutOfBounds);
        }
        let end = offset
            .checked_add(width)
            .ok_or(DataViewError::OutOfBounds)?;
        if end > self.byte_length() {
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

    pub fn get_bigint64(&self, offset: usize, little_endian: bool) -> Result<i64, DataViewError> {
        Ok(decode::<i64, 8>(self.read::<8>(offset)?, little_endian))
    }

    pub fn get_biguint64(&self, offset: usize, little_endian: bool) -> Result<u64, DataViewError> {
        Ok(decode::<u64, 8>(self.read::<8>(offset)?, little_endian))
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

    pub fn set_bigint64(
        &self,
        offset: usize,
        value: i64,
        little_endian: bool,
    ) -> Result<(), DataViewError> {
        encode::<i64, 8>(self, offset, value, little_endian)
    }

    pub fn set_biguint64(
        &self,
        offset: usize,
        value: u64,
        little_endian: bool,
    ) -> Result<(), DataViewError> {
        encode::<u64, 8>(self, offset, value, little_endian)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataViewError {
    Detached,
    OutOfBounds,
    ViewOutOfBounds,
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
data_view_codec!(i64, 8);
data_view_codec!(u64, 8);
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

pub(crate) fn f16_round(value: f64) -> f64 {
    half_to_f64(f64_to_half(value))
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
