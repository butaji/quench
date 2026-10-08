use std::ops::Index;
use std::rc::Rc;

#[derive(Clone, Debug, Default)]
pub(crate) struct AtomTable {
    text: Rc<str>,
    ends: Rc<[u32]>,
}

impl AtomTable {
    pub(crate) fn from_rcs(values: &[Rc<str>]) -> Self {
        let bytes = values.iter().map(|value| value.len()).sum();
        let mut text = String::with_capacity(bytes);
        let mut ends = Vec::with_capacity(values.len());
        for value in values {
            text.push_str(value);
            ends.push(text.len() as u32);
        }
        Self::new(text, ends)
    }

    pub(crate) fn new(text: String, ends: Vec<u32>) -> Self {
        Self {
            text: Rc::from(text),
            ends: Rc::from(ends),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.ends.len()
    }

    pub(crate) fn iter(&self) -> impl ExactSizeIterator<Item = &str> {
        (0..self.len()).map(|index| &self[index])
    }
}

impl Index<usize> for AtomTable {
    type Output = str;

    fn index(&self, index: usize) -> &Self::Output {
        let end = self.ends[index] as usize;
        let start = index
            .checked_sub(1)
            .map_or(0, |previous| self.ends[previous] as usize);
        &self.text[start..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_atoms_preserve_empty_and_unicode_boundaries() {
        let values = [Rc::from("alpha"), Rc::from(""), Rc::from("λ")];
        let table = AtomTable::from_rcs(&values);
        assert_eq!(table.iter().collect::<Vec<_>>(), ["alpha", "", "λ"]);
        let cloned = table.clone();
        assert_eq!(&cloned[2], "λ");
    }
}
