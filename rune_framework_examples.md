# Rune Framework Examples: Zero-Overhead TS→Rust Mappings

> Comprehensive examples showing how Rune's TypeScript/TSX subset transpiles to idiomatic, zero-overhead Rust for each major framework scenario.

---

## 1. Web/API: Axum

**Pattern:** Extractors, state, middleware, JSON REST API.

### Rune (`api.r.ts`)

```typescript
import { Router, Json, State, Path } from "axum";
import { Task } from "./models.r.ts";

export type AppState = {
  tasks: Task[];
  nextId: number;
};

export function createRouter(): Router {
  return Router.new()
    .route("/tasks", { get: listTasks, post: createTask })
    .route("/tasks/:id", { get: getTask, delete: deleteTask })
    .layer(TraceLayer.new());
}

async function listTasks(
  state: State<AppState>
): Json<Task[]> {
  return Json(state.tasks);
}

async function getTask(
  state: State<AppState>,
  params: Path<{ id: number }>
): Json<Task | null> {
  for (const t of state.tasks) {
    if (t.id === params.id) return Json(t);
  }
  return Json(null);
}

async function createTask(
  state: State<AppState>,
  body: Json<{ title: string }>
): Json<Task> {
  const task: Task = {
    id: state.nextId,
    title: body.title,
    done: false,
  };
  state.tasks.push(task);
  state.nextId += 1;
  return Json(task);
}

async function deleteTask(
  state: State<AppState>,
  params: Path<{ id: number }>
): Json<boolean> {
  const initialLen = state.tasks.length;
  state.tasks = state.tasks.filter((t) => t.id !== params.id);
  return Json(state.tasks.length < initialLen);
}
```

### Emitted Rust

```rust
use axum::{
    extract::{Path, State},
    routing::{delete, get, post},
    Json, Router,
};
use tower_http::trace::TraceLayer;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub tasks: Vec<Task>,
    pub next_id: i32,
}

pub fn create_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tasks", get(list_tasks).post(create_task))
        .route("/tasks/:id", get(get_task).delete(delete_task))
        .layer(TraceLayer::new_for_http())
}

async fn list_tasks(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<Task>> {
    Json(state.tasks.clone())
}

async fn get_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> Json<Option<Task>> {
    Json(state.tasks.iter().find(|t| t.id == id).cloned())
}

async fn create_task(
    State(mut state): State<Arc<AppState>>,
    Json(body): Json<CreateTaskBody>,
) -> Json<Task> {
    let task = Task {
        id: state.next_id,
        title: body.title,
        done: false,
    };
    state.tasks.push(task.clone());
    state.next_id += 1;
    Json(task)
}

async fn delete_task(
    State(mut state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> Json<bool> {
    let initial_len = state.tasks.len();
    state.tasks.retain(|t| t.id != id);
    Json(state.tasks.len() < initial_len)
}
```

**Zero-overhead note:** `State<AppState>` maps directly to Axum's `State` extractor. `Json<T>` is Axum's zero-cost JSON extractor. Router builder chain is emitted verbatim. `Arc<AppState>` shared state with zero-cost cloning.

---

## 2. Web/API: Actix-web

**Pattern:** Scope-based routing, Data extractors, JSON responses.

### Rune (`server.r.ts`)

```typescript
import { App, HttpServer, web, HttpResponse } from "actix_web";
import { Task } from "./models.r.ts";

export type AppState = {
  tasks: Task[];
};

export function createApp(): App {
  return App.new()
    .appData(web.Data({ tasks: [] }))
    .service(
      web.scope("/api")
        .route("/tasks", { get: listTasks })
        .route("/tasks", { post: createTask })
    );
}

async function listTasks(data: web.Data<AppState>): HttpResponse {
  return HttpResponse.Ok().json(data.tasks);
}

async function createTask(
  data: web.Data<AppState>,
  body: web.Json<{ title: string }>
): HttpResponse {
  const task: Task = {
    id: data.tasks.length + 1,
    title: body.title,
    done: false,
  };
  data.tasks.push(task);
  return HttpResponse.Created().json(task);
}
```

### Emitted Rust

```rust
use actix_web::{web, App, HttpResponse, HttpServer};

pub struct AppState {
    pub tasks: Vec<Task>,
}

pub fn create_app() -> App {
    App::new()
        .app_data(web::Data::new(AppState { tasks: vec![] }))
        .service(
            web::scope("/api")
                .route("/tasks", web::get().to(list_tasks))
                .route("/tasks", web::post().to(create_task)),
        )
}

async fn list_tasks(data: web::Data<AppState>) -> HttpResponse {
    HttpResponse::Ok().json(data.tasks.clone())
}

async fn create_task(
    data: web::Data<AppState>,
    body: web::Json<CreateTaskBody>,
) -> HttpResponse {
    let task = Task {
        id: data.tasks.len() as i32 + 1,
        title: body.title.clone(),
        done: false,
    };
    data.tasks.push(task);
    HttpResponse::Created().json(task)
}
```

**Zero-overhead note:** `web.Data` and `web.Json` map directly to Actix extractors. `HttpResponse` builder methods emitted as direct calls. Scope nesting preserved.

