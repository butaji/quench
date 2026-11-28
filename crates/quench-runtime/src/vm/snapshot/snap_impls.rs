//! How the VM's identity types are written to and read from a snapshot.
use super::*;

impl Snap for Native {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        self.write_snapshot(writer);
        Ok(())
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Self::read_snapshot(reader)
    }
}

impl Snap for WellKnownSymbols {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        self.0.snap(writer)
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Ok(Self(Snap::unsnap(reader)?))
    }
}

impl Snap for ProgramId {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        writer.u32(self.raw());
        Ok(())
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Ok(Self::from_raw(reader.u32()?))
    }
}

impl Snap for JsString {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        self.write_snapshot(writer);
        Ok(())
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Self::read_snapshot(reader)
    }
}

/// `PropertyKey` tags, in declaration order.
mod property_key_tag {
    pub(super) const STRING: u8 = 0;
    pub(super) const SYMBOL: u8 = 1;
    pub(super) const PRIVATE: u8 = 2;
}

impl Snap for property_key::PropertyKey {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        match *self {
            Self::String(atom) => {
                writer.u8(property_key_tag::STRING);
                writer.u32(atom);
            }
            Self::Symbol(symbol) => {
                writer.u8(property_key_tag::SYMBOL);
                writer.value(symbol);
            }
            Self::Private(home, atom) => {
                writer.u8(property_key_tag::PRIVATE);
                writer.value(home);
                writer.u32(atom);
            }
        }
        Ok(())
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Ok(match reader.u8()? {
            property_key_tag::STRING => Self::String(reader.u32()?),
            property_key_tag::SYMBOL => Self::Symbol(reader.value()?),
            property_key_tag::PRIVATE => Self::Private(reader.value()?, reader.u32()?),
            _ => return Err(SnapshotError("invalid snapshot property key".into())),
        })
    }
}

crate::snapshot::snap_struct!(PropertyAttributes { [writable, enumerable, configurable, accessor]; getter, setter });
crate::snapshot::snap_struct!(ShapeAttributes { [writable, enumerable, configurable, accessor]; accessors });
crate::snapshot::snap_struct!(DataAttributes { [writable, enumerable, configurable]; });
crate::snapshot::snap_struct!(GlobalLexicalState { declarations, bindings, immutable_bindings });
crate::snapshot::snap_struct!(ParkedRealm { global, state, prototypes, promise_constructor });
crate::snapshot::snap_struct!(ArgumentsObjectTemplate { anchor, length_slot, callee_slot, iterator_slot });
crate::snapshot::snap_struct!(IteratorRealmPrototypes { helper, wrapper, generator, async_generator });
crate::snapshot::snap_struct!(ErrorRealmIntrinsics {
    host_formatter, constructor, stack_getter, stack_setter, call_site_constructor, call_site_prototype,
});
crate::snapshot::snap_struct!(PrivateHomeKey { environment, program, function, name });
crate::snapshot::snap_struct!(RuntimeAtoms {
    lexical_this, new_target, regexp_exec, regexp_flags, last_index, regexp_flag_accessors, then,
    constructor, value_of, to_string, next, name, empty, value, done, iterator_return, writable, enumerable, configurable,
    get, set,
    bound_target, bound_this, bound_args, proxy_traps,
});
crate::snapshot::snap_struct!(RealmIntrinsics {
    arguments_objects, iterator_prototypes, builtin_prototypes, function_constructors,
    promise_constructors, error_intrinsics, regexp_intrinsics, intl_number_format_constructors,
    intl_number_format_prototypes, intl_number_format_fallback_symbols, intl_collator_constructors,
    intl_collator_prototypes, intl_plural_rules_prototypes, intl_datetime_format_constructors,
    intl_datetime_format_prototypes, intl_datetime_format_fallback_symbols,
    intl_display_names_constructors, intl_display_names_prototypes,
    intl_duration_format_constructors, intl_duration_format_prototypes,
    intl_list_format_constructors, intl_list_format_prototypes,
    intl_relative_time_format_prototypes, intl_segmenter_prototypes,
    intl_segment_iterator_prototypes, intl_segments_prototypes, intl_locale_prototypes,
});

