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

impl Snap for ShapeTransition {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        match *self {
            Self::Root => writer.u8(transition_tag::ROOT),
            Self::FunctionName { name } => {
                writer.u8(transition_tag::FUNCTION_NAME);
                writer.u32(name);
            }
            Self::FunctionNameReconfigured => writer.u8(transition_tag::FUNCTION_NAME_RECONFIGURED),
            Self::FunctionLengthReconfigured => writer.u8(transition_tag::FUNCTION_LENGTH_RECONFIGURED),
            Self::Constructed { constructor, new_target_is_base } => {
                writer.u8(transition_tag::CONSTRUCTED);
                writer.value(constructor);
                writer.bool(new_target_is_base);
            }
            Self::Add { key, slot, attributes } => {
                writer.u8(transition_tag::ADD);
                key.snap(writer)?;
                writer.u32(slot);
                attributes.snap(writer)?;
            }
            Self::Delete { key, slot } => {
                writer.u8(transition_tag::DELETE);
                key.snap(writer)?;
                writer.u32(slot);
            }
            Self::Vacant => writer.u8(transition_tag::VACANT),
            Self::Descriptor { slot, attributes } => {
                writer.u8(transition_tag::DESCRIPTOR);
                writer.u32(slot);
                attributes.snap(writer)?;
            }
            Self::Dictionary { trigger } => {
                writer.u8(transition_tag::DICTIONARY);
                trigger.snap(writer)?;
            }
        }
        Ok(())
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Ok(match reader.u8()? {
            transition_tag::ROOT => Self::Root,
            transition_tag::FUNCTION_NAME => Self::FunctionName { name: reader.u32()? },
            transition_tag::FUNCTION_NAME_RECONFIGURED => Self::FunctionNameReconfigured,
            transition_tag::FUNCTION_LENGTH_RECONFIGURED => Self::FunctionLengthReconfigured,
            transition_tag::CONSTRUCTED => Self::Constructed {
                constructor: reader.value()?,
                new_target_is_base: reader.bool()?,
            },
            transition_tag::VACANT => Self::Vacant,
            tag => return Self::unsnap_keyed(tag, reader),
        })
    }
}

impl ShapeTransition {
    /// The transition of `tag` that names a property or a dictionary trigger.
    fn unsnap_keyed(tag: u8, reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        Ok(match tag {
            transition_tag::ADD => Self::Add {
                key: Snap::unsnap(reader)?,
                slot: reader.u32()?,
                attributes: Snap::unsnap(reader)?,
            },
            transition_tag::DELETE => Self::Delete { key: Snap::unsnap(reader)?, slot: reader.u32()? },
            transition_tag::DESCRIPTOR => Self::Descriptor { slot: reader.u32()?, attributes: Snap::unsnap(reader)? },
            transition_tag::DICTIONARY => Self::Dictionary { trigger: Snap::unsnap(reader)? },
            _ => return Err(SnapshotError("invalid snapshot shape transition".into())),
        })
    }
}