---

## 3. CLI/TUI: clap + Ratatui

**Pattern:** CLI parsing with clap builder, real-time TUI dashboard with Ratatui widgets.

### Rune (`main.r.ts`)

```typescript
import { Command, Arg } from "clap";
import { Terminal, CrosstermBackend } from "ratatui";

export type Args = {
  file: string;
  interval: number;
};

export function parseArgs(): Args {
  return Command.new("todox")
    .about("Task tracker TUI")
    .arg(Arg.new("file").required(true).help("Tasks JSON file"))
    .arg(Arg.new("interval").defaultValue("5").help("Refresh seconds"))
    .parse();
}

export function runTerminal(args: Args): void {
  const terminal = Terminal.new(CrosstermBackend.new());
  const app = AppState.new(args.file);

  while (app.running) {
    terminal.draw((frame) => {
      frame.renderWidget(rootView(app), frame.area());
    });

    if (pollEvent(args.interval * 1000)) {
      handleInput(app);
    }
  }
}
```

### Rune TSX (`views.r.tsx`)

```tsx
import { Block, Borders, List, ListItem, Gauge, Layout, Constraint } from "ratatui";
import { Task } from "./models.r.ts";

interface Props {
  tasks: Task[];
  selected: number;
  progress: number;
}

export function rootView(props: Props): Widget {
  return (
    <Layout direction="vertical" constraints={[Constraint.Percentage(80), Constraint.Percentage(20)]}>
      <List
        block={<Block title="Tasks" borders={Borders.ALL} />}
        items={props.tasks.map((task, i) => (
          <ListItem style={i === props.selected ? "bold" : "default"}>
            {task.done ? "[x] " : "[ ] "}{task.title}
          </ListItem>
        ))}
        highlightSymbol=">"
      />
      <Gauge
        block={<Block title="Progress" borders={Borders.ALL} />}
        percent={props.progress}
        label={`${props.progress}%`}
      />
    </Layout>
  );
}
```

### Emitted Rust

```rust
use clap::{Arg, Command};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Style, Modifier},
    widgets::{Block, Borders, Gauge, List, ListItem},
    Terminal,
};

pub struct Args {
    pub file: String,
    pub interval: i32,
}

pub fn parse_args() -> Args {
    let matches = Command::new("todox")
        .about("Task tracker TUI")
        .arg(Arg::new("file").required(true).help("Tasks JSON file"))
        .arg(Arg::new("interval").default_value("5").help("Refresh seconds"))
        .get_matches();

    Args {
        file: matches.get_one::<String>("file").unwrap().clone(),
        interval: matches.get_one::<String>("interval").unwrap().parse().unwrap(),
    }
}

pub fn run_terminal(args: &Args) {
    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = AppState::new(&args.file);

    while app.running {
        terminal.draw(|frame| {
            frame.render_widget(root_view(&app), frame.area());
        }).unwrap();

        if poll_event(args.interval * 1000) {
            handle_input(&mut app);
        }
    }
}

pub struct RootViewProps<'a> {
    pub tasks: &'a Vec<Task>,
    pub selected: usize,
    pub progress: f64,
}

pub fn root_view(props: &RootViewProps) -> impl ratatui::widgets::Widget {
    let items: Vec<ListItem> = props.tasks.iter().enumerate().map(|(i, task)| {
        let style = if i == props.selected {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let text = format!("{}{}{}", if task.done { "[x] " } else { "[ ] " }, task.title);
        ListItem::new(text).style(style)
    }).collect();

    let list = List::new(items)
        .block(Block::default().title("Tasks").borders(Borders::ALL))
        .highlight_symbol(">");

    let gauge = Gauge::default()
        .block(Block::default().title("Progress").borders(Borders::ALL))
        .percent(props.progress as u16)
        .label(format!("{}%", props.progress));

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(80), Constraint::Percentage(20)]);

    move |area: ratatui::layout::Rect, buf: &mut ratatui::buffer::Buffer| {
        let chunks = layout.split(area);
        list.render(chunks[0], buf);
        gauge.render(chunks[1], buf);
    }
}
```

**Zero-overhead note:** TSX `<Layout>` emits direct Ratatui `Layout::default()` builder. `<List>` emits `List::new()` with mapped items. No virtual DOM, no diffing—direct frame render calls. `impl ratatui::widgets::Widget` returned directly.

---

## 4. Desktop: Tauri (Rust Backend Commands)

**Pattern:** Command handlers, managed state, window events.

### Rune (`commands.r.ts`)

