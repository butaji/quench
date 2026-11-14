//! Private names across class evaluations, reflection, `eval` and proxies.

use super::output;

#[test]
fn nested_class_in_computed_key_keeps_outer_private_names() {
    assert_eq!(
        output(
            "let read; class A { static #x = 42; static [(class {}, read = () => A.#x, 'k')] = 1; } print(read()); for (let i = 0; i < 2; i++) { let f; class B { static #y = i; static [(class {}, f = () => B.#y, 'k')]; } print(f()); }"
        ),
        ["42", "0", "1"]
    );
}

#[test]
fn generator_return_inside_loop_head_pattern_closes_pattern_iterator() {
    assert_eq!(
        output("var log = []; function source(name) { return { [Symbol.iterator]() { return { next() { log.push(name + ':next'); return { done: false }; }, return() { log.push(name + ':return'); return {}; } }; } }; } function* plain() { for ([ {} = yield ] of [source('a')]) {} } var g = plain(); g.next(); print(JSON.stringify(g.return(7)) + ' ' + log.join()); log = []; function* guarded() { try { for (var [ x = yield ] of [source('b')]) {} } finally { log.push('finally'); } } g = guarded(); g.next(); print(JSON.stringify(g.return(8)) + ' ' + log.join()); "),
        [
            "{\"value\":7,\"done\":true} a:next,a:return",
            "{\"value\":8,\"done\":true} b:next,b:return,finally"
        ]
    );
}

#[test]
fn nested_direct_eval_vars_belong_to_the_calling_function() {
    assert_eq!(
        output("function u(code) { return eval(code); } u(\"eval('var z = 2;')\"); function v() { eval(\"eval('var y = 3;')\"); return typeof y; } function w() { 'use strict'; eval(\"eval('var x = 1;')\"); return typeof x; } print([typeof z, 'z' in globalThis, v(), typeof y, w()].join()); "),
        ["undefined,false,number,undefined,undefined"]
    );
}

#[test]
fn private_names_of_two_evaluations_of_one_class_never_share_a_brand() {
    // Each evaluation declares fresh private atoms, so an object carries the brand of its own
    // evaluation's class alone: fields, methods and static names, by `#x in o` and by access.
    assert_eq!(
        output(
            r#"
            const make = () => class {
              #x = 1; #m() { return 2; } static #s = 3;
              static has(o) { return #x in o; } static hasM(o) { return #m in o; } static hasS(o) { return #s in o; }
              static read(o) { return o.#x; } static call(o) { return o.#m(); } static sread() { return this.#s; }
            };
            const A = make(), B = make();
            const a = new A(), b = new B();
            print([A.has(a), A.has(b), B.has(a), B.has(b), A.hasM(a), A.hasM(b), A.hasS(A), A.hasS(B), B.hasS(B)].join());
            const errors = [];
            for (const f of [() => A.read(b), () => B.read(a), () => A.call(b), () => B.call(a)]) {
              try { f(); errors.push('no'); } catch (e) { errors.push(e instanceof TypeError ? 'TypeError' : 'other'); }
            }
            print(errors.join());
            print([A.read(a), B.read(b), A.call(a), A.sread(), B.sread()].join());
            "#
        ),
        ["true,false,false,true,true,false,true,false,true", "TypeError,TypeError,TypeError,TypeError", "1,1,2,3,3"]
    );
}

#[test]
fn private_names_stay_out_of_every_reflective_listing() {
    assert_eq!(
        output(
            r#"
            class Q {
              #a = 1; #b() {} static #c = 3; pub = 2;
              static probe(o) {
                const forIn = []; for (const k in o) forIn.push(k);
                return [Reflect.ownKeys(o).join(), Object.getOwnPropertyNames(o).join(), Object.getOwnPropertySymbols(o).length,
                  JSON.stringify(o), Object.keys(o).join(), Object.entries(o).join(';'), forIn.join(),
                  JSON.stringify(Object.getOwnPropertyDescriptors(o)), JSON.stringify({ ...o }), Object.isFrozen(Object.freeze(o))].join('|');
              }
            }
            print(Q.probe(new Q()));
            print(Reflect.ownKeys(Q).join());
            "#
        ),
        [
            r#"pub|pub|0|{"pub":2}|pub|pub,2|pub|{"pub":{"value":2,"writable":true,"enumerable":true,"configurable":true}}|{"pub":2}|true"#,
            "length,name,prototype,probe"
        ]
    );
}

#[test]
fn proxies_hold_private_names_of_their_own_and_expose_none() {
    assert_eq!(
        output(
            r#"
            class Base { constructor(o) { return o; } }
            class P extends Base { #p = 1; static get(o) { return o.#p; } static has(o) { return #p in o; } }
            const target = {};
            const proxy = new Proxy(target, {});
            new P(proxy);
            print([P.get(proxy), P.has(proxy), P.has(target), Reflect.ownKeys(proxy).length, Object.getOwnPropertySymbols(proxy).length, JSON.stringify(proxy), Object.keys(proxy).length].join());
            "#
        ),
        ["1,true,false,0,0,{},0"]
    );
}

#[test]
fn eval_reaches_the_private_names_of_the_class_it_runs_in() {
    assert_eq!(
        output(
            r#"
            class C { #x = 5; static #y = 6; run(code) { return eval(code); } static runS(code) { return eval(code); } }
            print([new C().run('this.#x'), C.runS('C.#y'), new C().run('#x in this'), C.runS('#y in C')].join());
            const make = () => class { #z = 7; run(code) { return eval(code); } };
            const [A, B] = [make(), make()];
            const a = new A();
            let cross;
            try { cross = new B().run.call(a, 'this.#z'); } catch (e) { cross = e instanceof TypeError ? 'TypeError' : 'other'; }
            print([a.run('this.#z'), cross].join());
            "#
        ),
        ["5,6,true,true", "7,TypeError"]
    );
}

#[test]
fn private_members_resolve_on_branded_proxy_receivers() {
    assert_eq!(
        output("class Base { constructor() { return new Proxy(this, {}); } } class F extends Base { #x = 1; t() { return this.#x; } } class M extends Base { #m() { return 2; } t() { return this.#m(); } } class G extends Base { get #g() { return 3; } t() { return this.#g; } } print([F, M, G].map(C => new C().t()).join(' '));"),
        ["1 2 3"]
    );
}
