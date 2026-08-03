use crate::Context;

#[test]
fn null_member_access_throws_type_error() {
    assert!(Context::new().unwrap().eval("null.x").is_err());
}

#[test]
fn undefined_member_access_throws_type_error() {
    assert!(Context::new().unwrap().eval("undefined.x").is_err());
}