```typescript
import { State, Window, Emitter } from "tauri";
import { Task } from "./models.r.ts";

export type AppState = {
  tasks: Task[];
  nextId: number;
};

export function initState(): AppState {
  return { tasks: [], nextId: 1 };
}

export function getTasks(state: State<AppState>): Task[] {
  return state.tasks;
}

export function addTask(
  state: State<AppState>,
  title: string
): { ok: true; task: Task } | { ok: false; error: string } {
  if (title.length === 0) {
    return { ok: false, error: "Title required" };
  }
  const task: Task = {
    id: state.nextId,
    title,
    done: false,
  };
  state.tasks.push(task);
  state.nextId += 1;
  return { ok: true, task };
}

export function toggleTask(
  state: State<AppState>,
  id: number
): boolean {
  for (const t of state.tasks) {
    if (t.id === id) {
      t.done = !t.done;
      return true;
    }
  }
  return false;
}

export function emitUpdate(window: Window, tasks: Task[]): void {
  window.emit("tasks:updated", tasks);
}
```

### Emitted Rust

```rust
use tauri::{State, Window, Manager};
use serde_json::Value;

#[derive(Default)]
pub struct AppState {
    pub tasks: Vec<Task>,
    pub next_id: i32,
}

#[tauri::command]
pub fn get_tasks(state: State<'_, AppState>) -> Vec<Task> {
    state.tasks.clone()
}

#[tauri::command]
pub fn add_task(
    state: State<'_, AppState>,
    title: String,
) -> Result<Task, String> {
    if title.is_empty() {
        return Err(String::from("Title required"));
    }
    let task = Task {
        id: state.next_id,
        title,
        done: false,
    };
    state.tasks.push(task.clone());
    state.next_id += 1;
    Ok(task)
}

#[tauri::command]
pub fn toggle_task(state: State<'_, AppState>, id: i32) -> bool {
    if let Some(t) = state.tasks.iter_mut().find(|t| t.id == id) {
        t.done = !t.done;
        return true;
    }
    false
}

#[tauri::command]
pub fn emit_update(window: Window, tasks: Vec<Task>) {
    window.emit("tasks:updated", tasks).unwrap();
}
```

**Zero-overhead note:** `State<AppState>` maps to `State<'_, AppState>`. `Window` maps to `tauri::Window`. Commands get `#[tauri::command]` attribute. Result types map to Rust `Result`. `window.emit()` is direct Tauri API.

---

## 5. Desktop: Dioxus (Cross-Platform UI)

**Pattern:** Component tree with props, signals, event handling.

### Rune TSX (`app.r.tsx`)

```tsx
import { useSignal, useMemo } from "dioxus";
import { Task } from "./models.r.ts";

interface AppProps {
  initialTasks: Task[];
}

export function App(props: AppProps): Element {
  const tasks = useSignal<Task[]>(props.initialTasks);
  const input = useSignal<string>("");
  const filter = useSignal<"all" | "active" | "done">("all");

  const filtered = useMemo(() => {
    return tasks().filter((t) => {
      if (filter() === "active") return !t.done;
      if (filter() === "done") return t.done;
      return true;
    });
  });

  const addTask = () => {
    if (input().length === 0) return;
    const newTask: Task = {
      id: tasks().length + 1,
      title: input(),
      done: false,
    };
    tasks.set([...tasks(), newTask]);
    input.set("");
  };

  const toggle = (id: number) => {
    tasks.set(tasks().map((t) => t.id === id ? { ...t, done: !t.done } : t));
  };

  return (
    <div className="app">
      <div className="filters">
        {(["all", "active", "done"] as const).map((f) => (
          <button
            className={filter() === f ? "active" : ""}
            onclick={() => filter.set(f)}
          >
            {f}
          </button>
        ))}
      </div>
      <div className="input-row">
        <input value={input()} oninput={(e) => input.set(e.value)} />
        <button onclick={addTask}>Add</button>
      </div>
      <ul className="task-list">
        {filtered().map((task) => (
          <li
            className={task.done ? "done" : ""}
            onclick={() => toggle(task.id)}
          >
            {task.done ? "[x]" : "[ ]"} {task.title}
          </li>
        ))}
      </ul>
    </div>
  );
}
```

### Emitted Rust

```rust
use dioxus::prelude::*;

#[derive(Props, PartialEq, Clone)]
pub struct AppProps {
    pub initial_tasks: Vec<Task>,
}

pub fn app(props: AppProps) -> Element {
    let mut tasks = use_signal(|| props.initial_tasks.clone());
    let mut input = use_signal(|| String::new());
    let mut filter = use_signal(|| Filter::All);

    let filtered = use_memo(move || {
        tasks.read().iter().filter(|t| match *filter.read() {
            Filter::Active => !t.done,
            Filter::Done => t.done,
            Filter::All => true,
        }).cloned().collect::<Vec<_>>()
    });

    let add_task = move || {
        if input.read().is_empty() { return; }
        let new_task = Task {
            id: tasks.read().len() as i32 + 1,
            title: input.read().clone(),
            done: false,
        };
        tasks.write().push(new_task);
        input.set(String::new());
    };

    let toggle = move |id: i32| {
        tasks.write().iter_mut().for_each(|t| {
            if t.id == id { t.done = !t.done; }
        });
    };

    rsx! {
        div { class: "app",
            div { class: "filters",
                {([Filter::All, Filter::Active, Filter::Done]).into_iter().map(|f| {
                    let active = *filter.read() == f;
                    rsx! {
                        button {
                            class: if active { "active" } else { "" },
                            onclick: move |_| filter.set(f),
                            "{f:?}"
                        }
                    }
                })}
            }
            div { class: "input-row",
                input { value: "{input}", oninput: move |e| input.set(e.value()) }
                button { onclick: move |_| add_task(), "Add" }
            }
            ul { class: "task-list",
                {filtered.read().iter().map(|task| {
                    let done = task.done;
                    let id = task.id;
                    rsx! {
                        li {
                            class: if done { "done" } else { "" },
                            onclick: move |_| toggle(id),
                            "{if done { "[x]" } else { "[ ]" }} {task.title}"
                        }
                    }
                })}
            }
        }
    }
}
```