crate::snapshot::snapshot_enum!(dictionary_trigger: DictionaryTrigger {
    PropertyCount, DeletionPattern, PrototypeUse, ModuleNamespace,
});

impl Snap for DictionaryTrigger {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        dictionary_trigger::write(writer, *self);
        Ok(())
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        dictionary_trigger::read(reader)
    }
}

/// `ShapeTransition` tags, in declaration order.
mod transition_tag {
    pub(super) const ROOT: u8 = 0;
    pub(super) const FUNCTION_NAME: u8 = 1;
    pub(super) const FUNCTION_NAME_RECONFIGURED: u8 = 2;
    pub(super) const FUNCTION_LENGTH_RECONFIGURED: u8 = 3;
    pub(super) const CONSTRUCTED: u8 = 4;
    pub(super) const ADD: u8 = 5;
    pub(super) const DELETE: u8 = 6;
    pub(super) const VACANT: u8 = 7;
    pub(super) const DESCRIPTOR: u8 = 8;
    pub(super) const DICTIONARY: u8 = 9;
}

/// The bytes of one shape's record in a snapshot (`shape_record`): fixed-width, so the table
/// reads back as one span with no per-field decoding.
pub(super) const SHAPE_RECORD: usize = 40;

/// `PropertyKey` kinds in a shape record's flag byte, above the attribute bits.
const KEY_SHIFT: u32 = 4;

/// The two words a property key takes in a shape record: its atom and its value.
fn key_words(key: property_key::PropertyKey) -> (u8, u32, u64) {
    match key {
        property_key::PropertyKey::String(atom) => (property_key_tag::STRING, atom, 0),
        property_key::PropertyKey::Symbol(symbol) => (property_key_tag::SYMBOL, 0, symbol.to_bits()),
        property_key::PropertyKey::Private(home, atom) => (property_key_tag::PRIVATE, atom, home.to_bits()),
    }
}

fn key_of_words(kind: u8, atom: u32, value: u64) -> SnapshotResult<property_key::PropertyKey> {
    Ok(match kind {
        property_key_tag::STRING => property_key::PropertyKey::String(atom),
        property_key_tag::SYMBOL => property_key::PropertyKey::Symbol(Value::from_bits(value)),
        property_key_tag::PRIVATE => property_key::PropertyKey::Private(Value::from_bits(value), atom),
        _ => return Err(SnapshotError("invalid snapshot property key".into())),
    })
}

fn data_bits(attributes: DataAttributes) -> u8 {
    u8::from(attributes.writable) | u8::from(attributes.enumerable) << 1 | u8::from(attributes.configurable) << 2
}

fn data_of_bits(bits: u8) -> DataAttributes {
    DataAttributes { writable: bits & 1 != 0, enumerable: bits & 2 != 0, configurable: bits & 4 != 0 }
}

/// Shape `shape` as its record: the parent, storage length, latest child and facts as the
/// table holds them, then the transition's tag, flags and operands. The memo and lookup index
/// are derived again after a restore.
pub(super) fn shape_record(shape: &Shape) -> [u8; SHAPE_RECORD] {
    let (tag, flags, atom, slot, value, extra) = match shape.transition {
        ShapeTransition::Root => (transition_tag::ROOT, 0, 0, 0, 0, 0),
        ShapeTransition::FunctionName { name } => (transition_tag::FUNCTION_NAME, 0, name, 0, 0, 0),
        ShapeTransition::FunctionNameReconfigured => (transition_tag::FUNCTION_NAME_RECONFIGURED, 0, 0, 0, 0, 0),
        ShapeTransition::FunctionLengthReconfigured => (transition_tag::FUNCTION_LENGTH_RECONFIGURED, 0, 0, 0, 0, 0),
        ShapeTransition::Constructed { constructor, class, new_target_is_base } => {
            (transition_tag::CONSTRUCTED, u8::from(new_target_is_base), 0, 0, constructor.to_bits(), class)
        }
        ShapeTransition::Add { key, slot, attributes } => {
            let (kind, atom, value) = key_words(key);
            (transition_tag::ADD, data_bits(attributes) | kind << KEY_SHIFT, atom, slot, value, 0)
        }
        ShapeTransition::Delete { key, slot } => {
            let (kind, atom, value) = key_words(key);
            (transition_tag::DELETE, kind << KEY_SHIFT, atom, slot, value, 0)
        }
        ShapeTransition::Vacant => (transition_tag::VACANT, 0, 0, 0, 0, 0),
        ShapeTransition::Descriptor { slot, attributes } => {
            let bits = u8::from(attributes.writable) | u8::from(attributes.enumerable) << 1
                | u8::from(attributes.configurable) << 2 | u8::from(attributes.accessor) << 3;
            (transition_tag::DESCRIPTOR, bits, 0, slot, 0, attributes.accessors)
        }
        ShapeTransition::Dictionary { trigger } => (transition_tag::DICTIONARY, trigger as u8, 0, 0, 0, 0),
    };
    let mut record = [0; SHAPE_RECORD];
    record[0..4].copy_from_slice(&shape.parent.to_le_bytes());
    record[4..8].copy_from_slice(&shape.storage_len.to_le_bytes());
    record[8..12].copy_from_slice(&shape.last_child.get().to_le_bytes());
    record[12..14].copy_from_slice(&shape.facts.0.to_le_bytes());
    record[14] = tag;
    record[15] = flags;
    record[16..20].copy_from_slice(&atom.to_le_bytes());
    record[20..24].copy_from_slice(&slot.to_le_bytes());
    record[24..32].copy_from_slice(&value.to_le_bytes());
    record[32..36].copy_from_slice(&extra.to_le_bytes());
    record
}

