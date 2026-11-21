//! In-place fast paths for `%TypedArray%.prototype.at`, `indexOf`, `includes` and `fill`: each
//! completes the call with no guest code and no raised error, or answers `None` so the general
//! native runs. Every path reads or writes a fixed-length view of an attached, non-resizable
//! buffer, whose length cannot change during the call.
use super::super::typed_array_access::TypedArrayElement;
use super::super::*;

/// The `indexOf` result when no element matches.
const NOT_FOUND: f64 = -1.0;

/// A typed array a plain path reads or writes: its kind, its buffer, the byte offset of its
/// first element and its element count.
#[derive(Clone, Copy)]
struct PlainView {
    kind: TypedArrayKind,
    buffer: Value,
    offset: usize,
    length: usize,
}

impl PlainView {
    /// The byte range the view's elements occupy in its buffer's store.
    fn span(self) -> std::ops::Range<usize> {
        let end = self.offset.saturating_add(self.length.saturating_mul(self.kind.width()));
        self.offset..end
    }
}

/// The two searches `plain_typed_search` runs: `indexOf` matches by strict equality, `includes`
/// by SameValueZero, under which a NaN matches a NaN.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Search {
    IndexOf,
    Includes,
}

impl Vm {
    /// `%TypedArray%.prototype.at` (`Native::TypedArrayAt`): the element at an int32 index, which
    /// a negative index counts from the end, as a Number; `undefined` past either end. `None`
    /// for a BigInt kind or an index that is not an int32.
    pub(in crate::vm) fn plain_typed_at(&mut self, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
        if native != Native::TypedArrayAt {
            return None;
        }
        let view = self.plain_typed_view(receiver)?;
        let index = args.first()?.as_int()?;
        match crate::staged::typed_elements::element_at(f64::from(index), view.length) {
            Some(position) => self.typed_array_number_element(receiver, position),
            None => Some(Value::UNDEFINED),
        }
    }

    /// `%TypedArray%.prototype.indexOf` (`Native::Uint8ArrayIndexOf`): the first index at or after
    /// `fromIndex` whose element is strictly equal to the Number search value, or -1. Uint8,
    /// Int32 and Float64 views only; a NaN search never matches.
    pub(in crate::vm) fn plain_typed_index_of(&mut self, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
        if native != Native::Uint8ArrayIndexOf {
            return None;
        }
        self.plain_typed_search(Search::IndexOf, receiver, args)
    }

    /// `%TypedArray%.prototype.includes` (`Native::Uint8ArrayIncludes`): as `plain_typed_index_of`
    /// but by SameValueZero, so a NaN search matches a NaN element of a Float64 view.
    pub(in crate::vm) fn plain_typed_includes(&mut self, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
        if native != Native::Uint8ArrayIncludes {
            return None;
        }
        self.plain_typed_search(Search::Includes, receiver, args)
    }

    /// `%TypedArray%.prototype.fill` (`Native::Uint8ArrayFill`): a Number value written over the
    /// range `[start, end)`, whose bounds are absent, undefined or int32 and clamped as `fill`
    /// clamps them. The element conversion is the kind's (`typed_array_element_bytes`: ToUint8,
    /// ToUint8Clamp, ToInt32 and so on). Returns the receiver. `None` for a BigInt kind, a value
    /// that is not a Number, an immutable buffer or a shared one.
    pub(in crate::vm) fn plain_typed_fill(&mut self, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
        if native != Native::Uint8ArrayFill {
            return None;
        }
        let view = self.plain_typed_view(receiver)?;
        if matches!(self.heap.get(view.buffer), Some(Cell::ArrayBuffer { immutable: true, .. })) {
            return None;
        }
        let mut operands = args.iter();
        let number = operands.next()?.as_number()?;
        let (start, end) = fill_range(operands.next(), operands.next(), view.length)?;
        self.typed_array_fill_elements(receiver, start, end, &TypedArrayElement::Number(number))
            .then_some(receiver)
    }

    /// The answer `indexOf` and `includes` share: the search over the owned bytes of the view,
    /// from `fromIndex`. `None` for a receiver, element kind, search value or `fromIndex` the
    /// plain path does not read.
    fn plain_typed_search(&self, mode: Search, receiver: Value, args: &[Value]) -> Option<Value> {
        let view = self.plain_typed_view(receiver)?;
        if !is_searchable(view.kind) {
            return None;
        }
        let search = args.first()?.as_number()?;
        let from = relative_bound(args.get(1), 0, view.length)?;
        let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get(view.buffer) else {
            return None;
        };
        let elements = bytes.owned()?.get(view.span())?;
        let found = crate::staged::typed_elements::find_number_element(elements, view.kind, from, search, mode == Search::Includes);
        Some(match mode {
            Search::IndexOf => Value::number(found.map_or(NOT_FOUND, |index| index as f64)),
            Search::Includes => Value::boolean(found.is_some()),
        })
    }

    /// The view a plain path reads: a non-BigInt, fixed-length view (not length-tracking) of a
    /// buffer that is attached and not resizable. `None` otherwise.
    fn plain_typed_view(&self, receiver: Value) -> Option<PlainView> {
        let Some(Cell::TypedArray { object, buffer, length }) = self.heap.get(receiver) else {
            return None;
        };
        let kind = object.native.kind();
        if kind.is_bigint() {
            return None;
        }
        let length = length.get()?;
        let Some(Cell::ArrayBuffer { detached: false, resizable: false, .. }) = self.heap.get(*buffer) else {
            return None;
        };
        Some(PlainView { kind, buffer: *buffer, offset: object.offset, length })
    }
}

/// Whether the elements of `kind` are searched in place: Uint8, Int32 and Float64 only.
fn is_searchable(kind: TypedArrayKind) -> bool {
    matches!(kind, TypedArrayKind::Uint8 | TypedArrayKind::Int32 | TypedArrayKind::Float64)
}

/// A bound of `fill` or of a search, resolved against `length`: `default` when absent or
/// undefined, else the int32 relative index (`crate::units::relative_index`). `None` for any
/// other value, whose ToIntegerOrInfinity may run guest code.
fn relative_bound(bound: Option<&Value>, default: usize, length: usize) -> Option<usize> {
    match bound.copied().filter(|value| !value.is_undefined()) {
        Some(value) => Some(crate::units::relative_index(f64::from(value.as_int()?), length)),
        None => Some(default),
    }
}

/// The `[start, end)` range `fill` writes: `start` from the first bound (default 0) and `end`
/// from the second (default `length`), with `end` never before `start`.
fn fill_range(start: Option<&Value>, end: Option<&Value>, length: usize) -> Option<(usize, usize)> {
    let start = relative_bound(start, 0, length)?;
    let end = relative_bound(end, length, length)?.max(start);
    Some((start, end))
}
