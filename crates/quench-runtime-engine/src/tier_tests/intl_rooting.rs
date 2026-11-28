//! The Intl layer's constructors root what they coerce: boxed locales, option views and fresh prototypes survive the callbacks and collections they run (moved from the core's `vm/tests.rs`, whose binary does not link the layers).
use crate::engine_prelude::*;
use quench_runtime::test_host::TestHost;
use quench_runtime::Engine;
use quench_runtime_intl::intl_collator::IntlCollatorVm;
use quench_runtime_intl::intl_datetime::IntlDatetimeVm;
use quench_runtime_intl::intl_display_names::IntlDisplayNamesVm;
use quench_runtime_intl::intl_duration_format::IntlDurationFormatVm;
use quench_runtime_intl::intl_namespace::IntlNamespaceVm;
use quench_runtime_intl::intl_number::IntlNumberVm;
use quench_runtime_intl::intl_plural_rules::IntlPluralRulesVm;
use quench_runtime_intl::intl_relative::IntlRelativeVm;

#[test]
fn collator_construction_roots_boxed_locale_and_option_views() {
    let compile = Engine::specialize;
    for phase in [
        "usage-string",
        "usage",
        "length",
        "index",
        "locale",
        "localeMatcher",
        "collation",
        "numeric",
        "caseFirst",
        "sensitivity",
        "ignorePunctuation",
    ] {
        for abrupt in [false, true] {
            let source = format!(
                r#"
            var trace=[];function traceLog(){{return trace.join(',');}}
            function hit(name){{trace.push(name);if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
            function localeValue(){{return {{toString(){{hit('locale');return 'en';}}}};}}
            function usageValue(){{return {{toString(){{hit('usage-string');return 'sort';}}}};}}
            Object.defineProperty(Number.prototype,'length',{{configurable:true,get(){{hit('length');return 1;}}}});
            Object.defineProperty(Number.prototype,'0',{{configurable:true,get(){{hit('index');return localeValue();}}}});
            for(var key of ['usage','localeMatcher','collation','numeric','caseFirst','sensitivity','ignorePunctuation']){{
                ((name)=>Object.defineProperty(Number.prototype,name,{{configurable:true,get(){{hit(name);if(name==='usage')return usageValue();return undefined;}}}}))(key);
            }}
            "#
            );
            let program = compile(&source, "collator-boxed-roots.js").unwrap();
            let mut vm = quench_runtime::test_support::vm(TestHost::default().with_test262());
            quench_runtime::test_support::execute(&mut vm, &program).unwrap();
            let constructor = *vm
                .realm
                .intrinsics
                .intl_collator_constructors
                .get(&vm.realm.globals)
                .unwrap();
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let locales = if matches!(phase, "length" | "index" | "locale") {
                Value::number(42.0)
            } else {
                vm.heap.alloc_string("en".into())
            };
            let result = vm.intl_collator_construct(
                &program,
                &[locales, Value::number(7.0)],
                constructor,
            );
            assert_eq!(result.is_ok(), !abrupt, "{phase}/{abrupt}");
            assert_eq!(vm.heap.root_count_for_test(), roots);
            assert_eq!(vm.active_call_roots.len(), calls);
            match result {
                Ok(collator) => {
                    let owner = vm.heap.root(collator);
                    vm.collect_now(&program);
                    let left = vm.heap.alloc_string("a".into());
                    let right = vm.heap.alloc_string("b".into());
                    let result = vm
                        .intl_collator_native(
                            &program,
                            Native::IntlCollatorCompare,
                            collator,
                            &[left, right],
                        )
                        .unwrap();
                    assert_eq!(result.as_number(), Some(-1.0), "{phase}");
                    let atom = vm.intern_atom("traceLog");
                    let logger = vm.own_property(vm.realm.globals, atom).unwrap();
                    let trace = vm
                        .call_value(&program, logger, Value::UNDEFINED, &[])
                        .unwrap();
                    let prefix = if matches!(phase, "length" | "index" | "locale") {
                        "length,index,locale,"
                    } else {
                        ""
                    };
                    assert_eq!(
                        vm.to_string(&program, trace).unwrap(),
                        format!(
                            "{prefix}usage,usage-string,localeMatcher,collation,numeric,caseFirst,sensitivity,ignorePunctuation"
                        ),
                        "{phase}"
                    );
                    vm.heap.release_root(owner);
                }
                Err(error) => {
                    let kind = vm.intern_atom("kind");
                    let value = vm
                        .own_property(error.thrown_value().unwrap(), kind)
                        .unwrap();
                    assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                }
            }
            vm.collect_now(&program);
        }
    }
}

#[test]
fn intl_coerced_option_views_survive_fresh_conversion_callbacks() {
    let compile = Engine::specialize;
    for (kind, key, expected) in [
        ("NumberFormat", "style", "percent"),
        ("PluralRules", "type", "ordinal"),
        ("RelativeTimeFormat", "numeric", "auto"),
    ] {
        for phase in ["convert", "matcher", "selected"] {
            for abrupt in [false, true] {
                let source = format!(
                    r#"
                var trace=[];
                function hit(name){{trace.push(name);if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                function matcher(){{return {{toString(){{hit('convert');return 'lookup';}}}};}}
                Object.defineProperty(Number.prototype,'localeMatcher',{{get(){{hit('matcher');return matcher();}},configurable:true}});
                Object.defineProperty(Number.prototype,'{key}',{{get(){{hit('selected');return '{expected}';}},configurable:true}});
                function inspect(value){{return Intl.{kind}.prototype.resolvedOptions.call(value)['{key}'];}}
                function traceLog(){{return trace.join(',');}}
                "#
                );
                let program = compile(&source, "intl-coerced-view.js").unwrap();
                let mut vm = quench_runtime::test_support::vm(TestHost::default().with_test262());
                quench_runtime::test_support::execute(&mut vm, &program).unwrap();
                let intl = vm.intern_atom("Intl");
                vm.materialize_lazy_data_property(&program, vm.realm.globals, quench_runtime::vm::property_key::PropertyKey::string(intl)).unwrap();
                let intl = vm.own_property(vm.realm.globals, intl).unwrap();
                let name = vm.intern_atom(kind);
                let constructor = vm.own_property(intl, name).unwrap();
                let locale = vm.heap.alloc_string("en".into());
                let weak = vm.heap.weak_handle(locale).unwrap();
                let locale_owner = vm.heap.root(locale);
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = match kind {
                    "NumberFormat" => vm.intl_number_format_construct(
                        &program,
                        &[locale, Value::number(7.0)],
                        constructor,
                    ),
                    "PluralRules" => vm.intl_plural_rules_construct(
                        &program,
                        &[locale, Value::number(7.0)],
                        constructor,
                    ),
                    "RelativeTimeFormat" => vm.intl_relative_time_format_construct(
                        &program,
                        &[locale, Value::number(7.0)],
                        constructor,
                    ),
                    _ => unreachable!(),
                };
                assert_eq!(result.is_ok(), !abrupt, "{kind}/{phase}/{abrupt}");
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                assert_eq!(vm.heap.weak_value(weak), Some(locale), "{kind}/{phase}");
                match result {
                    Ok(instance) => {
                        let owner = vm.heap.root(instance);
                        vm.collect_now(&program);
                        let atom = vm.intern_atom("inspect");
                        let inspect = vm.own_property(vm.realm.globals, atom).unwrap();
                        let output = vm
                            .call_value(&program, inspect, Value::UNDEFINED, &[instance])
                            .unwrap();
                        assert_eq!(
                            vm.to_string(&program, output).unwrap(),
                            expected,
                            "{kind}/{phase}"
                        );
                        let atom = vm.intern_atom("traceLog");
                        let logger = vm.own_property(vm.realm.globals, atom).unwrap();
                        let trace = vm
                            .call_value(&program, logger, Value::UNDEFINED, &[])
                            .unwrap();
                        assert_eq!(
                            vm.to_string(&program, trace).unwrap(),
                            "matcher,convert,selected",
                            "{kind}/{phase}"
                        );
                        vm.heap.release_root(owner);
                    }
                    Err(error) => {
                        let atom = vm.intern_atom("kind");
                        let value = vm
                            .own_property(error.thrown_value().unwrap(), atom)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                    }
                }
                vm.heap.release_root(locale_owner);
                vm.collect_now(&program);
                assert!(vm.heap.weak_value(weak).is_none());
            }
        }
    }
}

#[test]
fn intl_constructor_views_survive_prototype_and_option_callbacks() {
    let compile = Engine::specialize;
    for (kind, key, expected) in [
        ("DisplayNames", "type", "language"),
        ("DurationFormat", "style", "short"),
        ("DateTimeFormat", "year", "numeric"),
    ] {
        for phase in ["prototype", "locale", "matcher", "convert", "selected"] {
            for abrupt in [false, true] {
                let source = format!(
                    r#"
                function hit(name){{if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                function matcher(){{return {{toString(){{hit('convert');return 'lookup';}}}};}}
                function locales(){{return {{get length(){{hit('locale');return 1;}},0:'en'}};}}
                function options(){{var value={{get localeMatcher(){{hit('matcher');return matcher();}}}};Object.defineProperty(value,'{key}',{{get(){{hit('selected');return '{expected}';}}}});return value;}}
                function target(){{return new Proxy(function(){{}},{{get(value,key){{if(key==='prototype'){{hit('prototype');return {{marker:'expected'}};}}return Reflect.get(value,key);}}}});}}
                function inspect(value){{return Object.getPrototypeOf(value).marker+':'+Intl.{kind}.prototype.resolvedOptions.call(value)['{key}'];}}
                "#
                );
                let program = compile(&source, "intl-constructor-views.js").unwrap();
                let mut vm = quench_runtime::test_support::vm(TestHost::default().with_test262());
                quench_runtime::test_support::execute(&mut vm, &program).unwrap();
                let mut args = Vec::new();
                let mut owners = Vec::new();
                for name in ["locales", "options", "target"] {
                    let atom = vm.intern_atom(name);
                    let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                    let value = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    args.push(value);
                    owners.push(vm.heap.root(value));
                }
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = match kind {
                    "DisplayNames" => {
                        vm.intl_display_names_construct(&program, &args[..2], args[2])
                    }
                    "DurationFormat" => {
                        vm.intl_duration_format_construct(&program, &args[..2], args[2])
                    }
                    "DateTimeFormat" => {
                        vm.intl_date_time_format_construct(&program, &args[..2], args[2])
                    }
                    _ => unreachable!(),
                };
                assert_eq!(result.is_ok(), !abrupt, "{kind}/{phase}/{abrupt}");
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                match result {
                    Ok(instance) => {
                        let owner = vm.heap.root(instance);
                        vm.collect_now(&program);
                        let atom = vm.intern_atom("inspect");
                        let inspect = vm.own_property(vm.realm.globals, atom).unwrap();
                        let output = vm
                            .call_value(&program, inspect, Value::UNDEFINED, &[instance])
                            .unwrap();
                        assert_eq!(
                            vm.to_string(&program, output).unwrap(),
                            format!("expected:{expected}"),
                            "{kind}/{phase}"
                        );
                        vm.heap.release_root(owner);
                    }
                    Err(error) => {
                        let atom = vm.intern_atom("kind");
                        let value = vm
                            .own_property(error.thrown_value().unwrap(), atom)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                    }
                }
                for owner in owners {
                    vm.heap.release_root(owner);
                }
                vm.collect_now(&program);
            }
        }
    }
}

#[test]
fn intl_locale_roots_boxed_options_and_fresh_prototype_views() {
    let compile = Engine::specialize;
    for kind in ["boxed", "object"] {
        for phase in [
            "language-string",
            "prototype",
            "tag",
            "language",
            "region",
            "calendar",
            "calendar-string",
        ] {
            for abrupt in [false, true] {
                let source = format!(
                    r#"
                var trace=[];
                function hit(name){{trace.push(name);if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                function language(){{return {{toString(){{hit('language-string');return 'fr';}}}};}}
                function calendar(){{return {{toString(){{hit('calendar-string');return 'gregory';}}}};}}
                Object.defineProperty(Number.prototype,'language',{{configurable:true,get(){{hit('language');return language();}}}});
                Object.defineProperty(Number.prototype,'region',{{configurable:true,get(){{hit('region');return 'US';}}}});
                Object.defineProperty(Number.prototype,'calendar',{{configurable:true,get(){{hit('calendar');return calendar();}}}});
                function tag(){{return {{toString(){{hit('tag');return 'en';}}}};}}
                function options(){{return '{kind}'==='boxed'?7:Object.create(Number.prototype);}}
                function target(){{return new Proxy(function(){{}},{{get(value,key){{if(key==='prototype'){{hit('prototype');return {{marker:'expected'}};}}return Reflect.get(value,key);}}}});}}
                function inspect(value){{return Object.getPrototypeOf(value).marker+'|'+Intl.Locale.prototype.toString.call(value);}}
                function traceLog(){{return trace.join(',');}}
                "#
                );
                let program = compile(&source, "intl-locale-views.js").unwrap();
                let mut vm = quench_runtime::test_support::vm(TestHost::default().with_test262());
                quench_runtime::test_support::execute(&mut vm, &program).unwrap();
                let mut args = Vec::new();
                let mut owners = Vec::new();
                for name in ["tag", "options", "target"] {
                    let atom = vm.intern_atom(name);
                    let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                    let value = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    args.push(value);
                    owners.push(vm.heap.root(value));
                }
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = vm.intl_namespace_construct(
                    &program,
                    Native::IntlLocale,
                    &args[..2],
                    args[2],
                );
                assert_eq!(result.is_ok(), !abrupt, "{kind}/{phase}/{abrupt}");
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                match result {
                    Ok(instance) => {
                        let weak = vm.heap.weak_handle(instance).unwrap();
                        let owner = vm.heap.root(instance);
                        vm.collect_now(&program);
                        let atom = vm.intern_atom("inspect");
                        let function = vm.own_property(vm.realm.globals, atom).unwrap();
                        let output = vm
                            .call_value(&program, function, Value::UNDEFINED, &[instance])
                            .unwrap();
                        assert_eq!(
                            vm.to_string(&program, output).unwrap(),
                            "expected|fr-US-u-ca-gregory",
                            "{kind}/{phase}"
                        );
                        let atom = vm.intern_atom("traceLog");
                        let function = vm.own_property(vm.realm.globals, atom).unwrap();
                        let output = vm
                            .call_value(&program, function, Value::UNDEFINED, &[])
                            .unwrap();
                        assert_eq!(
                            vm.to_string(&program, output).unwrap(),
                            "prototype,tag,language,language-string,region,calendar,calendar-string",
                            "{kind}/{phase}"
                        );
                        vm.heap.release_root(owner);
                        vm.collect_now(&program);
                        assert!(vm.heap.weak_value(weak).is_none());
                    }
                    Err(error) => {
                        let atom = vm.intern_atom("kind");
                        let value = vm
                            .own_property(error.thrown_value().unwrap(), atom)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                    }
                }
                for owner in owners {
                    vm.heap.release_root(owner);
                }
                vm.collect_now(&program);
            }
        }
    }
}
