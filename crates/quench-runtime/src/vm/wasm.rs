use super::*;

pub(super) struct StoredWasmModule {
    program: ProgramId,
    pub(super) signatures: Vec<crate::WasmSignature>,
    pub(super) globals: Vec<crate::WasmGlobalId>,
    memories: Vec<usize>,
    pub(super) tables: Vec<crate::WasmTableId>,
    pub(super) type_signatures: Vec<crate::WasmSignature>,
    pub(super) gc_supertypes: Vec<Option<u32>>,
    pub(super) gc_type_canonicals: Vec<u32>,
    pub(super) gc_type_fingerprints: Vec<String>,
    pub(super) gc_descriptors: Vec<crate::WasmGcDescriptor>,
    pub(super) function_type_indices: Vec<u32>,
    pub(super) gc_types: Vec<Option<crate::WasmGcType>>,
    pub(super) tag_signatures: Vec<crate::WasmSignature>,
    pub(super) tags: Vec<crate::WasmTagId>,
    pub(super) exception_handlers: Vec<Vec<crate::wasm::WasmExceptionHandler>>,
    pub(super) indirect_sites: Vec<crate::wasm::WasmIndirectSite>,
    v128_sites: Vec<crate::wasm::WasmV128Site>,
    atomic_sites: Vec<crate::wasm::WasmAtomicSite>,
    pub(super) imported_functions: Vec<Option<crate::WasmFunctionRef>>,
    pub(super) element_segments: Vec<crate::WasmElementSegment>,
    pub(super) data_segments: Vec<Option<Vec<u8>>>,
}

pub(super) struct WasmTableState {
    size: u64,
    maximum_size: Option<u64>,
    table64: bool,
    default: crate::WasmValue,
    pub(super) element_type: crate::WasmType,
    elements: rustc_hash::FxHashMap<u64, crate::WasmValue>,
    growth_defaults: Vec<(u64, crate::WasmValue)>,
}

pub(super) enum WasmGcObject {
    Array {
        module_index: u32,
        type_index: u32,
        values: Vec<Value>,
    },
    Struct {
        module_index: u32,
        type_index: u32,
        values: Vec<Value>,
        descriptor: Value,
    },
}

fn remap_gc_constant(
    value: crate::WasmValue,
    references: &rustc_hash::FxHashMap<u32, u32>,
) -> crate::WasmValue {
    match value {
        crate::WasmValue::FuncRef(Some(reference)) => references
            .get(&reference)
            .copied()
            .map_or(value, |reference| {
                crate::WasmValue::FuncRef(Some(reference))
            }),
        _ => value,
    }
}

fn normalize_gc_constant(
    value: crate::WasmValue,
    field: crate::WasmGcField,
) -> Result<crate::WasmValue, JsError> {
    let Some(bits) = field.packed_bits else {
        return Ok(value);
    };
    let crate::WasmValue::I32(value) = value else {
        return Err(JsError::validation(
            "invalid packed Wasm GC initializer".into(),
        ));
    };
    let mask = (1u32 << bits) - 1;
    Ok(crate::WasmValue::I32((value as u32 & mask) as i32))
}

fn remap_gc_table_values(
    table: &crate::WasmTableInit,
    references: &rustc_hash::FxHashMap<u32, u32>,
) -> crate::WasmTableInit {
    let mut table = table.clone();
    table.initial_value = remap_gc_constant(table.initial_value, references);
    for (_, values) in &mut table.elements {
        values
            .iter_mut()
            .for_each(|value| *value = remap_gc_constant(*value, references));
    }
    table
}

impl WasmTableState {
    fn new(init: &crate::WasmTableInit) -> Result<Self, JsError> {
        let mut table = Self {
            size: init.initial_size,
            maximum_size: init.maximum_size,
            table64: init.table64,
            default: init.initial_value,
            element_type: init.element_type,
            elements: rustc_hash::FxHashMap::default(),
            growth_defaults: Vec::new(),
        };
        for (offset, values) in &init.elements {
            let count = u64::try_from(values.len())
                .map_err(|_| JsError::wasm_trap_error(crate::WasmTrap::TableOutOfBounds))?;
            offset
                .checked_add(count)
                .filter(|end| *end <= table.size)
                .ok_or_else(|| JsError::wasm_trap_error(crate::WasmTrap::TableOutOfBounds))?;
            for (index, value) in values.iter().copied().enumerate() {
                table.elements.insert(*offset + index as u64, value);
            }
        }
        Ok(table)
    }

    fn apply_elements(&mut self, init: &crate::WasmTableInit) -> Result<(), JsError> {
        if self.table64 != init.table64
            || !wasm_reference_type_assignable(init.element_type, self.element_type)
        {
            return Err(JsError::validation("incompatible Wasm table type".into()));
        }
        for (offset, values) in &init.elements {
            let count = u64::try_from(values.len())
                .map_err(|_| JsError::wasm_trap_error(crate::WasmTrap::TableOutOfBounds))?;
            self.check_range(*offset, count)?;
            for (index, value) in values.iter().copied().enumerate() {
                self.elements.insert(*offset + index as u64, value);
            }
        }
        Ok(())
    }

    pub(super) fn get(&self, index: u64) -> Option<crate::WasmValue> {
        (index < self.size).then(|| {
            self.elements.get(&index).copied().unwrap_or_else(|| {
                self.growth_defaults
                    .iter()
                    .rev()
                    .find_map(|(start, value)| (index >= *start).then_some(*value))
                    .unwrap_or(self.default)
            })
        })
    }

    pub(super) fn set(&mut self, index: u64, value: crate::WasmValue) -> Result<(), JsError> {
        if index >= self.size {
            return Err(JsError::wasm_trap_error(crate::WasmTrap::TableOutOfBounds));
        }
        self.elements.insert(index, value);
        Ok(())
    }

    fn check_range(&self, start: u64, length: u64) -> Result<(), JsError> {
        start
            .checked_add(length)
            .filter(|end| *end <= self.size)
            .ok_or_else(|| JsError::wasm_trap_error(crate::WasmTrap::TableOutOfBounds))?;
        Ok(())
    }

    fn grow(&mut self, delta: u64, initial: crate::WasmValue) -> u64 {
        let previous = self.size;
        let Some(new_size) = previous.checked_add(delta) else {
            return u64::MAX;
        };
        let address_limit = if self.table64 {
            u64::MAX
        } else {
            u64::from(u32::MAX)
        };
        if new_size > address_limit || self.maximum_size.is_some_and(|maximum| new_size > maximum) {
            return u64::MAX;
        }
        if delta != 0 {
            self.growth_defaults.push((previous, initial));
            self.size = new_size;
        }
        previous
    }

    fn fill(
        &mut self,
        destination: u64,
        value: crate::WasmValue,
        length: u64,
    ) -> Result<(), JsError> {
        self.check_range(destination, length)?;
        for index in destination..destination + length {
            self.elements.insert(index, value);
        }
        Ok(())
    }
}

const WASM_STORAGE_PAGE_SIZE: usize = 65_536;

pub(super) struct WasmMemoryState {
    pages: rustc_hash::FxHashMap<u64, Box<[u8; WASM_STORAGE_PAGE_SIZE]>>,
    page_count: u64,
    maximum_pages: Option<u64>,
    memory64: bool,
    shared: bool,
    page_size_log2: u32,
}

impl WasmMemoryState {
    fn new(init: &crate::WasmMemoryInit) -> Result<Self, JsError> {
        Ok(Self {
            pages: rustc_hash::FxHashMap::default(),
            page_count: init.initial_pages,
            maximum_pages: init.maximum_pages,
            memory64: init.memory64,
            shared: init.shared,
            page_size_log2: init.page_size_log2,
        })
    }

    fn checked_range(&self, start: u64, width: u64) -> Result<std::ops::Range<u128>, JsError> {
        let start = u128::from(start);
        let end = start + u128::from(width);
        let available = u128::from(self.page_count) * self.page_size() as u128;
        if end > available {
            return Err(JsError::wasm_trap_error(crate::WasmTrap::MemoryOutOfBounds));
        }
        Ok(start..end)
    }

    fn read(
        &self,
        address: u64,
        offset: u64,
        kind: crate::WasmMemoryAccessKind,
    ) -> Result<u64, JsError> {
        let start = address
            .checked_add(offset)
            .ok_or_else(|| JsError::wasm_trap_error(crate::WasmTrap::MemoryOutOfBounds))?;
        let range = self.checked_range(start, kind.width() as u64)?;
        let mut raw = 0u64;
        for (index, byte_index) in range.enumerate() {
            let page = (byte_index / WASM_STORAGE_PAGE_SIZE as u128) as u64;
            let offset = (byte_index % WASM_STORAGE_PAGE_SIZE as u128) as usize;
            let byte = self.pages.get(&page).map_or(0, |bytes| bytes[offset]);
            raw |= u64::from(byte) << (index * 8);
        }
        Ok(raw)
    }

    fn read_bytes(&self, address: u64, offset: u64, width: usize) -> Result<[u8; 16], JsError> {
        let start = address
            .checked_add(offset)
            .ok_or_else(|| JsError::wasm_trap_error(crate::WasmTrap::MemoryOutOfBounds))?;
        let range = self.checked_range(start, width as u64)?;
        let mut bytes = [0; 16];
        for (index, byte_index) in range.enumerate() {
            bytes[index] = self.read_byte(byte_index);
        }
        Ok(bytes)
    }

    fn read_raw(&self, address: u64, width: u8) -> Result<u64, JsError> {
        let range = self.checked_range(address, u64::from(width))?;
        let mut raw = 0u64;
        for (index, byte_index) in range.enumerate() {
            raw |= u64::from(self.read_byte(byte_index)) << (index * 8);
        }
        Ok(raw)
    }

    fn write_raw(&mut self, address: u64, width: u8, raw: u64) -> Result<(), JsError> {
        self.write_bytes_at(address, usize::from(width), raw)
    }

    fn write(
        &mut self,
        address: u64,
        offset: u64,
        kind: crate::WasmMemoryAccessKind,
        raw: u64,
    ) -> Result<(), JsError> {
        let start = address
            .checked_add(offset)
            .ok_or_else(|| JsError::wasm_trap_error(crate::WasmTrap::MemoryOutOfBounds))?;
        self.write_bytes_at(start, kind.width(), raw)
    }

    fn write_bytes(&mut self, start: u64, bytes: &[u8]) -> Result<(), JsError> {
        self.checked_range(start, bytes.len() as u64)?;
        for (index, byte) in bytes.iter().copied().enumerate() {
            self.write_byte(u128::from(start) + index as u128, byte);
        }
        Ok(())
    }

    fn write_bytes_at(&mut self, start: u64, width: usize, raw: u64) -> Result<(), JsError> {
        self.checked_range(start, width as u64)?;
        for index in 0..width {
            self.write_byte(
                u128::from(start) + index as u128,
                (raw >> (index * 8)) as u8,
            );
        }
        Ok(())
    }

