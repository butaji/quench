use super::*;
use quench_runtime::{Engine, ResidualProgram, Value as JsValue};

const ALGORITHMS: [&str; 4] = ["sha256", "sha512", "sha1", "md5"];
const HASH_COUNT: usize = 48;
const HMAC_COUNT: usize = 24;
/// Every this-many-th handle is released before the first collection.
const RELEASE_STRIDE: usize = 3;
/// The oracle's groups: hash digests, Hmac digests and copies; then the survivor's digest.
const DIGEST_GROUPS: usize = 3;

struct Binding {
    runtime: Runtime<NodeHost>,
    program: ResidualProgram,
    hash: RootId,
    hmac: RootId,
    key: RootId,
}

fn oracle() -> serde_json::Value {
    let oracle = std::process::Command::new("node")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/crypto_hash_native_oracle.cjs"))
        .env_remove("NODE_OPTIONS")
        .output().unwrap();
    assert!(oracle.status.success(), "{}", String::from_utf8_lossy(&oracle.stderr));
    serde_json::from_slice(&oracle.stdout).unwrap()
}

fn binding() -> Binding {
    let mut runtime = Runtime::new(NodeHost::new(vec!["quench-node".into()]));
    let program = Engine::specialize("0;", "native-hash-slots.js").unwrap();
    runtime.execute(&program).unwrap();
    let undefined = runtime.root(JsValue::UNDEFINED);
    let initialize = runtime.host_function(crate::host::shared_vm::operation("cryptoHashBinding")).unwrap();
    let binding = runtime.call_rooted(initialize, undefined, &[]).unwrap();
    let hash = runtime.get_named_property_rooted(binding, "Hash").unwrap();
    let hmac = runtime.get_named_property_rooted(binding, "Hmac").unwrap();
    let key = runtime.uint8_array_from_rooted(vec![0, 255, 1, 128, 42]).unwrap();
    Binding { runtime, program, hash, hmac, key }
}

fn call(runtime: &mut Runtime<NodeHost>, object: RootId, name: &str, args: &[RootId]) -> RootId {
    let method = runtime.get_named_property_rooted(object, name).unwrap();
    let result = runtime.call_rooted(method, object, args).unwrap();
    runtime.release_root(method);
    result
}

/// Feeds `text` to a handle, which reports success.
fn update(runtime: &mut Runtime<NodeHost>, value: RootId, text: &str) {
    let text = runtime.string_rooted(text);
    let result = call(runtime, value, "update", &[text]);
    assert_eq!(runtime.rooted_value(result).unwrap().as_bool(), Some(true));
    for root in [text, result] { runtime.release_root(root); }
}

fn hex_digest(runtime: &mut Runtime<NodeHost>, value: RootId) -> String {
    let encoding = runtime.string_rooted("hex");
    let digest = call(runtime, value, "digest", &[encoding]);
    let text = runtime.string_text_rooted(digest).unwrap().unwrap();
    for root in [encoding, digest] { runtime.release_root(root); }
    text
}

fn new_handle(binding: &mut Binding, kind: RootId, algorithm: &str) -> RootId {
    let runtime = &mut binding.runtime;
    let algorithm = runtime.string_rooted(algorithm);
    let value = if kind == binding.hash {
        runtime.construct_rooted(kind, kind, &[algorithm]).unwrap()
    } else {
        let value = runtime.construct_rooted(kind, kind, &[]).unwrap();
        let result = call(runtime, value, "init", &[algorithm, binding.key]);
        runtime.release_root(result);
        value
    };
    runtime.release_root(algorithm);
    value
}

/// `count` live handles of `kind`, all updated before any digests; returns them and their digests.
fn digest_group(binding: &mut Binding, kind: RootId, count: usize) -> (Vec<RootId>, serde_json::Value) {
    let mut live = Vec::new();
    for index in 0..count {
        let value = new_handle(binding, kind, ALGORITHMS[index % ALGORITHMS.len()]);
        update(&mut binding.runtime, value, &format!("prefix {index} é水🦄"));
        live.push(value);
    }
    let digests: Vec<_> = live.iter().enumerate().map(|(index, &value)| {
        update(&mut binding.runtime, value, &format!(" suffix {index}"));
        hex_digest(&mut binding.runtime, value)
    }).collect();
    (live, serde_json::to_value(digests).unwrap())
}