/// The shape `shape_record` wrote.
pub(super) fn shape_of_record(record: &[u8; SHAPE_RECORD]) -> SnapshotResult<Shape> {
    let word = |at: usize| u32::from_le_bytes([record[at], record[at + 1], record[at + 2], record[at + 3]]);
    let (flags, atom, slot, extra) = (record[15], word(16), word(20), word(32));
    let value = u64::from_le_bytes([
        record[24], record[25], record[26], record[27], record[28], record[29], record[30], record[31],
    ]);
    let transition = match record[14] {
        transition_tag::ROOT => ShapeTransition::Root,
        transition_tag::FUNCTION_NAME => ShapeTransition::FunctionName { name: atom },
        transition_tag::FUNCTION_NAME_RECONFIGURED => ShapeTransition::FunctionNameReconfigured,
        transition_tag::FUNCTION_LENGTH_RECONFIGURED => ShapeTransition::FunctionLengthReconfigured,
        transition_tag::CONSTRUCTED => ShapeTransition::Constructed {
            constructor: Value::from_bits(value),
            class: extra,
            new_target_is_base: flags & 1 != 0,
        },
        transition_tag::ADD => ShapeTransition::Add {
            key: key_of_words(flags >> KEY_SHIFT, atom, value)?,
            slot,
            attributes: data_of_bits(flags),
        },
        transition_tag::DELETE => ShapeTransition::Delete { key: key_of_words(flags >> KEY_SHIFT, atom, value)?, slot },
        transition_tag::VACANT => ShapeTransition::Vacant,
        transition_tag::DESCRIPTOR => ShapeTransition::Descriptor {
            slot,
            attributes: ShapeAttributes {
                writable: flags & 1 != 0,
                enumerable: flags & 2 != 0,
                configurable: flags & 4 != 0,
                accessor: flags & 8 != 0,
                accessors: extra,
            },
        },
        transition_tag::DICTIONARY => ShapeTransition::Dictionary {
            trigger: match flags {
                0 => DictionaryTrigger::PropertyCount,
                1 => DictionaryTrigger::DeletionPattern,
                2 => DictionaryTrigger::PrototypeUse,
                3 => DictionaryTrigger::ModuleNamespace,
                _ => return Err(SnapshotError("invalid snapshot dictionary trigger".into())),
            },
        },
        _ => return Err(SnapshotError("invalid snapshot shape transition".into())),
    };
    Ok(Shape {
        parent: word(0),
        transition,
        storage_len: word(4),
        facts: ShapeFacts(u16::from_le_bytes([record[12], record[13]])),
        memo: std::cell::Cell::new(ShapeMemo::INITIAL),
        lookup_index: std::cell::OnceCell::new(),
        last_child: std::cell::Cell::new(word(8)),
    })
}
