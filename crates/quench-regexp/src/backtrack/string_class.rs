//! Classes of strings (`v` flag `\q{...}` and string properties), matched by width.
use super::{class_match_widths, ClassExpr, Text, Unit};

/// A class of strings with the flags that match it and its longest match (`class_max_width`).
pub(super) struct StringClass {
    pub(super) class: ClassExpr,
    pub(super) ignore_case: bool,
    pub(super) unicode: bool,
    pub(super) max_width: usize,
}

impl StringClass {
    /// The units `from..to` of `text` as the lowered matcher's units.
    fn window<T: Text + ?Sized>(text: &T, from: usize, to: usize) -> Vec<Unit> {
        (from..to).map(|index| Unit { value: text.unit(index), offset_start: index, offset_end: index + 1 }).collect()
    }

    /// The positions a match of the class at `position` ends at, longest match first; read
    /// backward, the positions its matches ending at `position` start at.
    pub(super) fn ends<T: Text + ?Sized>(&self, text: &T, position: usize, backward: bool) -> Vec<usize> {
        let widths = |window: &[Unit]| class_match_widths(&self.class, window, 0, self.ignore_case, self.unicode);
        if backward {
            return (1..=self.max_width.min(position))
                .rev()
                .filter(|width| widths(&Self::window(text, position - width, position)).contains(width))
                .map(|width| position - width)
                .collect();
        }
        let window = Self::window(text, position, (position + self.max_width).min(text.len()));
        widths(&window).into_iter().map(|width| position + width).collect()
    }
}