**Zero-overhead note:** TSX transpiles to Dioxus `rsx!` macro. `useSignal` → `use_signal`. Component functions return `Element`. No wrapper—direct Dioxus reactivity system with fine-grained signals.

---

## 6. Desktop: egui (Immediate-Mode Tools)

**Pattern:** Frame-based UI with panels, tables, and interactive widgets.

### Rune TSX (`tool.r.tsx`)

```tsx
import { Ctx, Window, CentralPanel, SidePanel, TopBottomPanel } from "egui";
import { Task } from "./models.r.ts";

interface Props {
  tasks: Task[];
  selected: number | null;
}

export function taskEditor(ctx: Ctx, props: Props): void {
  return (
    <TopBottomPanel top="48px">
      <div style={{ layout: "horizontal", spacing: "8px" }}>
        <button onclick={() => newTask(ctx)}>+ New</button>
        <button onclick={() => saveTasks(ctx)}>Save</button>
      </div>
    </TopBottomPanel>
    <SidePanel left="200px">
      <Window title="Task List">
        {props.tasks.map((task, i) => (
          <selectable
            selected={props.selected === task.id}
            onclick={() => selectTask(ctx, task.id)}
          >
            {task.done ? "☑" : "☐"} {task.title}
          </selectable>
        ))}
      </Window>
    </SidePanel>
    <CentralPanel>
      {props.selected !== null && (
        <Window title="Editor" scroll={true}>
          <input label="Title" value={currentTitle(ctx)} />
          <checkbox label="Done" checked={currentDone(ctx)} />
          <colorPicker label="Tag" color={currentColor(ctx)} />
        </Window>
      )}
    </CentralPanel>
  );
}
```

### Emitted Rust

```rust
use egui::{CentralPanel, Color32, SidePanel, TopBottomPanel, Ui};

pub struct TaskEditorProps<'a> {
    pub tasks: &'a Vec<Task>,
    pub selected: Option<i32>,
}

pub fn task_editor(ctx: &mut egui::Context, props: &TaskEditorProps, ui: &mut Ui) {
    TopBottomPanel::top("top_bar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            if ui.button("+ New").clicked() { new_task(ctx); }
            if ui.button("Save").clicked() { save_tasks(ctx); }
        });
    });

    SidePanel::left("task_list").resizable(true).default_width(200.0).show(ctx, |ui| {
        ui.group(|ui| {
            ui.label("Task List");
            for task in props.tasks.iter() {
                let selected = props.selected == Some(task.id);
                let response = ui.selectable_label(
                    selected,
                    format!("{} {}", if task.done { "☑" } else { "☐" }, task.title)
                );
                if response.clicked() { select_task(ctx, task.id); }
            }
        });
    });

    CentralPanel::default().show(ctx, |ui| {
        if let Some(_id) = props.selected {
            ui.group(|ui| {
                ui.label("Editor");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.text_edit_singleline(&mut current_title(ctx));
                    ui.checkbox(&mut current_done(ctx), "Done");
                    ui.color_edit_button_srgba(&mut current_color(ctx));
                });
            });
        }
    });
}
```

**Zero-overhead note:** TSX maps to egui's closure-based builder API. `<TopBottomPanel>` → `.show(ctx, |ui| ...)`. No retained widget tree—pure immediate-mode calls matching egui's frame architecture.

---

## 7. WASM/Web: Leptos (Full-Stack Reactive)

**Pattern:** Fine-grained signals, server functions, reactive DOM.

### Rune TSX (`app.r.tsx`)

```tsx
import { createSignal, createEffect, For, Show } from "leptos";
import { Task } from "./models.r.ts";

export function TaskApp(): Element {
  const [tasks, setTasks] = createSignal<Task[]>([]);
  const [input, setInput] = createSignal<string>("");
  const [loading, setLoading] = createSignal<boolean>(false);

  createEffect(() => {
    loadTasks().then((data) => setTasks(data));
  });

  const addTask = async () => {
    if (input().length === 0) return;
    setLoading(true);
    const task = await createTaskServer(input());
    setTasks([...tasks(), task]);
    setInput("");
    setLoading(false);
  };

  const toggle = async (id: number) => {
    await toggleTaskServer(id);
    setTasks(tasks().map((t) => t.id === id ? { ...t, done: !t.done } : t));
  };

  return (
    <div class="task-app">
      <Show when={() => !loading()} fallback={<span>Saving...</span>}>
        <div class="input-row">
          <input
            type="text"
            value={input()}
            oninput={(e) => setInput(e.target.value)}
          />
          <button onclick={addTask} disabled={loading()}>
            Add Task
          </button>
        </div>
      </Show>
      <ul class="task-list">
        <For each={tasks}>
          {(task) => (
            <li
              class={task.done ? "done" : ""}
              onclick={() => toggle(task.id)}
            >
              {task.done ? "✓" : "○"} {task.title}
            </li>
          )}
        </For>
      </ul>
    </div>
  );
}
```

