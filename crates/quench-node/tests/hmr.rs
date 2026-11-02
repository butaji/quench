//! `--hmr` as a running process sees it: accepted CommonJS and TypeScript ES module edits
//! replace modules in place, a failed update keeps the prior module, an edit nobody
//! accepted restarts the process, and runs without the flag are unchanged.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

const QUENCH: &str = env!("CARGO_BIN_EXE_quench-node");
/// How long one expected line may take (a debug build starts slowly).
const LINE_DEADLINE: Duration = Duration::from_secs(60);

struct App {
    root: PathBuf,
    child: Child,
    lines: Receiver<String>,
}

impl App {
    fn start(name: &str, files: &[(&str, &str)], entry: &str) -> Self {
        let root = std::env::temp_dir().join(format!("quench-hmr-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for (file, source) in files {
            std::fs::write(root.join(file), source).unwrap();
        }
        let mut child = Command::new(QUENCH)
            .arg("--hmr")
            .arg(root.join(entry))
            .env_remove("NODE_OPTIONS")
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    return;
                }
            }
        });
        Self { root, child, lines }
    }

    fn expect(&self, expected: &str) {
        loop {
            let line = self
                .lines
                .recv_timeout(LINE_DEADLINE)
                .unwrap_or_else(|_| panic!("no line {expected:?} from the running app"));
            if line == expected {
                return;
            }
            assert!(!line.starts_with("unexpected"), "{line}");
        }
    }

    fn edit(&self, file: &str, source: &str) {
        write_new_version(&self.root.join(file), source);
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Replace a file's contents with a new modification time even within one clock tick.
fn write_new_version(path: &Path, source: &str) {
    let before = std::fs::metadata(path).and_then(|metadata| metadata.modified()).ok();
    loop {
        std::fs::write(path, source).unwrap();
        let after = std::fs::metadata(path).and_then(|metadata| metadata.modified()).ok();
        if after != before {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn accepted_commonjs_dependency_is_replaced_in_place() {
    let app = App::start(
        "commonjs",
        &[
            (
                "main.cjs",
                "let dep = require('./dep.cjs');\n\
                 module.hot.accept('./dep.cjs', (updated) => { dep = updated; console.log('accepted ' + dep.value); });\n\
                 console.log('start ' + dep.value);\n",
            ),
            ("dep.cjs", "exports.value = 1;\n"),
        ],
        "main.cjs",
    );
    app.expect("start 1");
    app.edit("dep.cjs", "exports.value = 2;\n");
    app.expect("accepted 2");
    // A throwing update keeps the prior module; the next good edit still applies.
    app.edit("dep.cjs", "throw new Error('broken');\n");
    std::thread::sleep(Duration::from_millis(500));
    app.edit("dep.cjs", "exports.value = 3;\n");
    app.expect("accepted 3");
}

#[test]
fn self_accepted_typescript_module_keeps_disposed_data() {
    let app = App::start(
        "typescript",
        &[
            (
                "main.mts",
                "import { greet } from './greeting.ts';\n\
                 console.log(greet('start'));\n\
                 setInterval(() => {}, 1000);\n",
            ),
            (
                "greeting.ts",
                "const generation: number = (import.meta.hot!.data.generation ?? 0) + 1;\n\
                 import.meta.hot!.dispose((data: { generation: number }) => { data.generation = generation; });\n\
                 import.meta.hot!.accept();\n\
                 export function greet(name: string): string { return `hello ${name} ${generation}`; }\n\
                 console.log(greet('loaded'));\n",
            ),
        ],
        "main.mts",
    );
    app.expect("hello loaded 1");
    app.expect("hello start 1");
    app.edit(
        "greeting.ts",
        "const generation: number = (import.meta.hot!.data.generation ?? 0) + 1;\n\
         import.meta.hot!.dispose((data: { generation: number }) => { data.generation = generation; });\n\
         import.meta.hot!.accept();\n\
         export function greet(name: string): string { return `goodbye ${name} ${generation}`; }\n\
         console.log(greet('loaded'));\n",
    );
    app.expect("goodbye loaded 2");
}

#[test]
fn typescript_dependency_accepted_by_its_importer() {
    let app = App::start(
        "typescript-importer",
        &[
            (
                "main.ts",
                "import { area } from './shape.ts';\n\
                 console.log('start ' + area(2));\n\
                 import.meta.hot!.accept('./shape.ts', (shape: { area(side: number): number }) => console.log('accepted ' + shape.area(2)));\n",
            ),
            ("shape.ts", "export function area(side: number): number { return side * side; }\n"),
        ],
        "main.ts",
    );
    app.expect("start 4");
    app.edit("shape.ts", "export function area(side: number): number { return side * side * 10; }\n");
    app.expect("accepted 40");
}

#[test]
fn unaccepted_edit_restarts_the_process() {
    let app = App::start(
        "restart",
        &[
            ("main.cjs", "console.log('start ' + require('./dep.cjs').value);\n"),
            ("dep.cjs", "exports.value = 1;\n"),
        ],
        "main.cjs",
    );
    app.expect("start 1");
    app.edit("dep.cjs", "exports.value = 2;\n");
    app.expect("start 2");
    // A declined self-accepting module restarts too.
    app.edit("dep.cjs", "module.hot.accept(); module.hot.decline(); exports.value = 3;\n");
    app.expect("start 3");
    app.edit("dep.cjs", "module.hot.accept(); module.hot.decline(); exports.value = 4;\n");
    app.expect("start 4");
}

#[test]
fn without_the_flag_modules_have_no_hot_api_and_the_process_exits() {
    let root = std::env::temp_dir().join(format!("quench-hmr-{}-default", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("main.cjs"), "console.log(typeof module.hot);\n").unwrap();
    std::fs::write(root.join("main.mjs"), "console.log(typeof import.meta.hot);\n").unwrap();
    for entry in ["main.cjs", "main.mjs"] {
        let output = Command::new(QUENCH).arg(root.join(entry)).env_remove("NODE_OPTIONS").output().unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "undefined\n");
    }
    let output = Command::new(QUENCH).args(["--hmr", "-e", "1"]).output().unwrap();
    assert!(!output.status.success());
    let _ = std::fs::remove_dir_all(&root);
}