    fn write_byte(&mut self, byte_index: u128, byte: u8) {
        let page = (byte_index / WASM_STORAGE_PAGE_SIZE as u128) as u64;
        let offset = (byte_index % WASM_STORAGE_PAGE_SIZE as u128) as usize;
        let bytes = self
            .pages
            .entry(page)
            .or_insert_with(|| Box::new([0; WASM_STORAGE_PAGE_SIZE]));
        bytes[offset] = byte;
    }

    fn read_byte(&self, byte_index: u128) -> u8 {
        let page = (byte_index / WASM_STORAGE_PAGE_SIZE as u128) as u64;
        let offset = (byte_index % WASM_STORAGE_PAGE_SIZE as u128) as usize;
        self.pages.get(&page).map_or(0, |page| page[offset])
    }

    fn fill(&mut self, destination: u64, value: u8, length: u64) -> Result<(), JsError> {
        let range = self.checked_range(destination, length)?;
        if range.is_empty() {
            return Ok(());
        }
        if value == 0 {
            let first_page = (range.start / WASM_STORAGE_PAGE_SIZE as u128) as u64;
            let last_page = ((range.end - 1) / WASM_STORAGE_PAGE_SIZE as u128) as u64;
            for page_index in first_page..=last_page {
                let page_start = u128::from(page_index) * WASM_STORAGE_PAGE_SIZE as u128;
                let start = (range.start.max(page_start) - page_start) as usize;
                let end = (range.end.min(page_start + WASM_STORAGE_PAGE_SIZE as u128) - page_start)
                    as usize;
                if start == 0 && end == WASM_STORAGE_PAGE_SIZE {
                    self.pages.remove(&page_index);
                } else if let Some(page) = self.pages.get_mut(&page_index) {
                    page[start..end].fill(0);
                }
            }
            return Ok(());
        }
        for address in range {
            self.write_byte(address, value);
        }
        Ok(())
    }

    fn copy_within(&mut self, destination: u64, source: u64, length: u64) -> Result<(), JsError> {
        let source_start = u128::from(source);
        let destination_start = u128::from(destination);
        self.checked_range(source, length)?;
        self.checked_range(destination, length)?;
        if destination_start > source_start && destination_start < source_start + u128::from(length)
        {
            for offset in (0..length).rev() {
                let byte = self.read_byte(source_start + u128::from(offset));
                self.write_byte(destination_start + u128::from(offset), byte);
            }
        } else {
            for offset in 0..length {
                let byte = self.read_byte(source_start + u128::from(offset));
                self.write_byte(destination_start + u128::from(offset), byte);
            }
        }
        Ok(())
    }

    fn copy_from(
        &mut self,
        destination: u64,
        source: &Self,
        source_address: u64,
        length: u64,
    ) -> Result<(), JsError> {
        let source_start = u128::from(source_address);
        let destination_start = u128::from(destination);
        source.checked_range(source_address, length)?;
        self.checked_range(destination, length)?;
        for offset in 0..u128::from(length) {
            self.write_byte(
                destination_start + offset,
                source.read_byte(source_start + offset),
            );
        }
        Ok(())
    }

    fn grow(&mut self, delta: u64) -> Option<u64> {
        let old_pages = self.page_count;
        let address_bits = if self.memory64 { 64 } else { 32 };
        let max_pages = u64::try_from((1u128 << address_bits) / u128::from(self.page_size()))
            .unwrap_or(u64::MAX);
        let new_pages = old_pages.checked_add(delta)?;
        if new_pages > max_pages || self.maximum_pages.is_some_and(|max| new_pages > max) {
            return None;
        }
        self.page_count = new_pages;
        Some(old_pages)
    }

    fn page_size(&self) -> u64 {
        1u64 << self.page_size_log2
    }
}

impl<H: Host> Vm<H> {
    pub(super) fn wasm_atomic_input_count(
        &self,
        module: crate::WasmModuleId,
        site_index: u32,
    ) -> Result<u16, JsError> {
        self.wasm_modules
            .get(module.raw() as usize)
            .and_then(|module| module.atomic_sites.get(site_index as usize))
            .map(|site| site.input_count())
            .ok_or_else(|| JsError::validation("Wasm atomic site out of bounds".into()))
    }

    pub(super) fn wasm_atomic(
        &mut self,
        module: crate::WasmModuleId,
        site_index: u32,
        operands: &[Value],
    ) -> Result<Option<Value>, JsError> {
        use crate::wasm::atomic::{AtomicRmw, WasmAtomicAction};

        let site = *self
            .wasm_modules
            .get(module.raw() as usize)
            .and_then(|module| module.atomic_sites.get(site_index as usize))
            .ok_or_else(|| JsError::validation("Wasm atomic site out of bounds".into()))?;
        if site.action == WasmAtomicAction::Fence {
            return Ok(None);
        }
        let address = self
            .wasm_memory_operand(module, site.memory_index, operands[0])?
            .checked_add(site.offset)
            .ok_or_else(|| JsError::wasm_trap_error(crate::WasmTrap::MemoryOutOfBounds))?;
        if address & (u64::from(site.access_width) - 1) != 0 {
            return Err(JsError::wasm_trap_error(crate::WasmTrap::UnalignedAtomic));
        }
        let memory_index = *self
            .wasm_modules
            .get(module.raw() as usize)
            .and_then(|module| module.memories.get(site.memory_index as usize))
            .ok_or_else(|| JsError::validation("Wasm atomic memory index out of bounds".into()))?;
        let mask = if site.access_width == 8 {
            u64::MAX
        } else {
            (1u64 << (site.access_width * 8)) - 1
        };

        match site.action {
            WasmAtomicAction::Load => {
                let raw = self
                    .wasm_memories
                    .get(memory_index)
                    .ok_or_else(|| JsError::validation("unknown Wasm atomic memory".into()))?
                    .read_raw(address, site.access_width)?;
                Ok(Some(
                    self.encode_atomic_result(site.value_type.unwrap(), raw),
                ))
            }
            WasmAtomicAction::Store => {
                let value = self.atomic_value_bits(operands[1], site.value_type.unwrap())? & mask;
                self.wasm_memories
                    .get_mut(memory_index)
                    .ok_or_else(|| JsError::validation("unknown Wasm atomic memory".into()))?
                    .write_raw(address, site.access_width, value)?;
                Ok(None)
            }
            WasmAtomicAction::Rmw(operation) => {
                let expected_or_value =
                    self.atomic_value_bits(operands[1], site.value_type.unwrap())? & mask;
                let replacement = if operation == AtomicRmw::CompareExchange {
                    self.atomic_value_bits(operands[2], site.value_type.unwrap())? & mask
                } else {
                    expected_or_value
                };
                let memory = self
                    .wasm_memories
                    .get_mut(memory_index)
                    .ok_or_else(|| JsError::validation("unknown Wasm atomic memory".into()))?;
                let previous = memory.read_raw(address, site.access_width)? & mask;
                let next = match operation {
                    AtomicRmw::Add => previous.wrapping_add(replacement) & mask,
                    AtomicRmw::Sub => previous.wrapping_sub(replacement) & mask,
                    AtomicRmw::And => previous & replacement,
                    AtomicRmw::Or => previous | replacement,
                    AtomicRmw::Xor => previous ^ replacement,
                    AtomicRmw::Exchange => replacement,
                    AtomicRmw::CompareExchange if previous == expected_or_value => replacement,
                    AtomicRmw::CompareExchange => previous,
                };
                if operation != AtomicRmw::CompareExchange || previous == expected_or_value {
                    memory.write_raw(address, site.access_width, next)?;
                }
                Ok(Some(
                    self.encode_atomic_result(site.value_type.unwrap(), previous),
                ))
            }
            WasmAtomicAction::Notify => {
                self.wasm_memories
                    .get(memory_index)
                    .ok_or_else(|| JsError::validation("unknown Wasm atomic memory".into()))?
                    .read_raw(address, site.access_width)?;
                Ok(Some(Value::integer(0)))
            }
            WasmAtomicAction::Wait => {
                let value_type = site.value_type.unwrap();
                let expected = self.atomic_value_bits(operands[1], value_type)? & mask;
                let _timeout = self.wasm_i64_operand(operands[2])?;
                let current = self
                    .wasm_memories
                    .get(memory_index)
                    .ok_or_else(|| JsError::validation("unknown Wasm atomic memory".into()))?
                    .read_raw(address, site.access_width)?
                    & mask;
                let result = if current != expected { 1 } else { 2 };
                Ok(Some(Value::integer(result)))
            }
            WasmAtomicAction::Fence => unreachable!("fences are eliminated during lowering"),
        }
    }

    fn atomic_value_bits(&self, value: Value, ty: crate::WasmType) -> Result<u64, JsError> {
        match self.decode_wasm_scalar(value, ty)? {
            crate::WasmValue::I32(value) => Ok(u64::from(value as u32)),
            crate::WasmValue::I64(value) => Ok(value as u64),
            _ => Err(JsError::validation("invalid Wasm atomic operand".into())),
        }
    }

    fn encode_atomic_result(&mut self, ty: crate::WasmType, raw: u64) -> Value {
        let value = match ty {
            crate::WasmType::I32 => crate::WasmValue::I32(raw as u32 as i32),
            crate::WasmType::I64 => crate::WasmValue::I64(raw as i64),
            _ => unreachable!("atomic value type is integer"),
        };
        self.encode_wasm_scalar(value)
    }

    fn wasm_function_ref_handle(
        &mut self,
        module: crate::WasmModuleId,
        function_index: u32,
        imported_functions: &[Option<crate::WasmFunctionRef>],
    ) -> Result<u32, JsError> {
        let mut target = imported_functions
            .get(function_index as usize)
            .copied()
            .flatten()
            .unwrap_or(crate::WasmFunctionRef {
                module,
                function_index,
            });
        for _ in 0..=self.wasm_modules.len() {
            let next = self
                .wasm_modules
                .get(target.module.raw() as usize)
                .and_then(|stored| {
                    stored
                        .imported_functions
                        .get(target.function_index as usize)
                        .copied()
                        .flatten()
                });
            let Some(next) = next else {
                break;
            };
            target = next;
        }
        if let Some(handle) = self.wasm_function_ref_ids.get(&target).copied() {
            return Ok(handle);
        }
        let id = self.wasm_function_ref_next;
        if id == 0 || id > crate::wasm::WASM_FUNCTION_REF_HANDLE_ID_MASK {
            return Err(JsError::validation(
                "too many Wasm function reference handles".into(),
            ));
        }
        let handle = crate::wasm::WASM_FUNCTION_REF_HANDLE_TAG | id;
        self.wasm_function_ref_next = id + 1;
        self.wasm_function_ref_handles.insert(handle, target);
        self.wasm_function_ref_ids.insert(target, handle);
        Ok(handle)
    }