### Emitted Rust

```rust
use leptos::*;
use leptos::html::Input;

#[component]
pub fn TaskApp() -> impl IntoView {
    let (tasks, set_tasks) = create_signal(vec![]);
    let (input, set_input) = create_signal(String::new());
    let (loading, set_loading) = create_signal(false);

    create_effect(move |_| {
        let set_tasks = set_tasks.clone();
        spawn_local(async move {
            let data = load_tasks().await;
            set_tasks.set(data);
        });
    });

    let add_task = move || {
        if input.get().is_empty() { return; }
        set_loading.set(true);
        let set_tasks = set_tasks.clone();
        let set_input = set_input.clone();
        let set_loading = set_loading.clone();
        spawn_local(async move {
            let task = create_task_server(input.get()).await;
            set_tasks.update(|t| t.push(task));
            set_input.set(String::new());
            set_loading.set(false);
        });
    };

    let toggle = move |id: i32| {
        let set_tasks = set_tasks.clone();
        spawn_local(async move {
            toggle_task_server(id).await;
            set_tasks.update(|t| {
                if let Some(task) = t.iter_mut().find(|t| t.id == id) {
                    task.done = !task.done;
                }
            });
        });
    };

    view! {
        <div class="task-app">
            <Show when=move || !loading.get() fallback=move || view! { <span>"Saving..."</span> }>
                <div class="input-row">
                    <input
                        type="text"
                        prop:value={move || input.get()}
                        on:input=move |e| set_input.set(event_target_value(&e))
                    />
                    <button
                        on:click=move |_| add_task()
                        disabled={move || loading.get()}
                    >
                        "Add Task"
                    </button>
                </div>
            </Show>
            <ul class="task-list">
                <For each=move || tasks.get() key=|task| task.id let:task>
                    <li
                        class={move || if task.done { "done" } else { "" }}
                        on:click=move |_| toggle(task.id)
                    >
                        {move || if task.done { "✓" } else { "○" }} " " {task.title}
                    </li>
                </For>
            </ul>
        </div>
    }
}
```

**Zero-overhead note:** `createSignal` → Leptos `create_signal`. `<For>` → Leptos `For` component with keyed iteration. `Show` → `Show` component. `view!` macro generated directly. Fine-grained reactivity preserved—no virtual DOM diffing.

---

## 8. WASM/Web: Yew (Component-Based)

**Pattern:** Function components with hooks, callbacks, HTML macro.

### Rune TSX (`app.r.tsx`)

```tsx
import { useState, useEffect, functionComponent } from "yew";
import { Task } from "./models.r.ts";

export function TaskList(): Element {
  const [tasks, setTasks] = useState<Task[]>([]);
  const [draft, setDraft] = useState<string>("");

  useEffect(() => {
    fetchTasks().then((data) => setTasks(data));
  }, []);

  const addTask = () => {
    if (draft.length === 0) return;
    const task: Task = {
      id: tasks.length + 1,
      title: draft,
      done: false,
    };
    setTasks([...tasks, task]);
    setDraft("");
  };

  const toggle = (id: number) => {
    setTasks(tasks.map((t) =>
      t.id === id ? { ...t, done: !t.done } : t
    ));
  };

  return (
    <div className="task-list">
      <div className="controls">
        <input
          type="text"
          value={draft}
          oninput={(e) => setDraft(e.target.value)}
        />
        <button onclick={addTask}>Add</button>
      </div>
      <ul>
        {tasks.map((task) => (
          <li
            key={task.id}
            className={task.done ? "completed" : ""}
            onclick={() => toggle(task.id)}
          >
            <input type="checkbox" checked={task.done} />
            <span>{task.title}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
```

### Emitted Rust

