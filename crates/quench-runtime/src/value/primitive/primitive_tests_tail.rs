use super::*;

#[test]
fn test_to_object_identity_preserved() {
    assert!(matches!(to_object(&obj()), Value::Object(_)));
    let f = Value::Function(crate::value::ValueFunction::new(
        None,
        vec![],
        vec![],
        make_env(),
        false,
        false,
    ));
    assert!(matches!(to_object(&f), Value::Function(_)));
    assert!(matches!(to_object(&nf()), Value::NativeFunction(_)));
    let cls_val = Value::Class(Box::new(ClassValue {
        id: 0,
        name: None,
        constructor_params: vec![],
        constructor_body: vec![],
        has_explicit_constructor: false,
        methods: vec![],
        static_methods: vec![],
        getters: vec![],
        setters: vec![],
        static_getters: vec![],
        static_setters: vec![],
        instance_fields: vec![],
        static_fields: vec![],
        static_blocks: vec![],
        ordered_members: vec![],
        super_class: None,
        super_class_own_proto_cell: Rc::new(RefCell::new(None::<Value>)),
        prototype_cell: Rc::new(RefCell::new(None)),
        static_properties_cell: Rc::new(RefCell::new(HashMap::new())),
        deleted_properties: Rc::new(RefCell::new(HashSet::new())),
        class_def_env_cell: Rc::new(RefCell::new(None)),
        static_getter_keys_cell: Rc::new(RefCell::new(Vec::new())),
        static_setter_keys_cell: Rc::new(RefCell::new(Vec::new())),
        instance_field_keys_cell: Rc::new(RefCell::new(Vec::new())),
        static_field_keys_cell: Rc::new(RefCell::new(Vec::new())),
        extensible_cell: Rc::new(RefCell::new(true)),
        private_element_cache: Rc::new(RefCell::new(HashMap::new())),
        declared_private_names: HashSet::new(),
    }));
    assert!(matches!(to_object(&cls_val), Value::Class(_)));
    let gen_val = Value::Generator(Rc::new(RefCell::new(GeneratorObject::new(
        Rc::new(vec![]),
        vec![],
        make_env(),
        false,
    ))));
    assert!(matches!(to_object(&gen_val), Value::Generator(_)));
}