    pub(super) fn wasm_function_ref_value(
        &mut self,
        module: crate::WasmModuleId,
        function_index: u32,
    ) -> Result<crate::WasmValue, JsError> {
        let imported_functions = self
            .wasm_modules
            .get(module.raw() as usize)
            .ok_or_else(|| JsError::validation("unknown Wasm module".into()))?
            .imported_functions
            .clone();
        let handle = self.wasm_function_ref_handle(module, function_index, &imported_functions)?;
        Ok(crate::WasmValue::FuncRef(Some(handle)))
    }

    fn remap_wasm_function_value(
        &mut self,
        value: crate::WasmValue,
        module: crate::WasmModuleId,
        imported_functions: &[Option<crate::WasmFunctionRef>],
    ) -> Result<crate::WasmValue, JsError> {
        let crate::WasmValue::FuncRef(Some(function_index)) = value else {
            return Ok(value);
        };
        if function_index & !crate::WASM_GC_INITIAL_REFERENCE_ID_MASK
            == crate::WASM_GC_INITIAL_REFERENCE_TAG
            || self.wasm_gc_ref_types.contains_key(&function_index)
            || self.wasm_function_ref_handles.contains_key(&function_index)
        {
            return Ok(value);
        }
        let handle = self.wasm_function_ref_handle(module, function_index, imported_functions)?;
        Ok(crate::WasmValue::FuncRef(Some(handle)))
    }

    pub(super) fn wasm_gc_field(
        &self,
        reference: u32,
        field_index: usize,
        expect_array: bool,
    ) -> Result<crate::WasmGcField, JsError> {
        let object = self
            .wasm_gc_objects
            .get(&reference)
            .ok_or_else(|| JsError::wasm_trap_error(crate::WasmTrap::CastFailure))?;
        let (module_index, type_index) = match (object, expect_array) {
            (
                WasmGcObject::Array {
                    module_index,
                    type_index,
                    ..
                },
                true,
            )
            | (
                WasmGcObject::Struct {
                    module_index,
                    type_index,
                    ..
                },
                false,
            ) => (*module_index, *type_index),
            (WasmGcObject::Array { .. }, false) => {
                return Err(JsError::wasm_trap_error(crate::WasmTrap::CastFailure));
            }
            (WasmGcObject::Struct { .. }, true) => {
                return Err(JsError::wasm_trap_error(crate::WasmTrap::CastFailure));
            }
        };
        let gc_type = self
            .wasm_modules
            .get(module_index as usize)
            .and_then(|module| module.gc_types.get(type_index as usize))
            .and_then(Option::as_ref)
            .ok_or_else(|| JsError::validation("Wasm GC object type metadata is missing".into()))?;
        match gc_type {
            crate::WasmGcType::Array(field) if expect_array => Ok(*field),
            crate::WasmGcType::Struct(fields) if !expect_array => fields
                .get(field_index)
                .copied()
                .ok_or_else(|| JsError::validation("Wasm struct field is out of bounds".into())),
            _ => Err(JsError::validation(
                "Wasm GC object layout does not match its kind".into(),
            )),
        }
    }

    pub(super) fn wasm_gc_object_values(
        &self,
        reference: u32,
        expect_array: bool,
    ) -> Result<&Vec<Value>, JsError> {
        match self.wasm_gc_objects.get(&reference) {
            Some(WasmGcObject::Array { values, .. }) if expect_array => Ok(values),
            Some(WasmGcObject::Struct { values, .. }) if !expect_array => Ok(values),
            Some(_) => Err(JsError::wasm_trap_error(crate::WasmTrap::CastFailure)),
            None => Err(JsError::wasm_trap_error(crate::WasmTrap::CastFailure)),
        }
    }

    pub(crate) fn execute_wasm_i32(
        &mut self,
        function: &crate::WasmI32Function,
        args: &[i32],
    ) -> Result<Option<i32>, JsError> {
        let signature = function.signature();
        if signature
            .params
            .iter()
            .any(|ty| *ty != crate::WasmType::I32)
            || signature
                .result
                .is_some_and(|ty| ty != crate::WasmType::I32)
            || !signature.additional_results.is_empty()
        {
            return Err(JsError::validation(
                "function requires the typed Wasm boundary".into(),
            ));
        }
        let args: Vec<_> = args.iter().copied().map(crate::WasmValue::I32).collect();
        self.execute_wasm(function, &args).map(|result| {
            result.map(|value| {
                let crate::WasmValue::I32(value) = value else {
                    unreachable!("checked i32 signature")
                };
                value
            })
        })
    }

    pub(crate) fn execute_wasm(
        &mut self,
        function: &crate::WasmFunction,
        args: &[crate::WasmValue],
    ) -> Result<Option<crate::WasmValue>, JsError> {
        let program = &function.program;
        program.validate().map_err(JsError::validation)?;
        let signature = function.signature();
        if args.len() != signature.params.len() {
            return Err(JsError::validation("Wasm argument count mismatch".into()));
        }
        if args
            .iter()
            .zip(&signature.params)
            .any(|(value, ty)| !wasm_value_assignable_to(*value, *ty))
        {
            return Err(JsError::validation("Wasm argument type mismatch".into()));
        }
        self.initialize(program)?;
        let module = self.install_wasm_module(function)?;
        let mut results = self.invoke_wasm_function(module, function.entry, args)?;
        match results.len() {
            0 => Ok(None),
            1 => Ok(results.pop()),
            _ => Err(JsError::validation(
                "multi-value function requires the typed Wasm result boundary".into(),
            )),
        }
    }

    pub(crate) fn install_wasm_module(
        &mut self,
        module: &crate::WasmModule,
    ) -> Result<crate::WasmModuleId, JsError> {
        self.install_wasm_module_with_imports(module, &[])
    }

    pub(crate) fn install_wasm_module_with_imports(
        &mut self,
        module: &crate::WasmModule,
        imported_globals: &[Option<crate::WasmGlobalId>],
    ) -> Result<crate::WasmModuleId, JsError> {
        self.install_wasm_module_with_imports_and_tables(module, imported_globals, &[])
    }

    pub(crate) fn install_wasm_module_with_imports_and_tables(
        &mut self,
        module: &crate::WasmModule,
        imported_globals: &[Option<crate::WasmGlobalId>],
        imported_tables: &[Option<crate::WasmTableId>],
    ) -> Result<crate::WasmModuleId, JsError> {
        self.install_wasm_module_with_imports_tables_memories(
            module,
            imported_globals,
            imported_tables,
            &[],
        )
    }