```rust
use yew::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[function_component(TaskList)]
pub fn task_list() -> Html {
    let tasks = use_state(|| vec![]);
    let draft = use_state(|| String::new());

    {
        let tasks = tasks.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                let data = fetch_tasks().await;
                tasks.set(data);
            });
            || ()
        });
    }

    let add_task = {
        let tasks = tasks.clone();
        let draft = draft.clone();
        Callback::from(move |_| {
            if draft.is_empty() { return; }
            let mut new_tasks = (*tasks).clone();
            new_tasks.push(Task {
                id: new_tasks.len() as i32 + 1,
                title: (*draft).clone(),
                done: false,
            });
            tasks.set(new_tasks);
            draft.set(String::new());
        })
    };

    let toggle = {
        let tasks = tasks.clone();
        Callback::from(move |id: i32| {
            let mut new_tasks = (*tasks).clone();
            if let Some(t) = new_tasks.iter_mut().find(|t| t.id == id) {
                t.done = !t.done;
            }
            tasks.set(new_tasks);
        })
    };

    html! {
        <div class="task-list">
            <div class="controls">
                <input
                    type="text"
                    value={(*draft).clone()}
                    oninput={let draft = draft.clone(); Callback::from(move |e: InputEvent| {
                        draft.set(e.target_unchecked_into::<web_sys::HtmlInputElement>().value());
                    })}
                />
                <button onclick={add_task}>{ "Add" }</button>
            </div>
            <ul>
                { for tasks.iter().map(|task| {
                    let id = task.id;
                    let done = task.done;
                    let toggle = toggle.clone();
                    html! {
                        <li
                            key={task.id}
                            class={if done { "completed" } else { "" }}
                            onclick={let toggle = toggle.clone(); Callback::from(move |_| toggle.emit(id))}
                        >
                            <input type="checkbox" checked={done} />
                            <span>{ task.title.clone() }</span>
                        </li>
                    }
                }) }
            </ul>
        </div>
    }
}
```

**Zero-overhead note:** `useState` → Yew `use_state`. `useEffect` → `use_effect_with`. TSX → `html!` macro. Callbacks emitted as Yew `Callback` types. Keyed list rendering via `for` iterator.

---

## 9. Game: Bevy (ECS)

**Pattern:** Systems, components, queries, resources, events.

### Rune (`game.r.ts`)

```typescript
import { App, Plugin, Query, Res, ResMut, EventReader, EventWriter, Commands } from "bevy";
import { Task } from "./models.r.ts";

// Components
export type Position = {
  x: number;
  y: number;
};

export type Velocity = {
  x: number;
  y: number;
};

export type TaskEntity = {
  task: Task;
  priority: number;
};

// Resources
export type GameState = {
  score: number;
  paused: boolean;
};

// Events
export type TaskCompleted = {
  taskId: number;
  reward: number;
};

export function setupGame(app: App): void {
  app.addPlugins(DefaultPlugins)
    .insertResource({ score: 0, paused: false })
    .addEvent(TaskCompleted)
    .addSystem(Update, moveSystem)
    .addSystem(Update, taskCompletionSystem)
    .addSystem(Startup, spawnInitialTasks);
}

function moveSystem(
  query: Query<[Position, Velocity]>,
  time: Res<Time>
): void {
  for (const [pos, vel] of query) {
    pos.x += vel.x * time.deltaSeconds();
    pos.y += vel.y * time.deltaSeconds();
  }
}

function taskCompletionSystem(
  query: Query<TaskEntity>,
  mutState: ResMut<GameState>,
  mutEvents: EventWriter<TaskCompleted>
): void {
  for (const entity of query) {
    if (entity.task.done && entity.priority > 5) {
      mutState.score += entity.priority * 10;
      mutEvents.write({ taskId: entity.task.id, reward: entity.priority * 10 });
    }
  }
}

function spawnInitialTasks(mutCommands: Commands): void {
  mutCommands.spawn({
    task: { id: 1, title: "Collect wood", done: false },
    priority: 3,
    position: { x: 0, y: 0 },
    velocity: { x: 1, y: 0 },
  });
}
```

### Emitted Rust

```rust
use bevy::prelude::*;

#[derive(Component)]
pub struct Position { pub x: f32, pub y: f32 }

#[derive(Component)]
pub struct Velocity { pub x: f32, pub y: f32 }

#[derive(Component)]
pub struct TaskEntity {
    pub task: Task,
    pub priority: i32,
}

#[derive(Resource)]
pub struct GameState {
    pub score: i32,
    pub paused: bool,
}

#[derive(Event)]
pub struct TaskCompleted {
    pub task_id: i32,
    pub reward: i32,
}

pub fn setup_game(app: &mut App) {
    app.add_plugins(DefaultPlugins)
        .insert_resource(GameState { score: 0, paused: false })
        .add_event::<TaskCompleted>()
        .add_systems(Update, (move_system, task_completion_system))
        .add_systems(Startup, spawn_initial_tasks);
}

fn move_system(
    mut query: Query<(&mut Position, &Velocity)>,
    time: Res<Time>,
) {
    for (mut pos, vel) in query.iter_mut() {
        pos.x += vel.x * time.delta_seconds();
        pos.y += vel.y * time.delta_seconds();
    }
}

fn task_completion_system(
    query: Query<&TaskEntity>,
    mut state: ResMut<GameState>,
    mut events: EventWriter<TaskCompleted>,
) {
    for entity in query.iter() {
        if entity.task.done && entity.priority > 5 {
            state.score += entity.priority * 10;
            events.send(TaskCompleted {
                task_id: entity.task.id,
                reward: entity.priority * 10,
            });
        }
    }
}

fn spawn_initial_tasks(mut commands: Commands) {
    commands.spawn((
        TaskEntity {
            task: Task { id: 1, title: String::from("Collect wood"), done: false },
            priority: 3,
        },
        Position { x: 0.0, y: 0.0 },
        Velocity { x: 1.0, y: 0.0 },
    ));
}
```

