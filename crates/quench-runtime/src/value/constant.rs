impl From<&Constant> for Value {
    fn from(value: &Constant) -> Self {
        match value {
            Constant::Number(value) => Self::Number(*value),
            Constant::Boolean(value) => Self::Boolean(*value),
            Constant::String(value) => Self::String(value.clone()),
            Constant::StringUnits(value) => Self::StringUnits(std::rc::Rc::new(crate::value::StringUnitsData::new(value.clone()))),
            Constant::BigInt(value) => Self::BigInt(value.clone()),
            Constant::Null => Self::Null,
            Constant::Undefined => Self::Undefined,
        }
    }
}
