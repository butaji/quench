//! An object's private elements: the fields, methods and accessors its class evaluations
//! branded it with, in the order they were added.

use super::{Object, PrivateBrand, PrivateElement};
use crate::value::Value;

impl Object {
    pub(crate) fn private_names(&self) -> impl Iterator<Item = PrivateBrand> + '_ {
        self.extras()
            .into_iter()
            .flat_map(|extras| extras.private_elements.iter().map(|element| element.brand))
    }
    #[inline(always)]
    pub(crate) fn has_private_name(&self, brand: PrivateBrand) -> bool {
        self.private_position(brand, 0).is_some()
    }
    /// The position of the element carrying `brand`: `hint`'s when it carries it, else the
    /// first that does. A class's instances add their elements in one order, so a site that
    /// passes the position it found last finds it at once.
    #[inline(always)]
    pub(crate) fn private_position(&self, brand: PrivateBrand, hint: usize) -> Option<usize> {
        let elements = &self.extras()?.private_elements;
        if elements.get(hint).is_some_and(|element| element.brand == brand) {
            return Some(hint);
        }
        elements.iter().position(|element| element.brand == brand)
    }
    /// The data of the element at `position` (`private_position`); `None` for a method's or
    /// accessor's brand-only element.
    #[inline(always)]
    pub(crate) fn private_data(&self, position: usize) -> Option<Value> {
        self.extras()?.private_elements.get(position)?.data
    }
    pub(crate) fn add_private_name(&mut self, brand: PrivateBrand) {
        if !self.has_private_name(brand) {
            self.extras_mut()
                .private_elements
                .push(PrivateElement { brand, data: None });
        }
    }
    #[inline(always)]
    pub(crate) fn private_field(&self, brand: PrivateBrand) -> Option<Value> {
        self.private_data(self.private_position(brand, 0)?)
    }
    /// Stores `value` into the initialized field at `position` (`private_position`); `false`
    /// for a brand-only element, which no store reaches.
    #[inline(always)]
    pub(crate) fn set_private_data(&mut self, position: usize, value: Value) -> bool {
        let Some(element) = self.extras_mut_existing().and_then(|extras| extras.private_elements.get_mut(position)).filter(|element| element.data.is_some()) else { return false; };
        element.data = Some(value);
        true
    }
    #[inline(always)]
    pub(crate) fn set_private_field(&mut self, brand: PrivateBrand, value: Value) -> bool {
        self.private_position(brand, 0).is_some_and(|position| self.set_private_data(position, value))
    }
    #[inline(always)] pub(crate) fn initialize_private_field(&mut self, brand: PrivateBrand, value: Value) -> bool {
        let elements = &mut self.extras_mut().private_elements;
        if let Some(element) = elements.iter_mut().find(|element| element.brand == brand) {
            if element.data.is_some() {
                return false;
            }
            element.data = Some(value);
        } else {
            elements.push(PrivateElement {
                brand,
                data: Some(value),
            });
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_positions_follow_insertion_order_and_any_hint_finds_them() {
        use super::PrivateBrand;
        let home = Value::integer(1);
        let brand = |name| PrivateBrand { home, name };
        let mut object = Object::new(Value::NULL);
        assert_eq!(object.private_position(brand(0), 0), None);
        for name in 0..4 {
            assert!(object.initialize_private_field(brand(name), Value::integer(10 + name as i32)));
        }
        object.add_private_name(brand(9));
        for name in 0..4 {
            for hint in [0, 1, 3, 200] {
                assert_eq!(object.private_position(brand(name), hint), Some(name as usize));
            }
            assert_eq!(object.private_field(brand(name)), Some(Value::integer(10 + name as i32)));
        }
        // A brand-only element has a position and no data, and no store reaches it.
        assert_eq!(object.private_position(brand(9), 2), Some(4));
        assert_eq!(object.private_data(4), None);
        assert!(!object.set_private_data(4, Value::integer(0)));
        // Another home's brand of the same name is another element.
        let other = PrivateBrand { home: Value::integer(2), name: 0 };
        assert_eq!(object.private_position(other, 0), None);
        assert!(object.set_private_data(2, Value::integer(77)));
        assert_eq!(object.private_field(brand(2)), Some(Value::integer(77)));
        assert!(object.set_private_field(brand(3), Value::integer(88)));
        assert_eq!(object.private_data(3), Some(Value::integer(88)));
        assert!(!object.set_private_field(other, Value::integer(0)));
    }
}
