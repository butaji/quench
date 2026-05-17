//! # Framework Example Tests
//!
//! Tests that verify the 12 framework examples from rune_framework_examples.md
//! parse, analyze, and emit key patterns correctly.

use crate::{analyzer, codegen, parser};
use std::path::Path;

fn load_example(path: &str) -> String {
    // Tests run from crates/rune/, so examples are at ../../examples/
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let full_path = Path::new(&manifest_dir).join("../../examples").join(path);
    std::fs::read_to_string(&full_path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", full_path.display(), e))
}

fn transpile_example(source: &str, filename: &str) -> String {
    let file = parser::parse_file_from_str(source, filename).unwrap();
    let analysis = analyzer::analyze(&file).unwrap();
    let result = codegen::generate(&file, &analysis).unwrap();
    result.source
}

// ------------------------------------------------------------------
// 1. Axum API
// ------------------------------------------------------------------

#[test]
fn test_axum_api_parses_and_emits() {
    let source = load_example("01_axum_api/api.r.ts");
    let emitted = transpile_example(&source, "api.r.ts");

    assert!(emitted.contains("pub fn create_router"));
    assert!(emitted.contains("async fn list_tasks"));
    assert!(emitted.contains("async fn get_task"));
    assert!(emitted.contains("async fn create_task"));
    assert!(emitted.contains("async fn delete_task"));
    assert!(emitted.contains("AppState"));
}

// ------------------------------------------------------------------
// 2. Actix-web
// ------------------------------------------------------------------

#[test]
fn test_actix_web_parses_and_emits() {
    let source = load_example("02_actix_web/server.r.ts");
    let emitted = transpile_example(&source, "server.r.ts");

    assert!(emitted.contains("pub fn create_app"));
    assert!(emitted.contains("async fn list_tasks"));
    assert!(emitted.contains("async fn create_task"));
    assert!(emitted.contains("AppState"));
}

// ------------------------------------------------------------------
// 3. Ratatui + clap
// ------------------------------------------------------------------

#[test]
fn test_ratatui_clap_main_parses_and_emits() {
    let source = load_example("03_ratatui_clap/main.r.ts");
    let emitted = transpile_example(&source, "main.r.ts");

    assert!(emitted.contains("pub fn parse_args"));
    assert!(emitted.contains("pub fn run_terminal"));
    assert!(emitted.contains("Args"));
}

#[test]
fn test_ratatui_clap_views_tsx_parses_and_emits() {
    let source = load_example("03_ratatui_clap/views.r.tsx");
    let emitted = transpile_example(&source, "views.r.tsx");

    // TSX should emit at least the function and props struct;
    // Layout/Gauge are not yet in the JSX emitter (generic fallback)
    assert!(emitted.contains("pub fn root_view"));
    assert!(emitted.contains("Props"));
}

// ------------------------------------------------------------------
// 4. Tauri Commands
// ------------------------------------------------------------------

#[test]
fn test_tauri_commands_parses_and_emits() {
    let source = load_example("04_tauri_commands/commands.r.ts");
    let emitted = transpile_example(&source, "commands.r.ts");

    assert!(emitted.contains("pub fn init_state"));
    assert!(emitted.contains("pub fn get_tasks"));
    assert!(emitted.contains("pub fn add_task"));
    assert!(emitted.contains("pub fn toggle_task"));
    assert!(emitted.contains("pub fn emit_update"));
    assert!(emitted.contains("AppState"));
}

// ------------------------------------------------------------------
// 5. Dioxus App
// ------------------------------------------------------------------

#[test]
fn test_dioxus_app_parses_and_emits() {
    let source = load_example("05_dioxus_app/app.r.tsx");
    let emitted = transpile_example(&source, "app.r.tsx");

    assert!(emitted.contains("pub fn app"));
    assert!(emitted.contains("AppProps"));
}

// ------------------------------------------------------------------
// 6. egui Tool
// ------------------------------------------------------------------

#[test]
fn test_egui_tool_parses_and_emits() {
    let source = load_example("06_egui_tool/tool.r.tsx");
    let emitted = transpile_example(&source, "tool.r.tsx");

    assert!(emitted.contains("pub fn task_editor"));
    assert!(emitted.contains("Props"));
}

// ------------------------------------------------------------------
// 7. Leptos App
// ------------------------------------------------------------------

#[test]
fn test_leptos_app_parses_and_emits() {
    let source = load_example("07_leptos_app/app.r.tsx");
    let emitted = transpile_example(&source, "app.r.tsx");

    assert!(emitted.contains("pub fn task_app"));
}

// ------------------------------------------------------------------
// 8. Yew App
// ------------------------------------------------------------------

#[test]
fn test_yew_app_parses_and_emits() {
    let source = load_example("08_yew_app/app.r.tsx");
    let emitted = transpile_example(&source, "app.r.tsx");

    assert!(emitted.contains("pub fn task_list"));
}

// ------------------------------------------------------------------
// 9. Bevy Game
// ------------------------------------------------------------------

#[test]
fn test_bevy_game_parses_and_emits() {
    let source = load_example("09_bevy_game/game.r.ts");
    let emitted = transpile_example(&source, "game.r.ts");

    assert!(emitted.contains("pub fn setup_game"));
    assert!(emitted.contains("Position"));
    assert!(emitted.contains("Velocity"));
    assert!(emitted.contains("GameState"));
    assert!(emitted.contains("TaskCompleted"));
}

// ------------------------------------------------------------------
// 10. SQLx DB
// ------------------------------------------------------------------

#[test]
fn test_sqlx_db_parses_and_emits() {
    let source = load_example("10_sqlx_db/db.r.ts");
    let emitted = transpile_example(&source, "db.r.ts");

    assert!(emitted.contains("pub async fn init_db"));
    assert!(emitted.contains("pub async fn get_task_by_id"));
    assert!(emitted.contains("pub async fn create_task"));
    assert!(emitted.contains("pub async fn list_tasks"));
    assert!(emitted.contains("DbPool"));
}

// ------------------------------------------------------------------
// 11. Tonic Service
// ------------------------------------------------------------------

#[test]
fn test_tonic_service_parses_and_emits() {
    let source = load_example("11_tonic_service/service.r.ts");
    let emitted = transpile_example(&source, "service.r.ts");

    assert!(emitted.contains("pub fn task_service"));
    assert!(emitted.contains("async fn handle_get_task"));
    assert!(emitted.contains("async fn handle_list_tasks"));
    assert!(emitted.contains("async fn handle_create_task"));
}

// ------------------------------------------------------------------
// 12. Candle Inference
// ------------------------------------------------------------------

#[test]
fn test_candle_infer_parses_and_emits() {
    let source = load_example("12_candle_infer/infer.r.ts");
    let emitted = transpile_example(&source, "infer.r.ts");

    assert!(emitted.contains("pub async fn load_llama"));
    assert!(emitted.contains("pub fn load_tokenizer"));
    assert!(emitted.contains("pub async fn complete"));
    assert!(emitted.contains("LlamaModel"));
}



#[test]
fn _check_all_examples_validation() {
    let examples = [
        "01_axum_api/api.r.ts",
        "02_actix_web/server.r.ts",
        "03_ratatui_clap/main.r.ts",
        "03_ratatui_clap/views.r.tsx",
        "04_tauri_commands/commands.r.ts",
        "05_dioxus_app/app.r.tsx",
        "06_egui_tool/tool.r.tsx",
        "07_leptos_app/app.r.tsx",
        "08_yew_app/app.r.tsx",
        "09_bevy_game/game.r.ts",
        "10_sqlx_db/db.r.ts",
        "11_tonic_service/service.r.ts",
        "12_candle_infer/infer.r.ts",
    ];
    for ex in examples {
        let source = load_example(ex);
        let file = parser::parse_file_from_str(&source, ex).unwrap();
        match analyzer::analyze(&file) {
            Ok(_) => println!("OK: {}", ex),
            Err(e) => println!("FAIL: {} => {:?}", ex, e),
        }
    }
}