/// A copy continues from its source's state independently of it.
fn copy_digests(binding: &mut Binding) -> serde_json::Value {
    let hash = binding.hash;
    let copies: Vec<_> = ALGORITHMS.iter().map(|algorithm| {
        let original = new_handle(binding, hash, algorithm);
        let runtime = &mut binding.runtime;
        update(runtime, original, "copy prefix");
        let copied = runtime.construct_rooted(hash, hash, &[original]).unwrap();
        let pair: Vec<_> = [(original, " original"), (copied, " copied")].into_iter().map(|(value, suffix)| {
            update(runtime, value, suffix);
            hex_digest(runtime, value)
        }).collect();
        for root in [original, copied] { runtime.release_root(root); }
        pair
    }).collect();
    serde_json::to_value(copies).unwrap()
}

/// Released handles' slots are vacated at collection and reused by new handles.
fn reclaim_at_collection(binding: &mut Binding, handles: &[RootId], algorithm: RootId) -> Vec<RootId> {
    let hash = binding.hash;
    let runtime = &mut binding.runtime;
    let shared = runtime.host_mut().shared_state();
    let allocated = shared.borrow().crypto_hashes.handles.len();
    for &handle in handles.iter().step_by(RELEASE_STRIDE) { runtime.release_root(handle); }
    runtime.collect(&binding.program).unwrap();
    let vacancies = shared.borrow().crypto_hashes.free.len();
    assert_eq!(vacancies, handles.len() / RELEASE_STRIDE + ALGORITHMS.len() * 2);
    let replacements: Vec<_> = (0..vacancies).map(|_| runtime.construct_rooted(hash, hash, &[algorithm]).unwrap()).collect();
    assert_eq!(shared.borrow().crypto_hashes.handles.len(), allocated);
    assert!(shared.borrow().crypto_hashes.free.is_empty());
    replacements
}

#[test]
fn native_hash_slots_match_node_and_reclaim_at_collection() {
    let expected = oracle();
    let mut binding = binding();
    let (hash, hmac) = (binding.hash, binding.hmac);
    let (mut handles, hashes) = digest_group(&mut binding, hash, HASH_COUNT);
    let (hmacs, hmac_digests) = digest_group(&mut binding, hmac, HMAC_COUNT);
    handles.extend(hmacs);
    let results = vec![hashes, hmac_digests, copy_digests(&mut binding)];
    assert_eq!(serde_json::Value::Array(results), serde_json::Value::Array(expected.as_array().unwrap()[..DIGEST_GROUPS].to_vec()));

    // Native resources disappear at GC, before an event-loop turn or another constructor.
    let survivor = new_handle(&mut binding, hash, "sha256");
    update(&mut binding.runtime, survivor, "live across");
    let algorithm = binding.runtime.string_rooted("sha256");
    let replacements = reclaim_at_collection(&mut binding, &handles, algorithm);
    // A live target remains usable across collection, while wrong receivers are rejected.
    let runtime = &mut binding.runtime;
    update(runtime, survivor, " collection");
    assert_eq!(hex_digest(runtime, survivor), expected[DIGEST_GROUPS].as_str().unwrap());
    assert_eq!(hex_digest(runtime, handles[1]), expected[0][1].as_str().unwrap());
    let update_method = runtime.get_named_property_rooted(handles[1], "update").unwrap();
    let wrong = runtime.object_rooted().unwrap();
    let error = runtime.call_rooted(update_method, wrong, &[algorithm]).unwrap_err();
    let exception = error.exception.expect("a rejected receiver throws");
    runtime.release_root(exception);
    let kept = handles.into_iter().enumerate().filter(|(index, _)| index % RELEASE_STRIDE != 0).map(|(_, value)| value);
    for value in kept.chain(replacements).chain([survivor]) { runtime.release_root(value); }
    runtime.collect(&binding.program).unwrap();
    let shared = runtime.host_mut().shared_state();
    assert!(shared.borrow().crypto_hashes.handles.is_empty());
    assert!(shared.borrow().crypto_hashes.free.is_empty());
}
