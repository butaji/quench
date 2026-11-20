//! `ByteLengthQueuingStrategy` and `CountQueuingStrategy`
//! (`lib/internal/webstreams/queuingstrategies.js`).

use super::readable::field::HIGH_WATER_MARK;
use super::{
    arg, attach, class, custom_inspect, get, install, is_undefined, literal, put, put_num, record, throw,
    validate_dictionary, Brand, ClassMember, Context, Dictionary, Done, Outcome, WebError,
};
use crate::modules::host_members::{current, function, members};
use quench_runtime::RootId;

members!(Member, "webStrategy", [
    ByteLengthConstruct,
    CountConstruct,
    HighWaterMark,
    Size,
    ByteLengthInspect,
    CountInspect,
    ByteSize,
    CountSize,
]);

/// What reading a strategy's `#state` from another object throws.
const FOREIGN_RECEIVER: &str = "Cannot read private member #state from an object whose class did not declare it";

/// The table members holding the shared `size` functions.
const BYTE_SIZE: &str = "byteSize";
const COUNT_SIZE: &str = "countSize";

const MEMBERS: [ClassMember<Member>; 2] =
    [ClassMember::Getter("highWaterMark", Member::HighWaterMark), ClassMember::Getter("size", Member::Size)];

pub(super) fn build(c: &mut Context<'_>, table: RootId) -> Done {
    for (brand, construct, inspect, size, size_member, length) in [
        (Brand::ByteLengthStrategy, Member::ByteLengthConstruct, Member::ByteLengthInspect, BYTE_SIZE, Member::ByteSize, 1),
        (Brand::CountStrategy, Member::CountConstruct, Member::CountInspect, COUNT_SIZE, Member::CountSize, 0),
    ] {
        let constructor = class(c, brand, construct, 1, inspect)?;
        let prototype = get(c, constructor, "prototype")?;
        install(c, prototype, &MEMBERS)?;
        put(c, table, brand.name(), constructor)?;
        let function = function(c, size_member, "size", &[])?;
        super::set_length(c, function, length)?;
        put(c, table, size, function)?;
    }
    Ok(())
}

fn construct(c: &mut Context<'_>, this: RootId, args: &[RootId], brand: Brand) -> Outcome {
    let init = arg(c, args, 0);
    validate_dictionary(c, init, "init", Dictionary::Plain)?;
    let high_water_mark = get(c, init, HIGH_WATER_MARK)?;
    if is_undefined(c, high_water_mark) {
        return Err(throw(c, WebError::MissingOption("init.highWaterMark")));
    }
    let high_water_mark = c.to_number(high_water_mark)?;
    let state = record(c, &[], &[])?;
    put_num(c, state, HIGH_WATER_MARK, high_water_mark)?;
    attach(c, this, brand, state)?;
    Ok(c.undefined())
}

/// The strategy's brand and slots, for either class.
fn strategy(c: &mut Context<'_>, this: RootId) -> Result<(Brand, RootId), quench_runtime::RootedError> {
    if let Some(state) = super::state_of(c, this, Brand::ByteLengthStrategy)? {
        return Ok((Brand::ByteLengthStrategy, state));
    }
    match super::state_of(c, this, Brand::CountStrategy)? {
        Some(state) => Ok((Brand::CountStrategy, state)),
        None => {
            let error = c.type_error_rooted(FOREIGN_RECEIVER)?;
            Err(c.throw(error))
        }
    }
}

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let member: Member = current(c)?;
    match member {
        Member::ByteLengthConstruct => {
            super::require_new(c, Brand::ByteLengthStrategy.name())?;
            construct(c, this, args, Brand::ByteLengthStrategy)
        }
        Member::CountConstruct => {
            super::require_new(c, Brand::CountStrategy.name())?;
            construct(c, this, args, Brand::CountStrategy)
        }
        Member::HighWaterMark => {
            let (_, state) = strategy(c, this)?;
            get(c, state, HIGH_WATER_MARK)
        }
        Member::Size => {
            let (brand, _) = strategy(c, this)?;
            let table = super::classes(c)?;
            get(c, table, if brand == Brand::ByteLengthStrategy { BYTE_SIZE } else { COUNT_SIZE })
        }
        Member::ByteLengthInspect | Member::CountInspect => {
            let (brand, state) = strategy(c, this)?;
            let high_water_mark = get(c, state, HIGH_WATER_MARK)?;
            let data = literal(c, &["highWaterMark"], &[high_water_mark])?;
            custom_inspect(c, this, brand.name(), data, args)
        }
        Member::ByteSize => {
            let chunk = arg(c, args, 0);
            get(c, chunk, "byteLength")
        }
        Member::CountSize => Ok(c.number(1.0)),
    }
}