    pub(crate) fn install_wasm_module_with_imports_tables_memories(
        &mut self,
        module: &crate::WasmModule,
        imported_globals: &[Option<crate::WasmGlobalId>],
        imported_tables: &[Option<crate::WasmTableId>],
        imported_memories: &[Option<crate::WasmMemoryId>],
    ) -> Result<crate::WasmModuleId, JsError> {
        module.program.validate().map_err(JsError::validation)?;
        if imported_globals.len() > module.globals.len() {
            return Err(JsError::validation("too many imported Wasm globals".into()));
        }
        if imported_tables.len() != module.table_imports.len()
            || imported_tables != module.table_imports
        {
            return Err(JsError::validation(
                "Wasm table import bindings mismatch".into(),
            ));
        }
        if imported_memories.len() != module.memory_imports.len()
            || imported_memories != module.memory_imports
        {
            return Err(JsError::validation(
                "Wasm memory import bindings mismatch".into(),
            ));
        }
        let program = if self.programs.len() == 0 {
            self.initialize(&module.program)?;
            ProgramId::MAIN
        } else {
            self.store_dynamic_program(module.program.clone())
                .ok_or_else(|| JsError::validation("too many runtime programs".into()))?
        };
        let raw = u32::try_from(self.wasm_modules.len())
            .map_err(|_| JsError::validation("too many Wasm modules".into()))?;
        let module_id = crate::WasmModuleId::from_raw(raw);
        let mut gc_reference_remap = rustc_hash::FxHashMap::default();
        for object in &module.gc_initial_objects {
            let gc_type = module
                .gc_types
                .get(object.type_index as usize)
                .and_then(Option::as_ref)
                .ok_or_else(|| JsError::validation("missing constant Wasm GC type".into()))?;
            let class = match gc_type {
                crate::WasmGcType::Struct(_) => crate::wasm::WASM_GC_REFERENCE_STRUCT_CLASS,
                crate::WasmGcType::Array(_) => crate::wasm::WASM_GC_REFERENCE_ARRAY_CLASS,
            };
            let reference = (class << crate::wasm::WASM_GC_REFERENCE_CLASS_SHIFT)
                | (self.wasm_gc_next_ref & crate::wasm::WASM_GC_REFERENCE_ID_MASK);
            self.wasm_gc_next_ref = self.wasm_gc_next_ref.wrapping_add(1).max(1);
            gc_reference_remap.insert(object.reference, reference);
        }
        for object in &module.gc_initial_objects {
            let reference = gc_reference_remap[&object.reference];
            let gc_type = module.gc_types[object.type_index as usize]
                .as_ref()
                .ok_or_else(|| JsError::validation("missing constant Wasm GC type".into()))?;
            let values = match gc_type {
                crate::WasmGcType::Array(field) => object
                    .values
                    .iter()
                    .copied()
                    .map(|value| normalize_gc_constant(value, *field))
                    .collect::<Result<Vec<_>, _>>()?,
                crate::WasmGcType::Struct(fields) => {
                    if fields.len() != object.values.len() {
                        return Err(JsError::validation(
                            "Wasm struct constant field count mismatch".into(),
                        ));
                    }
                    object
                        .values
                        .iter()
                        .copied()
                        .zip(fields)
                        .map(|(value, field)| normalize_gc_constant(value, *field))
                        .collect::<Result<Vec<_>, _>>()?
                }
            }
            .into_iter()
            .map(|value| remap_gc_constant(value, &gc_reference_remap))
            .map(|value| {
                self.remap_wasm_function_value(value, module_id, &module.imported_functions)
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|value| self.encode_wasm_scalar(value))
            .collect();
            let descriptor = object
                .descriptor
                .map(|value| remap_gc_constant(value, &gc_reference_remap))
                .map(|value| {
                    self.remap_wasm_function_value(value, module_id, &module.imported_functions)
                })
                .transpose()?
                .map(|value| self.encode_wasm_scalar(value))
                .unwrap_or(Value::NULL);
            if module
                .gc_descriptors
                .get(object.type_index as usize)
                .is_some_and(|metadata| metadata.descriptor_type.is_some())
                && descriptor == Value::NULL
            {
                return Err(JsError::wasm_trap_error(
                    crate::WasmTrap::NullDescriptorReference,
                ));
            }
            let stored = match gc_type {
                crate::WasmGcType::Array(_) => crate::vm::wasm::WasmGcObject::Array {
                    module_index: raw,
                    type_index: object.type_index,
                    values,
                },
                crate::WasmGcType::Struct(_) => crate::vm::wasm::WasmGcObject::Struct {
                    module_index: raw,
                    type_index: object.type_index,
                    values,
                    descriptor,
                },
            };
            self.wasm_gc_ref_types.insert(reference, object.type_index);
            self.wasm_gc_objects.insert(reference, stored);
        }
        let mut globals = Vec::with_capacity(module.globals.len());
        for (index, (value, mutable)) in module.globals.iter().copied().enumerate() {
            let value = remap_gc_constant(value, &gc_reference_remap);
            let value =
                self.remap_wasm_function_value(value, module_id, &module.imported_functions)?;
            let imported = imported_globals.get(index).copied().flatten();
            if let Some(global_id) = imported {
                let (actual_value, actual_mutable) = self
                    .wasm_globals
                    .get(global_id.raw() as usize)
                    .copied()
                    .ok_or_else(|| JsError::validation("unknown Wasm global import".into()))?;
                if actual_value.ty() != value.ty() || actual_mutable != mutable {
                    return Err(JsError::validation(
                        "incompatible Wasm global import".into(),
                    ));
                }
                globals.push(global_id);
            } else {
                let global_id = crate::WasmGlobalId::from_raw(
                    u32::try_from(self.wasm_globals.len())
                        .map_err(|_| JsError::validation("too many Wasm globals".into()))?,
                );
                self.wasm_globals.push((value, mutable));
                globals.push(global_id);
            }
        }
        if module.tag_imports.len() > module.tag_signatures.len() {
            return Err(JsError::validation(
                "too many imported Wasm exception tags".into(),
            ));
        }
        let mut tags = Vec::with_capacity(module.tag_signatures.len());
        for (index, signature) in module.tag_signatures.iter().enumerate() {
            if index < module.tag_imports.len() {
                let tag = module.tag_imports[index].ok_or_else(|| {
                    JsError::validation("unresolved imported Wasm exception tag".into())
                })?;
                let actual = self
                    .wasm_modules
                    .get(tag.module.raw() as usize)
                    .and_then(|provider| provider.tag_signatures.get(tag.index as usize))
                    .ok_or_else(|| JsError::validation("unknown Wasm exception tag".into()))?;
                if actual != signature {
                    return Err(JsError::validation(
                        "incompatible Wasm exception tag import".into(),
                    ));
                }
                tags.push(tag);
            } else {
                tags.push(crate::WasmTagId {
                    module: module_id,
                    index: u32::try_from(index)
                        .map_err(|_| JsError::validation("too many Wasm exception tags".into()))?,
                });
            }
        }
        let mut element_segments = module.element_segments.clone();
        for segment in &mut element_segments {
            if let Some(values) = &mut segment.values {
                for value in values {
                    *value = remap_gc_constant(*value, &gc_reference_remap);
                    *value = self.remap_wasm_function_value(
                        *value,
                        module_id,
                        &module.imported_functions,
                    )?;
                }
            }
        }
        self.wasm_modules.push(StoredWasmModule {
            program,
            signatures: module.signatures.clone(),
            globals,
            memories: Vec::new(),
            tables: Vec::new(),
            type_signatures: module.type_signatures.clone(),
            gc_supertypes: module.gc_supertypes.clone(),
            gc_type_canonicals: module.gc_type_canonicals.clone(),
            gc_type_fingerprints: module.gc_type_fingerprints.clone(),
            gc_descriptors: module.gc_descriptors.clone(),
            function_type_indices: module.function_type_indices.clone(),
            gc_types: module.gc_types.clone(),
            tag_signatures: module.tag_signatures.clone(),
            tags,
            exception_handlers: module.exception_handlers.clone(),
            indirect_sites: module.indirect_sites.clone(),
            v128_sites: module.v128_sites.clone(),
            atomic_sites: module.atomic_sites.clone(),
            imported_functions: module.imported_functions.clone(),
            element_segments,
            data_segments: module.data_segments.clone(),
        });
        let mut tables = Vec::with_capacity(module.tables.len());
        for (index, original_init) in module.tables.iter().enumerate() {
            let mut init = remap_gc_table_values(original_init, &gc_reference_remap);
            init.initial_value = self.remap_wasm_function_value(
                init.initial_value,
                module_id,
                &module.imported_functions,
            )?;
            for (_, values) in &mut init.elements {
                for value in values {
                    *value = self.remap_wasm_function_value(
                        *value,
                        module_id,
                        &module.imported_functions,
                    )?;
                }
            }
            match module.table_imports.get(index) {
                Some(Some(table_id)) => {
                    tables.push(*table_id);
                    self.wasm_modules[raw as usize].tables.clone_from(&tables);
                    let table = self
                        .wasm_tables
                        .get(table_id.raw() as usize)
                        .cloned()
                        .ok_or_else(|| JsError::validation("unknown imported Wasm table".into()))?;
                    {
                        let mut state = table.borrow_mut();
                        if state.table64 != init.table64
                            || state.element_type != init.element_type
                            || state.size < init.initial_size
                            || init.maximum_size.is_some_and(|maximum| {
                                state.maximum_size.is_none_or(|actual| actual > maximum)
                            })
                        {
                            return Err(JsError::validation(
                                "incompatible imported Wasm table limits".into(),
                            ));
                        }
                        state.apply_elements(&init)?;
                    }
                }
                Some(None) => {
                    return Err(JsError::validation("unresolved imported Wasm table".into()));
                }
                None => {
                    let table_id = crate::WasmTableId::from_raw(
                        u32::try_from(self.wasm_tables.len())
                            .map_err(|_| JsError::validation("too many Wasm tables".into()))?,
                    );
                    self.wasm_tables
                        .push(std::rc::Rc::new(std::cell::RefCell::new(
                            WasmTableState::new(&init)?,
                        )));
                    tables.push(table_id);
                    self.wasm_modules[raw as usize].tables.clone_from(&tables);
                }
            }
        }
        let mut memories = Vec::with_capacity(module.memories.len());
        for (index, init) in module.memories.iter().enumerate() {
            let memory_index = match module.memory_imports.get(index) {
                Some(Some(memory_id)) => {
                    let memory_index = memory_id.raw() as usize;
                    let state = self.wasm_memories.get(memory_index).ok_or_else(|| {
                        JsError::validation("unknown imported Wasm memory".into())
                    })?;
                    if state.memory64 != init.memory64
                        || state.shared != init.shared
                        || state.page_size_log2 != init.page_size_log2
                        || state.page_count < init.initial_pages
                        || init.maximum_pages.is_some_and(|maximum| {
                            state.maximum_pages.is_none_or(|actual| actual > maximum)
                        })
                    {
                        return Err(JsError::validation(
                            "incompatible imported Wasm memory limits".into(),
                        ));
                    }
                    memories.push(memory_index);
                    memory_index
                }
                Some(None) => {
                    return Err(JsError::validation(
                        "unresolved imported Wasm memory".into(),
                    ));
                }
                None => {
                    let memory_id = self.wasm_memories.len();
                    self.wasm_memories.push(WasmMemoryState::new(init)?);
                    memories.push(memory_id);
                    memory_id
                }
            };
            self.wasm_modules[raw as usize]
                .memories
                .clone_from(&memories);
            let state = self
                .wasm_memories
                .get_mut(memory_index)
                .ok_or_else(|| JsError::validation("unknown Wasm memory".into()))?;
            for (offset, bytes) in &init.data_segments {
                state.write_bytes(*offset, bytes)?;
            }
        }
        if let Some(start_function) = module.start_function {
            self.invoke_wasm_function(module_id, start_function, &[])?;
        }
        Ok(module_id)
    }

    pub(super) fn call_wasm_import(
        &mut self,
        target: crate::WasmFunctionRef,
        arguments: &[Value],
    ) -> Result<Value, JsError> {
        let stored = self
            .wasm_modules
            .get(target.module.raw() as usize)
            .ok_or_else(|| JsError::validation("unknown imported Wasm module".into()))?;
        let program_id = stored.program;
        let program = self
            .programs
            .get(program_id)
            .ok_or_else(|| JsError::validation("missing imported Wasm program".into()))?;
        let previous_program = std::mem::replace(&mut self.active_program, program_id);
        let previous_module = self.active_wasm_module.replace(target.module);
        let result = self.call_user_maybe_async(
            &program,
            target.function_index,
            Value::NULL,
            Value::UNDEFINED,
            arguments,
            CallContext::Internal,
        );
        self.active_program = previous_program;
        self.active_wasm_module = previous_module;
        result
    }

    pub(super) fn tail_call_wasm_function(
        &mut self,
        target: crate::WasmFunctionRef,
        frame: usize,
        arguments: &[Value],
    ) -> Result<(), JsError> {
        let mut target = target;
        for _ in 0..=self.wasm_modules.len() {
            let imported = self
                .wasm_modules
                .get(target.module.raw() as usize)
                .and_then(|module| {
                    module
                        .imported_functions
                        .get(target.function_index as usize)
                        .copied()
                        .flatten()
                });
            let Some(imported) = imported else {
                break;
            };
            target = imported;
        }
        let stored = self
            .wasm_modules
            .get(target.module.raw() as usize)
            .ok_or_else(|| JsError::validation("unknown Wasm tail-call module".into()))?;
        let program_id = stored.program;
        let program = self
            .programs
            .get(program_id)
            .ok_or_else(|| JsError::validation("missing Wasm tail-call program".into()))?;
        self.with_call_roots(arguments.iter().copied(), |vm| {
            vm.active_program = program_id;
            vm.active_wasm_module = Some(target.module);
            vm.prepare_user_tail(
                &program,
                frame,
                target.function_index,
                Value::NULL,
                Value::UNDEFINED,
                arguments,
                CallContext::Internal,
            )
        })
    }

    pub(super) fn execute_wasm_v128(
        &mut self,
        module: crate::WasmModuleId,
        site_index: u32,
        result: Value,
        left: Value,
        right: Value,
    ) -> Result<Value, JsError> {
        let site = *self
            .wasm_modules
            .get(module.raw() as usize)
            .and_then(|module| module.v128_sites.get(site_index as usize))
            .ok_or_else(|| JsError::validation("Wasm SIMD site out of bounds".into()))?;
        let name = crate::wasm::simd::name(site.operator)
            .ok_or_else(|| JsError::validation("Wasm SIMD operator out of bounds".into()))?;
        let arity = crate::wasm::simd::arity(name);
        let values = match arity {
            1 => [left, Value::UNDEFINED, Value::UNDEFINED],
            2 => [left, right, Value::UNDEFINED],
            3 => [result, left, right],
            _ => return Err(JsError::validation("invalid Wasm SIMD arity".into())),
        };
        let scalar_type = if name.ends_with("Splat") {
            if name.starts_with("I8") || name.starts_with("I16") || name.starts_with("I32") {
                Some(crate::WasmType::I32)
            } else if name.starts_with("I64") {
                Some(crate::WasmType::I64)
            } else if name.starts_with("F32") {
                Some(crate::WasmType::F32)
            } else {
                Some(crate::WasmType::F64)
            }
        } else if name.contains("ReplaceLane") {
            Some(match name {
                "I8x16ReplaceLane" | "I16x8ReplaceLane" | "I32x4ReplaceLane" => {
                    crate::WasmType::I32
                }
                "I64x2ReplaceLane" => crate::WasmType::I64,
                "F32x4ReplaceLane" => crate::WasmType::F32,
                "F64x2ReplaceLane" => crate::WasmType::F64,
                _ => return Err(JsError::validation("invalid Wasm SIMD lane type".into())),
            })
        } else if name.ends_with("Shl") || name.ends_with("ShrS") || name.ends_with("ShrU") {
            None
        } else {
            None
        };
        let vector_at = |index: usize, vm: &Self| -> Result<u128, JsError> {
            match vm.decode_wasm_scalar(values[index], crate::WasmType::V128)? {
                crate::WasmValue::V128(bits) => Ok(bits),
                _ => unreachable!("decoded v128 value"),
            }
        };
        let mut a = 0u128;
        let mut b = 0u128;
        let mut c = 0u128;
        let mut scalar = None;
        if name.ends_with("Splat") {
            scalar = Some(self.decode_wasm_scalar(values[0], scalar_type.unwrap())?);
        } else if name.contains("ReplaceLane") {
            a = vector_at(0, self)?;
            scalar = Some(self.decode_wasm_scalar(values[1], scalar_type.unwrap())?);
        } else if name.contains("ExtractLane")
            || name.ends_with("AllTrue")
            || name.ends_with("Bitmask")
            || name == "V128AnyTrue"
            || name == "V128Not"
        {
            a = vector_at(0, self)?;
        } else if arity == 3 {
            a = vector_at(0, self)?;
            b = vector_at(1, self)?;
            c = vector_at(2, self)?;
        } else if arity == 2 {
            a = vector_at(0, self)?;
            if name.ends_with("Shl") || name.ends_with("ShrS") || name.ends_with("ShrU") {
                scalar = Some(self.decode_wasm_scalar(values[1], crate::WasmType::I32)?);
            } else {
                b = vector_at(1, self)?;
            }
        } else if arity == 1 {
            a = vector_at(0, self)?;
        }
        let value = crate::wasm::simd::apply(name, site.lane, site.shuffle, a, b, c, scalar)
            .ok_or_else(|| JsError::validation(format!("unsupported Wasm SIMD operator {name}")))?;
        if value.ty() != crate::wasm::simd::result_type(name) {
            return Err(JsError::validation("invalid Wasm SIMD result type".into()));
        }
        Ok(self.encode_wasm_scalar(value))
    }

    pub(super) fn wasm_table_size(
        &self,
        module: crate::WasmModuleId,
        table_index: u32,
    ) -> Option<(u64, bool)> {
        let table = *self
            .wasm_modules
            .get(module.raw() as usize)?
            .tables
            .get(table_index as usize)?;
        let table = self.wasm_tables.get(table.raw() as usize)?.borrow();
        Some((table.size, table.table64))
    }

    pub(crate) fn wasm_table_id(
        &self,
        module: crate::WasmModuleId,
        table_index: u32,
    ) -> Option<crate::WasmTableId> {
        self.wasm_modules
            .get(module.raw() as usize)?
            .tables
            .get(table_index as usize)
            .copied()
    }

    pub(crate) fn wasm_tag_id(
        &self,
        module: crate::WasmModuleId,
        tag_index: u32,
    ) -> Option<crate::WasmTagId> {
        self.wasm_modules
            .get(module.raw() as usize)?
            .tags
            .get(tag_index as usize)
            .copied()
    }

    pub(crate) fn wasm_table_info(
        &self,
        table: crate::WasmTableId,
    ) -> Option<(crate::WasmType, bool, u64, Option<u64>)> {
        let table = self.wasm_tables.get(table.raw() as usize)?.borrow();
        Some((
            table.element_type,
            table.table64,
            table.size,
            table.maximum_size,
        ))
    }

    pub(crate) fn wasm_memory_id(
        &self,
        module: crate::WasmModuleId,
        memory_index: u32,
    ) -> Option<crate::WasmMemoryId> {
        let memory = *self
            .wasm_modules
            .get(module.raw() as usize)?
            .memories
            .get(memory_index as usize)?;
        u32::try_from(memory)
            .ok()
            .map(crate::WasmMemoryId::from_raw)
    }

    pub(crate) fn wasm_memory_info(
        &self,
        memory: crate::WasmMemoryId,
    ) -> Option<(bool, u64, Option<u64>, u32, bool)> {
        let memory = self.wasm_memories.get(memory.raw() as usize)?;
        Some((
            memory.memory64,
            memory.page_count,
            memory.maximum_pages,
            memory.page_size_log2,
            memory.shared,
        ))
    }

    pub(super) fn wasm_table_is64(
        &self,
        module: crate::WasmModuleId,
        table_index: u32,
    ) -> Option<bool> {
        let table = *self
            .wasm_modules
            .get(module.raw() as usize)?
            .tables
            .get(table_index as usize)?;
        Some(self.wasm_tables.get(table.raw() as usize)?.borrow().table64)
    }

    pub(super) fn wasm_table_operand(
        &self,
        module: crate::WasmModuleId,
        table_index: u32,
        value: Value,
    ) -> Result<u64, JsError> {
        let table64 = self
            .wasm_table_is64(module, table_index)
            .ok_or_else(|| JsError::validation("Wasm table index out of bounds".into()))?;
        let ty = if table64 {
            crate::WasmType::I64
        } else {
            crate::WasmType::I32
        };
        match self.decode_wasm_scalar(value, ty)? {
            crate::WasmValue::I64(value) => Ok(value as u64),
            crate::WasmValue::I32(value) => Ok(u64::from(value as u32)),
            _ => unreachable!("table operand has integer type"),
        }
    }

    pub(super) fn wasm_table_grow(
        &mut self,
        module: crate::WasmModuleId,
        table_index: u32,
        delta: u64,
        initial: crate::WasmValue,
    ) -> Option<(u64, bool)> {
        let table = *self
            .wasm_modules
            .get(module.raw() as usize)?
            .tables
            .get(table_index as usize)?;
        let table = self.wasm_tables.get(table.raw() as usize)?.clone();
        let mut table = table.borrow_mut();
        let table64 = table.table64;
        Some((table.grow(delta, initial), table64))
    }

    pub(super) fn wasm_table_fill(
        &mut self,
        module: crate::WasmModuleId,
        table_index: u32,
        destination: u64,
        value: crate::WasmValue,
        length: u64,
    ) -> Result<(), JsError> {
        let table = *self
            .wasm_modules
            .get_mut(module.raw() as usize)
            .and_then(|module| module.tables.get(table_index as usize))
            .ok_or_else(|| JsError::validation("Wasm table index out of bounds".into()))?;
        self.wasm_tables
            .get(table.raw() as usize)
            .ok_or_else(|| JsError::validation("Wasm table index out of bounds".into()))?
            .borrow_mut()
            .fill(destination, value, length)
    }

    pub(super) fn wasm_table_copy(
        &mut self,
        module: crate::WasmModuleId,
        destination_table: u32,
        source_table: u32,
        destination: u64,
        source: u64,
        length: u64,
    ) -> Result<(), JsError> {
        let tables = &self
            .wasm_modules
            .get(module.raw() as usize)
            .ok_or_else(|| JsError::validation("unknown Wasm module".into()))?
            .tables;
        let destination_id = *tables
            .get(destination_table as usize)
            .ok_or_else(|| JsError::validation("Wasm table index out of bounds".into()))?;
        let source_id = *tables
            .get(source_table as usize)
            .ok_or_else(|| JsError::validation("Wasm table index out of bounds".into()))?;
        let destination_state = self
            .wasm_tables
            .get(destination_id.raw() as usize)
            .cloned()
            .ok_or_else(|| JsError::validation("Wasm table index out of bounds".into()))?;
        let source_state = self
            .wasm_tables
            .get(source_id.raw() as usize)
            .cloned()
            .ok_or_else(|| JsError::validation("Wasm table index out of bounds".into()))?;
        if !wasm_reference_type_assignable(
            source_state.borrow().element_type,
            destination_state.borrow().element_type,
        ) {
            return Err(JsError::validation(
                "incompatible Wasm table.copy types".into(),
            ));
        }
        destination_state
            .borrow()
            .check_range(destination, length)?;
        source_state.borrow().check_range(source, length)?;
        let values = {
            let source_state = source_state.borrow();
            (source..source + length)
                .map(|index| source_state.get(index).expect("checked table range"))
                .collect::<Vec<_>>()
        };
        let mut destination_state = destination_state.borrow_mut();
        for (offset, value) in values.into_iter().enumerate() {
            destination_state.set(destination + offset as u64, value)?;
        }
        Ok(())
    }

    pub(super) fn wasm_table_init(
        &mut self,
        module: crate::WasmModuleId,
        table_index: u32,
        element_index: u32,
        destination: u64,
        source: u32,
        length: u32,
    ) -> Result<(), JsError> {
        let stored = self
            .wasm_modules
            .get(module.raw() as usize)
            .ok_or_else(|| JsError::validation("unknown Wasm module".into()))?;
        let table_id = *stored
            .tables
            .get(table_index as usize)
            .ok_or_else(|| JsError::validation("Wasm table index out of bounds".into()))?;
        let segment = stored
            .element_segments
            .get(element_index as usize)
            .ok_or_else(|| JsError::validation("Wasm element index out of bounds".into()))?;
        let table = self
            .wasm_tables
            .get(table_id.raw() as usize)
            .cloned()
            .ok_or_else(|| JsError::validation("Wasm table index out of bounds".into()))?;
        if !wasm_reference_type_assignable(segment.element_type, table.borrow().element_type) {
            return Err(JsError::validation(
                "incompatible Wasm table.init types".into(),
            ));
        }
        table.borrow().check_range(destination, u64::from(length))?;
        let values = segment.values.as_deref().unwrap_or(&[]);
        let source_end = source
            .checked_add(length)
            .filter(|end| *end as usize <= values.len())
            .ok_or_else(|| JsError::wasm_trap_error(crate::WasmTrap::TableOutOfBounds))?;
        let copied = values[source as usize..source_end as usize].to_vec();
        let mut table = table.borrow_mut();
        for (offset, value) in copied.into_iter().enumerate() {
            table.set(destination + offset as u64, value)?;
        }
        Ok(())
    }

    pub(super) fn wasm_element_drop(
        &mut self,
        module: crate::WasmModuleId,
        element_index: u32,
    ) -> Result<(), JsError> {
        let segment = self
            .wasm_modules
            .get_mut(module.raw() as usize)
            .and_then(|module| module.element_segments.get_mut(element_index as usize))
            .ok_or_else(|| JsError::validation("Wasm element index out of bounds".into()))?;
        segment.values = None;
        Ok(())
    }

    pub(crate) fn invoke_wasm_function(
        &mut self,
        module: crate::WasmModuleId,
        function_index: u32,
        args: &[crate::WasmValue],
    ) -> Result<Vec<crate::WasmValue>, JsError> {
        let stored = self
            .wasm_modules
            .get(module.raw() as usize)
            .ok_or_else(|| JsError::validation("unknown Wasm module".into()))?;
        let signature = stored
            .signatures
            .get(function_index as usize)
            .cloned()
            .ok_or_else(|| JsError::validation("Wasm function index out of bounds".into()))?;
        if args.len() != signature.params.len() {
            return Err(JsError::validation("Wasm argument count mismatch".into()));
        }
        if args
            .iter()
            .zip(&signature.params)
            .any(|(value, ty)| !wasm_value_assignable_to(*value, *ty))
        {
            return Err(JsError::validation("Wasm argument type mismatch".into()));
        }
        let program_id = stored.program;
        let imported_target = stored
            .imported_functions
            .get(function_index as usize)
            .copied()
            .flatten();
        let program = self
            .programs
            .get(program_id)
            .ok_or_else(|| JsError::validation("missing Wasm program".into()))?;
        let previous_program = std::mem::replace(&mut self.active_program, program_id);
        let previous_module = self.active_wasm_module.replace(module);
        let values = args
            .iter()
            .copied()
            .map(|value| self.encode_wasm_scalar(value))
            .collect::<Vec<_>>();
        let result = if let Some(target) = imported_target {
            self.call_wasm_import(target, &values)
        } else {
            self.call_user(
                &program,
                function_index,
                Value::NULL,
                Value::UNDEFINED,
                &values,
                CallContext::Internal,
            )
        };
        self.active_program = previous_program;
        self.active_wasm_module = previous_module;
        let result = result?;
        let result_types = signature.result_types().collect::<Vec<_>>();
        if result_types.is_empty() {
            return Ok(Vec::new());
        }
        if result_types.len() == 1 {
            return Ok(vec![self.decode_wasm_scalar(result, result_types[0])?]);
        }
        let Some(Cell::WasmMultiValue(values)) = self.heap.get(result) else {
            return Err(JsError::validation(
                "invalid Wasm multi-value result".into(),
            ));
        };
        if values.len() != result_types.len() {
            return Err(JsError::validation(
                "Wasm multi-value result count mismatch".into(),
            ));
        }
        values
            .iter()
            .copied()
            .zip(result_types)
            .map(|(value, ty)| self.decode_wasm_scalar(value, ty))
            .collect()
    }

    pub(crate) fn wasm_global(
        &self,
        module: crate::WasmModuleId,
        global_index: u32,
    ) -> Option<crate::WasmValue> {
        let global_id = *self
            .wasm_modules
            .get(module.raw() as usize)?
            .globals
            .get(global_index as usize)?;
        self.wasm_globals
            .get(global_id.raw() as usize)
            .map(|(value, _)| *value)
    }

    pub(crate) fn wasm_global_id(
        &self,
        module: crate::WasmModuleId,
        global_index: u32,
    ) -> Option<crate::WasmGlobalId> {
        self.wasm_modules
            .get(module.raw() as usize)?
            .globals
            .get(global_index as usize)
            .copied()
    }

    pub(super) fn wasm_memory_size(
        &self,
        module: crate::WasmModuleId,
        memory_index: u32,
    ) -> Option<(u64, bool)> {
        let memory = *self
            .wasm_modules
            .get(module.raw() as usize)?
            .memories
            .get(memory_index as usize)?;
        let memory = self.wasm_memories.get(memory)?;
        Some((memory.page_count, memory.memory64))
    }

    pub(super) fn wasm_memory_is64(
        &self,
        module: crate::WasmModuleId,
        memory_index: u32,
    ) -> Option<bool> {
        let memory = *self
            .wasm_modules
            .get(module.raw() as usize)?
            .memories
            .get(memory_index as usize)?;
        Some(self.wasm_memories.get(memory)?.memory64)
    }

    pub(super) fn wasm_memory_operand(
        &self,
        module: crate::WasmModuleId,
        memory_index: u32,
        value: Value,
    ) -> Result<u64, JsError> {
        let memory64 = self
            .wasm_memory_is64(module, memory_index)
            .ok_or_else(|| JsError::validation("Wasm memory index out of bounds".into()))?;
        let ty = if memory64 {
            crate::WasmType::I64
        } else {
            crate::WasmType::I32
        };
        match self.decode_wasm_scalar(value, ty)? {
            crate::WasmValue::I64(value) => Ok(value as u64),
            crate::WasmValue::I32(value) => Ok(u64::from(value as u32)),
            _ => unreachable!("memory operand has integer type"),
        }
    }

    pub(super) fn wasm_memory_grow(
        &mut self,
        module: crate::WasmModuleId,
        memory_index: u32,
        delta: u64,
    ) -> Option<(u64, bool)> {
        let memory = *self
            .wasm_modules
            .get(module.raw() as usize)?
            .memories
            .get(memory_index as usize)?;
        let memory = self.wasm_memories.get_mut(memory)?;
        let memory64 = memory.memory64;
        Some((memory.grow(delta).unwrap_or(u64::MAX), memory64))
    }

    pub(super) fn wasm_memory_load(
        &self,
        module: crate::WasmModuleId,
        memory_index: u32,
        address: u64,
        offset: u32,
        kind: crate::WasmMemoryAccessKind,
    ) -> Result<crate::WasmValue, JsError> {
        let memory = *self
            .wasm_modules
            .get(module.raw() as usize)
            .and_then(|module| module.memories.get(memory_index as usize))
            .ok_or_else(|| JsError::validation("Wasm memory index out of bounds".into()))?;
        let raw = self
            .wasm_memories
            .get(memory)
            .ok_or_else(|| JsError::validation("missing Wasm memory".into()))?
            .read(address, u64::from(offset), kind)?;
        use crate::WasmMemoryAccessKind as Kind;
        Ok(match kind {
            Kind::I32Load8S => crate::WasmValue::I32(raw as u8 as i8 as i32),
            Kind::I32Load8U => crate::WasmValue::I32(raw as u8 as i32),
            Kind::I32Load16S => crate::WasmValue::I32(raw as u16 as i16 as i32),
            Kind::I32Load16U => crate::WasmValue::I32(raw as u16 as i32),
            Kind::I64Load8S => crate::WasmValue::I64(raw as u8 as i8 as i64),
            Kind::I64Load8U => crate::WasmValue::I64(raw as u8 as i64),
            Kind::I64Load16S => crate::WasmValue::I64(raw as u16 as i16 as i64),
            Kind::I64Load16U => crate::WasmValue::I64(raw as u16 as i64),
            Kind::I64Load32S => crate::WasmValue::I64(raw as u32 as i32 as i64),
            Kind::I64Load32U => crate::WasmValue::I64(raw as u32 as i64),
            Kind::I32Load => crate::WasmValue::I32(raw as u32 as i32),
            Kind::I64Load => crate::WasmValue::I64(raw as i64),
            Kind::F32Load => crate::WasmValue::F32(raw as u32),
            Kind::F64Load => crate::WasmValue::F64(raw),
            _ => return Err(JsError::validation("Wasm store used as a load".into())),
        })
    }

    pub(super) fn wasm_memory_store(
        &mut self,
        module: crate::WasmModuleId,
        memory_index: u32,
        address: u64,
        offset: u32,
        kind: crate::WasmMemoryAccessKind,
        value: Value,
    ) -> Result<(), JsError> {
        let memory = *self
            .wasm_modules
            .get(module.raw() as usize)
            .and_then(|module| module.memories.get(memory_index as usize))
            .ok_or_else(|| JsError::validation("Wasm memory index out of bounds".into()))?;
        let value = self.decode_wasm_scalar(value, kind.value_type())?;
        let raw = match value.bits() {
            crate::wasm::ScalarBits::Bits32(bits) => u64::from(bits),
            crate::wasm::ScalarBits::Bits64(bits) => bits,
            crate::wasm::ScalarBits::Bits128(_) => {
                return Err(JsError::validation(
                    "Wasm memory store expected a scalar".into(),
                ));
            }
            crate::wasm::ScalarBits::Reference(_) => {
                return Err(JsError::validation(
                    "Wasm memory store expected a scalar".into(),
                ));
            }
            crate::wasm::ScalarBits::I31Reference(_) => {
                return Err(JsError::validation(
                    "Wasm memory store expected a scalar".into(),
                ));
            }
            crate::wasm::ScalarBits::ExnReference(_) => {
                return Err(JsError::validation(
                    "Wasm memory store expected a scalar".into(),
                ));
            }
        };
        self.wasm_memories
            .get_mut(memory)
            .ok_or_else(|| JsError::validation("missing Wasm memory".into()))?
            .write(address, u64::from(offset), kind, raw)
    }

    pub(super) fn wasm_v128_memory_load(
        &self,
        module: crate::WasmModuleId,
        site_index: u32,
        address_value: Value,
        original: Value,
    ) -> Result<crate::WasmValue, JsError> {
        let site = *self
            .wasm_modules
            .get(module.raw() as usize)
            .and_then(|module| module.v128_sites.get(site_index as usize))
            .ok_or_else(|| JsError::validation("Wasm SIMD site out of bounds".into()))?;
        let memory_index = site.memory_index.ok_or_else(|| {
            JsError::validation("Wasm SIMD site is not a memory operation".into())
        })?;
        let address = self.wasm_memory_operand(module, memory_index, address_value)?;
        let name = crate::wasm::simd::name(site.operator)
            .ok_or_else(|| JsError::validation("Wasm SIMD operator out of bounds".into()))?;
        let width = crate::wasm::simd::memory_width(name)
            .ok_or_else(|| JsError::validation("invalid Wasm SIMD memory operator".into()))?;
        let memory = *self
            .wasm_modules
            .get(module.raw() as usize)
            .and_then(|module| module.memories.get(memory_index as usize))
            .ok_or_else(|| JsError::validation("Wasm memory index out of bounds".into()))?;
        let bytes = self
            .wasm_memories
            .get(memory)
            .ok_or_else(|| JsError::validation("missing Wasm memory".into()))?
            .read_bytes(address, u64::from(site.offset), width)?;
        let original = if name.ends_with("Lane") {
            match self.decode_wasm_scalar(original, crate::WasmType::V128)? {
                crate::WasmValue::V128(bits) => bits,
                _ => unreachable!("decoded v128 value"),
            }
        } else {
            0
        };
        let bits = crate::wasm::simd::memory_load(name, &bytes[..width], original, site.lane)
            .ok_or_else(|| JsError::validation(format!("unsupported Wasm SIMD load {name}")))?;
        Ok(crate::WasmValue::V128(bits))
    }

    pub(super) fn wasm_v128_memory_store(
        &mut self,
        module: crate::WasmModuleId,
        site_index: u32,
        address_value: Value,
        value: Value,
    ) -> Result<(), JsError> {
        let site = *self
            .wasm_modules
            .get(module.raw() as usize)
            .and_then(|module| module.v128_sites.get(site_index as usize))
            .ok_or_else(|| JsError::validation("Wasm SIMD site out of bounds".into()))?;
        let memory_index = site.memory_index.ok_or_else(|| {
            JsError::validation("Wasm SIMD site is not a memory operation".into())
        })?;
        let address = self.wasm_memory_operand(module, memory_index, address_value)?;
        let name = crate::wasm::simd::name(site.operator)
            .ok_or_else(|| JsError::validation("Wasm SIMD operator out of bounds".into()))?;
        let memory = *self
            .wasm_modules
            .get(module.raw() as usize)
            .and_then(|module| module.memories.get(memory_index as usize))
            .ok_or_else(|| JsError::validation("Wasm memory index out of bounds".into()))?;
        let crate::WasmValue::V128(bits) = self.decode_wasm_scalar(value, crate::WasmType::V128)?
        else {
            unreachable!("decoded v128 value")
        };
        let (width, raw) = crate::wasm::simd::memory_store(name, bits, site.lane)
            .ok_or_else(|| JsError::validation(format!("unsupported Wasm SIMD store {name}")))?;
        let bytes = raw.to_le_bytes();
        self.wasm_memories
            .get_mut(memory)
            .ok_or_else(|| JsError::validation("missing Wasm memory".into()))?
            .write_bytes(
                address
                    .checked_add(u64::from(site.offset))
                    .ok_or_else(|| JsError::wasm_trap_error(crate::WasmTrap::MemoryOutOfBounds))?,
                &bytes[..width],
            )
    }

    pub(super) fn wasm_memory_fill(
        &mut self,
        module: crate::WasmModuleId,
        memory_index: u32,
        destination: u64,
        value: i32,
        length: u64,
    ) -> Result<(), JsError> {
        let memory = *self
            .wasm_modules
            .get(module.raw() as usize)
            .and_then(|module| module.memories.get(memory_index as usize))
            .ok_or_else(|| JsError::validation("Wasm memory index out of bounds".into()))?;
        self.wasm_memories
            .get_mut(memory)
            .ok_or_else(|| JsError::validation("missing Wasm memory".into()))?
            .fill(destination, value as u8, length)
    }

    pub(super) fn wasm_memory_copy(
        &mut self,
        module: crate::WasmModuleId,
        destination_memory: u32,
        source_memory: u32,
        destination: u64,
        source: u64,
        length: u64,
    ) -> Result<(), JsError> {
        let stored = self
            .wasm_modules
            .get(module.raw() as usize)
            .ok_or_else(|| JsError::validation("unknown Wasm module".into()))?;
        let destination_memory = *stored
            .memories
            .get(destination_memory as usize)
            .ok_or_else(|| JsError::validation("Wasm memory index out of bounds".into()))?;
        let source_memory = *stored
            .memories
            .get(source_memory as usize)
            .ok_or_else(|| JsError::validation("Wasm memory index out of bounds".into()))?;
        if destination_memory == source_memory {
            return self.wasm_memories[destination_memory].copy_within(destination, source, length);
        }
        if destination_memory < source_memory {
            let (before, after) = self.wasm_memories.split_at_mut(source_memory);
            before[destination_memory].copy_from(destination, &after[0], source, length)
        } else {
            let (before, after) = self.wasm_memories.split_at_mut(destination_memory);
            after[0].copy_from(destination, &before[source_memory], source, length)
        }
    }

    pub(super) fn wasm_memory_init(
        &mut self,
        module: crate::WasmModuleId,
        memory_index: u32,
        data_index: u32,
        destination: u64,
        source: u32,
        length: u64,
    ) -> Result<(), JsError> {
        let stored = self
            .wasm_modules
            .get(module.raw() as usize)
            .ok_or_else(|| JsError::validation("unknown Wasm module".into()))?;
        let memory = *stored
            .memories
            .get(memory_index as usize)
            .ok_or_else(|| JsError::validation("Wasm memory index out of bounds".into()))?;
        let data = stored
            .data_segments
            .get(data_index as usize)
            .and_then(Option::as_deref)
            .unwrap_or(&[]);
        let source_start = source as usize;
        let length = usize::try_from(length)
            .map_err(|_| JsError::wasm_trap_error(crate::WasmTrap::MemoryOutOfBounds))?;
        let source_end = source_start
            .checked_add(length)
            .ok_or_else(|| JsError::wasm_trap_error(crate::WasmTrap::MemoryOutOfBounds))?;
        let bytes = data
            .get(source_start..source_end)
            .ok_or_else(|| JsError::wasm_trap_error(crate::WasmTrap::MemoryOutOfBounds))?;
        self.wasm_memories
            .get_mut(memory)
            .ok_or_else(|| JsError::validation("missing Wasm memory".into()))?
            .write_bytes(destination, bytes)
    }

    pub(super) fn wasm_data_drop(
        &mut self,
        module: crate::WasmModuleId,
        data_index: u32,
    ) -> Result<(), JsError> {
        let segment = self
            .wasm_modules
            .get_mut(module.raw() as usize)
            .and_then(|module| module.data_segments.get_mut(data_index as usize))
            .ok_or_else(|| JsError::validation("Wasm data index out of bounds".into()))?;
        *segment = None;
        Ok(())
    }

    pub(super) fn wasm_i64_operand(&self, value: Value) -> Result<i64, JsError> {
        let crate::WasmValue::I64(value) = self.decode_wasm_scalar(value, crate::WasmType::I64)?
        else {
            unreachable!("decoded i64 operand")
        };
        Ok(value)
    }

    pub(super) fn wasm_f32_operand(&self, value: Value) -> Result<f32, JsError> {
        let crate::WasmValue::F32(bits) = self.decode_wasm_scalar(value, crate::WasmType::F32)?
        else {
            unreachable!("decoded f32 operand")
        };
        Ok(f32::from_bits(bits))
    }

    pub(super) fn wasm_f64_operand(&self, value: Value) -> Result<f64, JsError> {
        let crate::WasmValue::F64(bits) = self.decode_wasm_scalar(value, crate::WasmType::F64)?
        else {
            unreachable!("decoded f64 operand")
        };
        Ok(f64::from_bits(bits))
    }

    pub(super) fn wasm_reference_matches(&self, value: Value, type_code: u32) -> bool {
        let nullable = type_code & 0x4000_0000 != 0;
        let exact = type_code & 0x2000_0000 != 0;
        if value == Value::NULL {
            return nullable;
        }
        if matches!(self.heap.get(value), Some(Cell::WasmBits64(_))) {
            return match type_code & 0x3fff_ffff {
                3 | 7 | 10 => true,
                _ => false,
            };
        }
        let Some(raw) = value.as_int().map(|value| value as u32) else {
            return false;
        };
        if type_code & 0x8000_0000 != 0 {
            let target_type = type_code & 0x1fff_ffff;
            let Some(module) = self.active_wasm_module else {
                let Some(source_type) = self.wasm_gc_ref_types.get(&raw).copied() else {
                    return false;
                };
                return source_type == target_type;
            };
            let Some(target_module) = self.wasm_modules.get(module.raw() as usize) else {
                return false;
            };
            let (source_module_index, source_type) = if let Some(function) =
                self.wasm_function_ref_handles.get(&raw).copied()
            {
                let Some(function_module) = self.wasm_modules.get(function.module.raw() as usize)
                else {
                    return false;
                };
                let Some(source_type) = function_module
                    .function_type_indices
                    .get(function.function_index as usize)
                    .copied()
                else {
                    return false;
                };
                (function.module.raw(), source_type)
            } else if let Some(object) = self.wasm_gc_objects.get(&raw) {
                let (source_module, source_type) = match object {
                    WasmGcObject::Array {
                        module_index,
                        type_index,
                        ..
                    }
                    | WasmGcObject::Struct {
                        module_index,
                        type_index,
                        ..
                    } => (*module_index, *type_index),
                };
                (source_module, source_type)
            } else if raw >> 28 == 0 {
                let Some(source_type) = target_module
                    .function_type_indices
                    .get(raw as usize)
                    .copied()
                else {
                    return false;
                };
                (module.raw(), source_type)
            } else {
                return false;
            };
            let Some(source_module) = self.wasm_modules.get(source_module_index as usize) else {
                return false;
            };
            return if source_module_index == module.raw() {
                if exact {
                    wasm_type_is_exact(source_module, source_type, target_type)
                } else {
                    wasm_type_is_subtype(source_module, source_type, target_type)
                }
            } else if exact {
                wasm_type_is_exact_across_modules(
                    source_module,
                    source_type,
                    target_module,
                    target_type,
                )
            } else {
                wasm_type_is_subtype_across_modules(
                    source_module,
                    source_type,
                    target_module,
                    target_type,
                )
            };
        }
        let class = raw >> 28;
        match type_code & 0x3fff_ffff {
            1 => class == 0,
            2 => matches!(class, 1 | 2),
            3 => matches!(class, 1 | 2 | 4..=7 | 8..=11 | 12..=15),
            4 | 5 | 6 | 11 | 12 | 13 | 14 => false,
            7 => matches!(class, 4..=7 | 8..=11 | 12..=15),
            8 => matches!(class, 8..=11),
            9 => matches!(class, 12..=15),
            10 => matches!(class, 4..=7),
            _ => false,
        }
    }

    pub(super) fn encode_wasm_scalar(&mut self, value: crate::WasmValue) -> Value {
        if let crate::WasmValue::ExnRef(reference) = value {
            return reference.unwrap_or(Value::NULL);
        }
        if let crate::WasmValue::I31Ref(reference) = value {
            return reference.map_or(Value::NULL, |bits| {
                self.heap
                    .alloc(Cell::WasmBits64(u64::from(bits & 0x7fff_ffff)))
            });
        }
        if let crate::WasmValue::FuncRef(reference) = value {
            return reference.map_or(Value::NULL, |index| Value::integer(index as i32));
        }
        if let crate::WasmValue::ExternRef(reference) = value {
            return reference.map_or(Value::NULL, |index| {
                let tagged = if index >> 28 == 0 {
                    crate::wasm::WASM_EXTERNREF_TAG | index
                } else {
                    index
                };
                Value::integer(tagged as i32)
            });
        }
        match value.bits() {
            crate::wasm::ScalarBits::Bits32(bits) => Value::integer(bits as i32),
            crate::wasm::ScalarBits::Bits64(bits) => self.heap.alloc(Cell::WasmBits64(bits)),
            crate::wasm::ScalarBits::Bits128(bits) => self
                .heap
                .alloc(Cell::BigInt(num_bigint::BigInt::from(bits).to_string())),
            crate::wasm::ScalarBits::Reference(_)
            | crate::wasm::ScalarBits::I31Reference(_)
            | crate::wasm::ScalarBits::ExnReference(_) => {
                unreachable!("reference values handled above")
            }
        }
    }

    pub(super) fn decode_wasm_scalar(
        &self,
        value: Value,
        ty: crate::WasmType,
    ) -> Result<crate::WasmValue, JsError> {
        if ty == crate::WasmType::ExnRef {
            if value == Value::NULL {
                return Ok(crate::WasmValue::ExnRef(None));
            }
            if matches!(self.heap.get(value), Some(Cell::WasmExceptionRef { .. })) {
                return Ok(crate::WasmValue::ExnRef(Some(value)));
            }
            return Err(JsError::validation(
                "invalid Wasm exception reference representation".into(),
            ));
        }
        if ty == crate::WasmType::V128 {
            let Some(Cell::BigInt(value)) = self.heap.get(value) else {
                return Err(JsError::validation(
                    "invalid Wasm v128 representation".into(),
                ));
            };
            let value = value
                .parse::<num_bigint::BigInt>()
                .map_err(|_| JsError::validation("invalid Wasm v128 representation".into()))?;
            let (sign, bytes) = value.to_bytes_le();
            if sign == num_bigint::Sign::Minus || bytes.len() > 16 {
                return Err(JsError::validation(
                    "invalid Wasm v128 representation".into(),
                ));
            }
            let mut bits = [0u8; 16];
            bits[..bytes.len()].copy_from_slice(&bytes);
            return Ok(crate::WasmValue::V128(u128::from_le_bytes(bits)));
        }
        if ty == crate::WasmType::I31Ref {
            let reference = if value == Value::NULL {
                None
            } else {
                let Some(Cell::WasmBits64(bits)) = self.heap.get(value) else {
                    return Err(JsError::validation("invalid Wasm i31 reference".into()));
                };
                Some(*bits as u32 & 0x7fff_ffff)
            };
            return ty
                .decode(crate::wasm::ScalarBits::I31Reference(reference))
                .ok_or_else(|| JsError::validation("invalid Wasm i31 reference type".into()));
        }
        if matches!(ty, crate::WasmType::FuncRef | crate::WasmType::ExternRef) {
            if ty == crate::WasmType::ExternRef
                && let Some(Cell::WasmBits64(bits)) = self.heap.get(value)
            {
                return Ok(crate::WasmValue::I31Ref(Some(*bits as u32 & 0x7fff_ffff)));
            }
            let reference = if value == Value::NULL {
                None
            } else {
                let raw = value.as_int().ok_or_else(|| {
                    JsError::validation("invalid Wasm reference representation".into())
                })? as u32;
                if ty == crate::WasmType::ExternRef
                    && matches!(
                        raw >> crate::wasm::WASM_GC_REFERENCE_CLASS_SHIFT,
                        crate::wasm::WASM_GC_REFERENCE_STRUCT_CLASS
                            | crate::wasm::WASM_GC_REFERENCE_ARRAY_CLASS
                    )
                {
                    return Ok(crate::WasmValue::FuncRef(Some(raw)));
                }
                Some(
                    if ty == crate::WasmType::ExternRef && matches!(raw >> 28, 1 | 2) {
                        raw & crate::wasm::WASM_REFERENCE_HANDLE_MASK
                    } else {
                        raw
                    },
                )
            };
            return ty
                .decode(crate::wasm::ScalarBits::Reference(reference))
                .ok_or_else(|| JsError::validation("invalid Wasm reference type".into()));
        }
        let bits = if let Some(bits) = value.as_int() {
            Some(crate::wasm::ScalarBits::Bits32(bits as u32))
        } else if let Some(Cell::WasmBits64(bits)) = self.heap.get(value) {
            Some(crate::wasm::ScalarBits::Bits64(*bits))
        } else {
            None
        };
        bits.and_then(|bits| ty.decode(bits))
            .ok_or_else(|| JsError::validation("invalid Wasm scalar representation".into()))
    }

    pub(super) fn wasm_function_type_matches(
        &self,
        module: crate::WasmModuleId,
        function_index: u32,
        expected_type: u32,
    ) -> Option<bool> {
        let stored = self.wasm_modules.get(module.raw() as usize)?;
        let actual_type = stored
            .function_type_indices
            .get(function_index as usize)
            .copied()?;
        Some(wasm_type_is_subtype(stored, actual_type, expected_type))
    }
}

fn wasm_type_is_subtype(stored: &StoredWasmModule, source_type: u32, target_type: u32) -> bool {
    let canonical = |index: u32| {
        stored
            .gc_type_canonicals
            .get(index as usize)
            .copied()
            .unwrap_or(index)
    };
    let target = canonical(target_type);
    let mut current = Some(source_type);
    while let Some(index) = current {
        if canonical(index) == target {
            return true;
        }
        current = stored.gc_supertypes.get(index as usize).copied().flatten();
    }
    false
}

fn wasm_type_is_exact(stored: &StoredWasmModule, source_type: u32, target_type: u32) -> bool {
    let canonical = |index: u32| {
        stored
            .gc_type_canonicals
            .get(index as usize)
            .copied()
            .unwrap_or(index)
    };
    canonical(source_type) == canonical(target_type)
}

fn wasm_type_is_exact_across_modules(
    source_module: &StoredWasmModule,
    source_type: u32,
    target_module: &StoredWasmModule,
    target_type: u32,
) -> bool {
    source_module.gc_type_fingerprints.get(source_type as usize)
        == target_module.gc_type_fingerprints.get(target_type as usize)
}

fn wasm_type_is_subtype_across_modules(
    source_module: &StoredWasmModule,
    source_type: u32,
    target_module: &StoredWasmModule,
    target_type: u32,
) -> bool {
    let Some(target) = target_module.gc_type_fingerprints.get(target_type as usize) else {
        return false;
    };
    let mut current = Some(source_type);
    while let Some(index) = current {
        if source_module.gc_type_fingerprints.get(index as usize) == Some(target) {
            return true;
        }
        current = source_module
            .gc_supertypes
            .get(index as usize)
            .copied()
            .flatten();
    }
    false
}

fn wasm_reference_type_assignable(source: crate::WasmType, destination: crate::WasmType) -> bool {
    source == destination
        || (source == crate::WasmType::I31Ref && destination == crate::WasmType::ExternRef)
}

fn wasm_value_assignable_to(value: crate::WasmValue, destination: crate::WasmType) -> bool {
    value.ty() == destination
        || (matches!(
            value,
            crate::WasmValue::FuncRef(None)
                | crate::WasmValue::ExternRef(None)
                | crate::WasmValue::I31Ref(None)
                | crate::WasmValue::ExnRef(None)
        ) && matches!(
            destination,
            crate::WasmType::FuncRef
                | crate::WasmType::ExternRef
                | crate::WasmType::I31Ref
                | crate::WasmType::ExnRef
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasmparser::Operator;

    #[test]
    fn unreachable_trap_unwinds_the_shared_activation() {
        let function = crate::Engine::lower_wasm_i32_function(
            "unreachable-unwind",
            0,
            0,
            true,
            [Operator::Unreachable, Operator::End].into_iter().map(Ok),
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        assert_eq!(
            vm.execute_wasm_i32(&function, &[]).unwrap_err().wasm_trap(),
            Some(crate::WasmTrap::Unreachable)
        );
        assert!(vm.frames.is_empty());
        vm.collect_now(function.residual());
    }

    #[test]
    fn nested_call_trap_unwinds_every_shared_activation() {
        let function = crate::Engine::lower_wasm_i32_module(
            "nested-unwind",
            1,
            [
                (0, 0, true, vec![Operator::Unreachable, Operator::End]),
                (
                    0,
                    0,
                    true,
                    vec![Operator::Call { function_index: 0 }, Operator::End],
                ),
            ]
            .into_iter()
            .map(|(params, locals, result, ops)| (params, locals, result, ops.into_iter().map(Ok))),
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        assert_eq!(
            vm.execute_wasm_i32(&function, &[]).unwrap_err().wasm_trap(),
            Some(crate::WasmTrap::Unreachable)
        );
        assert!(vm.frames.is_empty());
        vm.collect_now(function.residual());
    }

    #[test]
    fn integer_traps_unwind_the_shared_activation() {
        let operators = [
            Operator::LocalGet { local_index: 0 },
            Operator::LocalGet { local_index: 1 },
            Operator::I32DivS,
            Operator::End,
        ];
        let function = crate::Engine::lower_wasm_i32_function(
            "trap-unwind",
            2,
            0,
            true,
            operators.into_iter().map(Ok),
        )
        .unwrap();
        let mut vm = Vm::new(crate::SystemHost);
        for args in [[1, 0], [i32::MIN, -1]] {
            assert!(
                vm.execute_wasm_i32(&function, &args)
                    .unwrap_err()
                    .wasm_trap()
                    .is_some()
            );
            assert!(vm.frames.is_empty());
            vm.collect_now(function.residual());
        }
        assert_eq!(vm.execute_wasm_i32(&function, &[7, 2]).unwrap(), Some(3));
        assert!(vm.frames.is_empty());
    }

    #[test]
    fn installed_wasm_module_runs_repeatedly_without_resetting_the_shared_heap() {
        let mut runtime = crate::Runtime::new(crate::SystemHost);
        let program = crate::Engine::specialize("({ marker: 42 })", "shared-root.js").unwrap();
        let root = runtime.execute_rooted(&program).unwrap();
        let rooted_value = runtime.rooted_value(root).unwrap();
        let function = crate::Engine::lower_wasm_i32_function(
            "persistent-module",
            1,
            0,
            true,
            [
                Operator::LocalGet { local_index: 0 },
                Operator::I32Const { value: 1 },
                Operator::I32Add,
                Operator::End,
            ]
            .into_iter()
            .map(Ok),
        )
        .unwrap();

        let module = runtime.install_wasm_module(&function).unwrap();
        assert_eq!(runtime.rooted_value(root), Some(rooted_value));
        assert_eq!(
            runtime
                .invoke_wasm_function(module, 0, &[crate::WasmValue::I32(41)])
                .unwrap(),
            vec![crate::WasmValue::I32(42)]
        );
        assert_eq!(
            runtime
                .invoke_wasm_function(module, 0, &[crate::WasmValue::I32(9)])
                .unwrap(),
            vec![crate::WasmValue::I32(10)]
        );
        assert_eq!(runtime.rooted_value(root), Some(rooted_value));
    }
}