**Zero-overhead note:** Bevy components become `#[derive(Component)]` structs. `Query<[A, B]>` → `Query<(&A, &mut B)>`. `ResMut`/`Res`/`EventWriter` map to Bevy's system parameters. `Commands.spawn()` emits tuple bundles. ECS scheduling is native Bevy.

---

## 10. Database: SQLx (Compile-Time Checked)

**Pattern:** Async queries with compile-time SQL validation.

### Rune (`db.r.ts`)

```typescript
import { Pool, Postgres, queryAs } from "sqlx";
import { Task } from "./models.r.ts";

export type DbPool = Pool<Postgres>;

export async function initDb(databaseUrl: string): DbPool {
  return Pool.connect(databaseUrl);
}

export async function getTaskById(pool: DbPool, id: number): Task | null {
  const rows = await queryAs<Task>(
    pool,
    "SELECT id, title, done FROM tasks WHERE id = $1",
    [id]
  );
  return rows.length > 0 ? rows[0] : null;
}

export async function createTask(pool: DbPool, title: string): Task {
  const rows = await queryAs<Task>(
    pool,
    "INSERT INTO tasks (title, done) VALUES ($1, false) RETURNING id, title, done",
    [title]
  );
  return rows[0];
}

export async function listTasks(pool: DbPool): Task[] {
  return await queryAs<Task>(
    pool,
    "SELECT id, title, done FROM tasks ORDER BY id"
  );
}
```

### Emitted Rust

```rust
use sqlx::{PgPool, query_as};

pub type DbPool = PgPool;

pub async fn init_db(database_url: &str) -> Result<DbPool, sqlx::Error> {
    PgPool::connect(database_url).await
}

pub async fn get_task_by_id(pool: &DbPool, id: i32) -> Result<Option<Task>, sqlx::Error> {
    let row = query_as::<_, Task>(
        "SELECT id, title, done FROM tasks WHERE id = $1"
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn create_task(pool: &DbPool, title: &str) -> Result<Task, sqlx::Error> {
    let task = query_as::<_, Task>(
        "INSERT INTO tasks (title, done) VALUES ($1, false) RETURNING id, title, done"
    )
    .bind(title)
    .fetch_one(pool)
    .await?;
    Ok(task)
}

pub async fn list_tasks(pool: &DbPool) -> Result<Vec<Task>, sqlx::Error> {
    let tasks = query_as::<_, Task>(
        "SELECT id, title, done FROM tasks ORDER BY id"
    )
    .fetch_all(pool)
    .await?;
    Ok(tasks)
}
```

**Zero-overhead note:** `queryAs<Task>` maps to `query_as::<_, Task>()`. Compile-time SQL checking preserved via SQLx macros. `?` operator emitted for error propagation. `Pool.connect()` → `PgPool::connect()`.

---

## 11. Networking: Tonic (gRPC)

**Pattern:** Service definition, request/response handlers, streaming.

### Rune (`service.r.ts`)

```typescript
import { Request, Response, Streaming } from "tonic";
import { Task } from "./models.r.ts";

export type TaskService = {
  getTask: (req: Request<{ id: number }>) => Response<Task>;
  listTasks: (req: Request<{}>) => Streaming<Task>;
  createTask: (req: Request<{ title: string }>) => Response<Task>;
};

export function taskService(): TaskService {
  return {
    getTask: handleGetTask,
    listTasks: handleListTasks,
    createTask: handleCreateTask,
  };
}

async function handleGetTask(
  req: Request<{ id: number }>
): Response<Task> {
  const task = await dbGetTask(req.id);
  if (task === null) {
    return Response.notFound("Task not found");
  }
  return Response.ok(task);
}

async function handleListTasks(
  _req: Request<{}>
): Streaming<Task> {
  const tasks = await dbListTasks();
  return Streaming.from(tasks);
}

async function handleCreateTask(
  req: Request<{ title: string }>
): Response<Task> {
  const task = await dbCreateTask(req.title);
  return Response.ok(task);
}
```

### Emitted Rust

```rust
use tonic::{Request, Response, Status, Streaming};
use tokio_stream::wrappers::ReceiverStream;

pub struct TaskServiceImpl;

#[tonic::async_trait]
impl task_service_server::TaskService for TaskServiceImpl {
    async fn get_task(
        &self,
        request: Request<GetTaskRequest>,
    ) -> Result<Response<Task>, Status> {
        let id = request.into_inner().id;
        match db_get_task(id).await {
            Some(task) => Ok(Response::new(task)),
            None => Err(Status::not_found("Task not found")),
        }
    }

    type ListTasksStream = ReceiverStream<Result<Task, Status>>;

    async fn list_tasks(
        &self,
        _request: Request<ListTasksRequest>,
    ) -> Result<Response<Self::ListTasksStream>, Status> {
        let tasks = db_list_tasks().await;
        let (tx, rx) = tokio::sync::mpsc::channel(128);
        tokio::spawn(async move {
            for task in tasks {
                let _ = tx.send(Ok(task)).await;
            }
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn create_task(
        &self,
        request: Request<CreateTaskRequest>,
    ) -> Result<Response<Task>, Status> {
        let title = request.into_inner().title;
        let task = db_create_task(&title).await;
        Ok(Response::new(task))
    }
}
```

**Zero-overhead note:** Service trait implementation generated directly. `Request<T>` and `Response<T>` map to Tonic's types. Streaming uses `tokio_stream::wrappers::ReceiverStream`. `#[tonic::async_trait]` applied automatically.

---

## 12. AI/ML: Candle (LLM Inference)

**Pattern:** Model loading, tokenization, inference loop, tensor ops.

### Rune (`infer.r.ts`)

```typescript
import { Device, Tensor, SafeTensors } from "candle_core";
import { Tokenizer } from "tokenizers";

export type LlamaModel = {
  tensors: SafeTensors;
  device: Device;
};

export async function loadLlama(weightsPath: string, device: Device): LlamaModel {
  return {
    tensors: SafeTensors.load(weightsPath),
    device,
  };
}

export function loadTokenizer(path: string): Tokenizer {
  return Tokenizer.fromFile(path);
}

export async function complete(
  model: LlamaModel,
  tokenizer: Tokenizer,
  prompt: string,
  maxLen: number
): string {
  let tokens = tokenizer.encode(prompt).ids();

  for (let pos = 0; pos < maxLen; pos++) {
    const input = Tensor.new(tokens, [1, tokens.length], "u32", model.device);
    const logits = forwardLlama(model, input);
    const nextToken = logits.get(0).argmax(1).toScalarU32();

    if (nextToken === tokenizer.tokenToId("</s>")) {
      break;
    }
    tokens.push(nextToken);
  }

  return tokenizer.decode(tokens, true);
}

function forwardLlama(model: LlamaModel, input: Tensor): Tensor {
  return nativeLlamaForward(model.tensors, input);
}
```

### Emitted Rust

```rust
use candle_core::{Device, Tensor, SafeTensors, DType};
use tokenizers::Tokenizer;

pub struct LlamaModel {
    pub tensors: SafeTensors,
    pub device: Device,
}

pub fn load_llama(weights_path: &str, device: &Device) -> Result<LlamaModel, candle_core::Error> {
    let tensors = unsafe { SafeTensors::mmap(weights_path)? };
    Ok(LlamaModel { tensors, device: device.clone() })
}

pub fn load_tokenizer(path: &str) -> Result<Tokenizer, Box<dyn std::error::Error>> {
    Ok(Tokenizer::from_file(path)?)
}

pub fn complete(
    model: &LlamaModel,
    tokenizer: &Tokenizer,
    prompt: &str,
    max_len: usize,
) -> Result<String, Box<dyn std::error::Error>> {
    let encoding = tokenizer.encode(prompt, true).unwrap();
    let mut tokens = encoding.get_ids().to_vec();

    for _ in 0..max_len {
        let input = Tensor::new(tokens.as_slice(), &model.device)?
            .reshape((1, tokens.len()))?;
        let logits = native_llama_forward(&model.tensors, &input)?;
        let next_token = logits.get(0)?.argmax(1)?.to_scalar::<u32>()? as usize;

        if next_token == tokenizer.token_to_id("</s>").unwrap_or(0) as usize {
            break;
        }
        tokens.push(next_token as u32);
    }

    Ok(tokenizer.decode(&tokens, true).unwrap())
}
```

**Zero-overhead note:** `Tensor.new()` maps to `Tensor::new()`. `argmax(1)` → `argmax(1)?`. `SafeTensors` loaded via memory-mapped files. Tokenizer from HuggingFace `tokenizers` crate. Model-specific `forward` delegated to native Rust for architecture-specific optimizations.

---

## Summary: Zero-Overhead Principles

| Framework | Rune Abstraction | Rust Emission | Overhead |
|---|---|---|---|
| Axum | `State<T>`, `Json<T>` | Direct extractors | Zero |
| Actix-web | `web.Data<T>`, `HttpResponse` | Direct extractors/responses | Zero |
| Ratatui | TSX `<List>`, `<Layout>` | Direct widget builders | Zero |
| Tauri | `State<T>`, `Window` | `#[tauri::command]` + managed state | Zero |
| Dioxus | TSX `rsx!` generation | Direct signal + component system | Zero |
| egui | TSX immediate-mode | Direct `ui.button()` calls | Zero |
| Leptos | TSX `view!` generation | Fine-grained reactive DOM | Zero |
| Yew | TSX `html!` generation | `Callback` + component system | Zero |
| Bevy | ECS queries/resources | `#[derive(Component)]` + systems | Zero |
| SQLx | `queryAs<T>()` | `query_as!()` compile-time checked | Zero |
| Tonic | Service trait objects | `#[tonic::async_trait]` impl | Zero |
| Candle | Tensor ops | Direct `Tensor::new()` + `argmax()` | Zero |

**Key insight:** Rune never introduces wrapper types, virtual dispatch layers, or runtime translation. The TS/TSX you write is statically analyzed and emitted as the exact Rust code you would have written by hand, using each framework's native APIs directly.
