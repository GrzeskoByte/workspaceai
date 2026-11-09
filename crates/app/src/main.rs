//! WorkspaceAI native app (GPUI). Slice A: workspaces, projects, chats, messages and sending.
//! The window renders `workspaceai_client::model::Model`; background threads talk to the supervisor and report
//! back through one channel that the window drains on a timer.

mod flow;
mod md_view;
mod onboarding;
mod pages;
mod panels;
mod terminal_view;
mod text_input;
mod ui;
mod webview;

use gpui::{
    actions, div, prelude::*, px, rgb, rgba, size, App, Bounds, ExternalPaths, PathPromptOptions, KeyBinding, Context, Entity, Focusable, FontWeight, ScrollHandle, SharedString, Window, WindowBounds,
    WindowOptions,
};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;
use text_input::{TextInput, TextInputEvent};
use workspaceai_client::api::{seg, Api};
use workspaceai_client::events;
use workspaceai_client::facts;
use workspaceai_client::files;
use workspaceai_client::model::{Connection, Model, RowKind};
use workspaceai_client::models::ModelList;
use workspaceai_client::palette::{self, Kind};
use workspaceai_client::projects;
use workspaceai_client::providers::{self, KeyState};
use workspaceai_client::routines;
use workspaceai_client::settings::{self, engine_actions, EngineAction};
use workspaceai_client::supervisor::Supervisor;
use workspaceai_client::chats::{self, Confirm, InFlight};
use workspaceai_client::computers;
use workspaceai_client::connectors;
use workspaceai_client::subscriptions::{self, SignIn, SubAction};
use workspaceai_client::command::{self, Outcome};
use workspaceai_client::library;
use workspaceai_client::markdown;
use workspaceai_shared::commands::{insert_mention, mention_at, parse_slash, ChatCommand};
use workspaceai_client::types::{AppState, Chat, ChatMessage, PermissionRequest, Snapshot, Subscription, UiEvent};

const BG: u32 = 0x1b1b1f;
const PANEL: u32 = 0x232329;
const RAIL: u32 = 0x151518;
const TEXT: u32 = 0xe6e6e8;
const MUTED: u32 = 0x8b8b95;
const ACCENT: u32 = 0xd97757;
const USER_BUBBLE: u32 = 0x33333b;
const RAISED_OFF: u32 = 0x2a2a30;

/// Everything a background thread can tell the window.
enum Msg {
    Ready(Api, Supervisor),
    StartFailed(String),
    Event(UiEvent),
    StreamLost(String),
    State(AppState),
    Snapshot(String, Snapshot),
    Messages(String, String, Vec<ChatMessage>),
    Permissions(String, String, Vec<PermissionRequest>),
    Commands(String, String, Vec<ChatCommand>),
    CommandDone(Outcome),
    Models(String, ModelList),
    Files(String, Vec<String>),
    Secrets(String, Vec<String>),
    /// An artifact's content, and the page Open Design stores for it (empty when none).
    ArtifactView(String, Result<library::View, String>, String),
    Subscriptions(String, Vec<Subscription>),
    ChatCreated(String, Chat),
    Pairing(Option<computers::Pairing>),
    ProviderCheck(String),
    /// A reply for the Settings pages in `pages.rs`: what it answers, the workspace, the JSON.
    Fetched(&'static str, String, serde_json::Value),
    SignIn(String, SignIn),
    /// A reply for the setup wizard and the new-workspace dialog (`onboarding.rs`).
    Onb(&'static str, serde_json::Value),
    Error(String),
}

fn repo_root() -> PathBuf {
    std::env::var_os("HARNESS_ROOT").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

fn node_binary() -> PathBuf {
    std::env::var_os("HARNESS_NODE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("node"))
}

fn spawn<F: FnOnce() + Send + 'static>(f: F) {
    std::thread::spawn(f);
}

fn fetch<T: serde::de::DeserializeOwned>(api: &Api, path: &str) -> Result<T, String> {
    let v = api.get(path).map_err(|e| e.to_string())?;
    serde_json::from_value(v).map_err(|e| format!("unexpected reply from {path}: {e}"))
}

fn load_state(api: Api, tx: Sender<Msg>) {
    spawn(move || {
        let _ = tx.send(match fetch::<AppState>(&api, "/api/state") {
            Ok(s) => Msg::State(s),
            Err(e) => Msg::Error(e),
        });
    });
}

fn load_snapshot(api: Api, ws: String, tx: Sender<Msg>) {
    spawn(move || {
        let _ = tx.send(match fetch::<Snapshot>(&api, &format!("/api/ws/{}/snapshot", seg(&ws))) {
            Ok(s) => Msg::Snapshot(ws, s),
            Err(e) => Msg::Error(e),
        });
    });
}

fn load_messages(api: Api, ws: String, chat: String, tx: Sender<Msg>) {
    spawn(move || {
        let r = api.get(&format!("/api/ws/{}/chats/{}/messages", seg(&ws), seg(&chat))).map_err(|e| e.to_string()).and_then(|v| {
            serde_json::from_value::<Vec<ChatMessage>>(v["messages"].clone()).map_err(|e| format!("unexpected messages reply: {e}"))
        });
        let _ = tx.send(match r {
            Ok(m) => Msg::Messages(ws, chat, m),
            Err(e) => Msg::Error(e),
        });
    });
}

fn load_permissions(api: Api, ws: String, chat: String, tx: Sender<Msg>) {
    spawn(move || {
        let r = api.get(&format!("/api/ws/{}/chats/{}/permissions", seg(&ws), seg(&chat))).map_err(|e| e.to_string()).and_then(|v| {
            serde_json::from_value::<Vec<PermissionRequest>>(v["permissions"].clone()).map_err(|e| format!("unexpected permissions reply: {e}"))
        });
        // A failure here only means no cards until the next event; it is not worth a toast.
        if let Ok(list) = r {
            let _ = tx.send(Msg::Permissions(ws, chat, list));
        }
    });
}

fn search_files(api: Api, ws: String, project: String, query: String, tx: Sender<Msg>) {
    spawn(move || {
        if let Ok(v) = api.get(&files::search_path(&ws, &project, &query)) {
            let _ = tx.send(Msg::Files(query, files::parse_files(&v)));
        }
    });
}

fn fork_chat(api: Api, ws: String, chat: String, tx: Sender<Msg>) {
    spawn(move || {
        let r = api.post(&format!("/api/ws/{}/chats/{}/fork", seg(&ws), seg(&chat)), None).map_err(|e| e.to_string()).and_then(|v| serde_json::from_value::<Chat>(v).map_err(|e| e.to_string()));
        let _ = tx.send(match r {
            Ok(c) => Msg::ChatCreated(ws, c),
            Err(e) => Msg::Error(e),
        });
    });
}

fn delete_chat(api: Api, ws: String, chat: String, tx: Sender<Msg>) {
    spawn(move || match api.del(&format!("/api/ws/{}/chats/{}", seg(&ws), seg(&chat))) {
        Ok(_) => load_snapshot(api, ws, tx),
        Err(e) => {
            let _ = tx.send(Msg::Error(e.to_string()));
        }
    });
}

/// Changes one workspace setting; the reply is the fresh snapshot.
fn patch_workspace(api: Api, ws: String, body: serde_json::Value, tx: Sender<Msg>) {
    spawn(move || {
        let _ = tx.send(match api.patch(&format!("/api/ws/{}", seg(&ws)), &body).map_err(|e| e.to_string()).and_then(|v| serde_json::from_value::<Snapshot>(v).map_err(|e| e.to_string())) {
            Ok(snap) => Msg::Snapshot(ws, snap),
            Err(e) => Msg::Error(e),
        });
    });
}

fn engine_action(api: Api, ws: String, action: EngineAction, tx: Sender<Msg>) {
    spawn(move || {
        if let Err(e) = api.post(&action.path(&ws), None) {
            let _ = tx.send(Msg::Error(e.to_string()));
        }
        load_snapshot(api, ws, tx);
    });
}

/// Any change that is answered by a fresh snapshot (facts, and later routines, projects…).
fn mutate(api: Api, ws: String, method: &'static str, path: String, body: Option<serde_json::Value>, tx: Sender<Msg>) {
    spawn(move || {
        if let Err(e) = api.request(method, &path, body.as_ref()) {
            let _ = tx.send(Msg::Error(format!("Could not save the change: {e}")));
        }
        load_snapshot(api, ws, tx);
    });
}

fn load_artifact(api: Api, ws: String, artifact: String, version: Option<u32>, tx: Sender<Msg>) {
    spawn(move || {
        let reply = api.get(&library::content_path(&ws, &artifact, version)).map_err(|e| e.to_string());
        let page = reply.as_ref().map(workspaceai_client::web::page_of).unwrap_or_default();
        let r = reply.and_then(|v| library::parse_view(&v).ok_or_else(|| "The artifact has no content.".to_string()));
        let _ = tx.send(Msg::ArtifactView(artifact, r, page));
    });
}

/// Detecting installed agents runs their CLIs, so this can take a few seconds.
fn load_subscriptions(api: Api, ws: String, tx: Sender<Msg>) {
    spawn(move || {
        if let Ok(v) = api.get(&subscriptions::list_path(&ws)) {
            let _ = tx.send(Msg::Subscriptions(ws, subscriptions::parse_list(&v)));
        }
    });
}

fn change_subscription(api: Api, ws: String, path: String, tx: Sender<Msg>) {
    spawn(move || {
        if let Err(e) = api.post(&path, None) {
            let _ = tx.send(Msg::Error(format!("Could not change the connection: {e}")));
        }
        load_subscriptions(api, ws, tx);
    });
}

fn request_pairing(api: Api, tx: Sender<Msg>) {
    spawn(move || match api.post(computers::PAIRING_PATH, None) {
        Ok(v) => {
            let _ = tx.send(Msg::Pairing(computers::parse_pairing(&v)));
        }
        Err(e) => {
            let _ = tx.send(Msg::Error(format!("Could not get a pairing code: {e}")));
        }
    });
}

fn test_provider(api: Api, ws: String, body: serde_json::Value, tx: Sender<Msg>) {
    spawn(move || {
        let model = body["model"].as_str().unwrap_or("").to_string();
        let text = match api.post(&providers::test_path(&ws), Some(&body)) {
            Ok(v) => providers::check_text(&v, &model),
            Err(e) => e.message,
        };
        let _ = tx.send(Msg::ProviderCheck(text));
    });
}

/// Starts the browser sign-in, then watches it until it is no longer waiting (10 minutes at most, like the server).
fn start_signin(api: Api, ws: String, sub: String, tx: Sender<Msg>) {
    spawn(move || {
        let path = subscriptions::signin_path(&ws, &sub);
        let mut state = match api.post(&path, None) {
            Ok(v) => subscriptions::parse_signin(&v),
            Err(e) => SignIn { state: "failed".into(), url: None, error: Some(e.message) },
        };
        let _ = tx.send(Msg::SignIn(sub.clone(), state.clone()));
        for _ in 0..400 {
            if !state.waiting() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1500));
            match api.get(&path) {
                Ok(v) => state = subscriptions::parse_signin(&v),
                Err(_) => continue,
            }
            let _ = tx.send(Msg::SignIn(sub.clone(), state.clone()));
        }
        load_subscriptions(api, ws, tx);
    });
}

fn signin_call(api: Api, ws: String, sub: String, path: String, body: Option<serde_json::Value>, tx: Sender<Msg>) {
    spawn(move || match api.post(&path, body.as_ref()) {
        Ok(v) => {
            let _ = tx.send(Msg::SignIn(sub, subscriptions::parse_signin(&v)));
            load_subscriptions(api, ws, tx);
        }
        Err(e) => {
            let _ = tx.send(Msg::Error(format!("Sign-in: {e}")));
        }
    });
}

/// Names of the secrets stored for a workspace (never their values).
fn load_secrets(api: Api, ws: String, tx: Sender<Msg>) {
    spawn(move || {
        if let Ok(v) = api.get(&providers::secrets_list_path(&ws)) {
            let names = v["names"].as_array().map(|a| a.iter().filter_map(|n| n.as_str().map(str::to_string)).collect()).unwrap_or_default();
            let _ = tx.send(Msg::Secrets(ws, names));
        }
    });
}

/// Saves or removes a secret, then refreshes which ones exist. The value is only ever in `body`.
fn change_secret(api: Api, ws: String, method: &'static str, path: String, body: Option<serde_json::Value>, tx: Sender<Msg>) {
    spawn(move || {
        if let Err(e) = api.request(method, &path, body.as_ref()) {
            let _ = tx.send(Msg::Error(format!("Could not change the key: {e}")));
        }
        load_secrets(api, ws, tx);
    });
}

/// Like `mutate`, for changes answered by fresh app state (computers) instead of a workspace snapshot.
fn mutate_state(api: Api, method: &'static str, path: String, body: Option<serde_json::Value>, tx: Sender<Msg>) {
    spawn(move || {
        if let Err(e) = api.request(method, &path, body.as_ref()) {
            let _ = tx.send(Msg::Error(format!("Could not save the change: {e}")));
        }
        load_state(api, tx);
    });
}

fn load_models(api: Api, ws: String, tx: Sender<Msg>) {
    spawn(move || {
        // The list is slow when an engine has to start; a failure just leaves the picker empty.
        if let Ok(list) = fetch::<ModelList>(&api, &format!("/api/ws/{}/models", seg(&ws))) {
            let _ = tx.send(Msg::Models(ws, list));
        }
    });
}

/// Changes a chat's model or effort, then refreshes the snapshot so every view agrees.
fn patch_chat(api: Api, ws: String, chat: String, body: serde_json::Value, tx: Sender<Msg>) {
    spawn(move || match api.patch(&format!("/api/ws/{}/chats/{}", seg(&ws), seg(&chat)), &body) {
        Ok(_) => load_snapshot(api, ws, tx),
        Err(e) => {
            // Whatever the window assumed, show what the server really has.
            let _ = tx.send(Msg::Error(format!("Could not save the change: {e}. Showing what the server has.")));
            load_snapshot(api, ws, tx);
        }
    });
}

fn load_commands(api: Api, ws: String, chat: String, tx: Sender<Msg>) {
    spawn(move || {
        let r = api.get(&format!("/api/ws/{}/chats/{}/commands", seg(&ws), seg(&chat))).map_err(|e| e.to_string()).and_then(|v| {
            serde_json::from_value::<Vec<ChatCommand>>(v["commands"].clone()).map_err(|e| format!("unexpected commands reply: {e}"))
        });
        if let Ok(list) = r {
            let _ = tx.send(Msg::Commands(ws, chat, list));
        }
    });
}

fn run_command(api: Api, ws: String, chat: String, text: String, tx: Sender<Msg>) {
    spawn(move || {
        let _ = tx.send(match api.post(&format!("/api/ws/{}/chats/{}/command", seg(&ws), seg(&chat)), Some(&serde_json::json!({ "text": text }))) {
            Ok(v) => Msg::CommandDone(command::interpret(&v)),
            Err(e) => Msg::Error(e.to_string()),
        });
    });
}

fn answer_permission(api: Api, ws: String, chat: String, request: String, decision: &'static str, tx: Sender<Msg>) {
    spawn(move || {
        if let Err(e) = api.post(&format!("/api/ws/{}/chats/{}/permission/{}", seg(&ws), seg(&chat), seg(&request)), Some(&serde_json::json!({ "decision": decision }))) {
            let _ = tx.send(Msg::Error(e.to_string()));
            load_permissions(api, ws, chat, tx);
        }
    });
}

fn stop_chat(api: Api, ws: String, chat: String, tx: Sender<Msg>) {
    spawn(move || {
        if let Err(e) = api.post(&format!("/api/ws/{}/chats/{}/stop", seg(&ws), seg(&chat)), None) {
            let _ = tx.send(Msg::Error(e.to_string()));
        }
    });
}

fn send_text(api: Api, ws: String, chat: String, text: String, tx: Sender<Msg>) {
    spawn(move || {
        if let Err(e) = api.post(&format!("/api/ws/{}/chats/{}/send", seg(&ws), seg(&chat)), Some(&serde_json::json!({ "text": text }))) {
            let _ = tx.send(Msg::Error(e.to_string()));
        }
    });
}

fn create_chat(api: Api, ws: String, project: String, text: Option<String>, tx: Sender<Msg>) {
    spawn(move || {
        let mut body = serde_json::json!({ "project": project });
        if let Some(t) = text {
            body["text"] = t.into();
        }
        let r = api.post(&format!("/api/ws/{}/chats", seg(&ws)), Some(&body)).map_err(|e| e.to_string()).and_then(|v| serde_json::from_value::<Chat>(v).map_err(|e| e.to_string()));
        let _ = tx.send(match r {
            Ok(c) => Msg::ChatCreated(ws, c),
            Err(e) => Msg::Error(e),
        });
    });
}

fn activate(api: Api, ws: String, tx: Sender<Msg>) {
    spawn(move || {
        let _ = tx.send(match api.post(&format!("/api/workspaces/{}/activate", seg(&ws)), None).map_err(|e| e.to_string()).and_then(|v| serde_json::from_value::<AppState>(v).map_err(|e| e.to_string())) {
            Ok(s) => Msg::State(s),
            Err(e) => Msg::Error(e),
        });
    });
}

/// Streams events forever, reconnecting after a drop.
fn run_events(api: Api, tx: Sender<Msg>) {
    spawn(move || loop {
        let sent = tx.clone();
        let r = events::stream(&api, move |e| {
            let _ = sent.send(Msg::Event(e));
        });
        let _ = tx.send(Msg::StreamLost(r.err().unwrap_or_else(|| "the event stream closed".into())));
        std::thread::sleep(Duration::from_millis(1500));
    });
}

actions!(app, [OpenPalette, ClosePalette, PaletteUp, PaletteDown]);

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Chat,
    Home,
    Create,
    Approvals,
    Project,
    Settings,
    Library,
}

struct AppView {
    palette_open: bool,
    palette_sel: usize,
    palette_input: Entity<TextInput>,
    view: View,
    settings_page: &'static str,
    name_input: Entity<TextInput>,
    fact_input: Entity<TextInput>,
    library_search: Entity<TextInput>,
    library_sel: Option<String>,
    library_view: Option<Result<library::View, String>>,
    library_page: String,
    side: panels::Side,
    secret_input: Entity<TextInput>,
    secret_names: Vec<String>,
    subs: Vec<Subscription>,
    signin: Option<(String, SignIn)>,
    signin_code: Entity<TextInput>,
    mcp_name: Entity<TextInput>,
    mcp_target: Entity<TextInput>,
    pairing: Option<computers::Pairing>,
    provider_form: Option<[Entity<TextInput>; 6]>,
    provider_check: String,
    project_input: Entity<TextInput>,
    show_archived: bool,
    move_open: bool,
    show_archived_chats: bool,
    routine_phrase: Entity<TextInput>,
    routine_prompt: Entity<TextInput>,
    extra: pages::Extra,
    flow: flow::Flow,
    onb: onboarding::Onb,
    model: Model,
    rx: Receiver<Msg>,
    tx: Sender<Msg>,
    api: Option<Api>,
    _supervisor: Option<Supervisor>,
    input: Entity<TextInput>,
    rename_input: Entity<TextInput>,
    renaming: bool,
    confirm: Confirm,
    in_flight: InFlight,
    scroll: ScrollHandle,
    local_id: u64,
    commands: Vec<ChatCommand>,
    models: ModelList,
    picker_open: bool,
    /// Files matching the `@` reference being typed, and the query they answer.
    files: Vec<String>,
    last_query: Option<String>,
    /// Information from a command (status, usage, help), shown above the composer until dismissed.
    panel: Option<(String, String)>,
}

impl AppView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (tx, rx) = channel();
        let start_tx = tx.clone();
        spawn(move || {
            let _ = start_tx.send(match Supervisor::start(&repo_root(), &node_binary()) {
                Ok((info, sup)) => Msg::Ready(Api::new(&info), sup),
                Err(e) => Msg::StartFailed(e),
            });
        });
        let input = cx.new(|cx| TextInput::new(cx, "Message…"));
        let rename_input = cx.new(|cx| TextInput::new(cx, "Chat title"));
        let palette_input = cx.new(|cx| TextInput::new(cx, "Jump to a chat, workspace or setting…"));
        cx.subscribe_in(&palette_input, window, |this, _, event: &TextInputEvent, window, cx| {
            if *event == TextInputEvent::Submit {
                this.run_palette(window, cx);
            }
        })
        .detach();
        cx.observe(&palette_input, |this, _, cx| {
            this.palette_sel = 0;
            cx.notify();
        })
        .detach();
        let library_search = cx.new(|cx| TextInput::new(cx, "Search artifacts"));
        cx.observe(&library_search, |_, _, cx| cx.notify()).detach();
        let secret_input = cx.new(|cx| TextInput::new_masked(cx, "Paste an API key"));
        cx.observe(&secret_input, |_, _, cx| cx.notify()).detach();
        let signin_code = cx.new(|cx| TextInput::new(cx, "Code from the sign-in page"));
        let mcp_name = cx.new(|cx| TextInput::new(cx, "Name (optional)"));
        let mcp_target = cx.new(|cx| TextInput::new(cx, "https://… or a command, e.g. npx -y server-name"));
        for input in [&signin_code, &mcp_name, &mcp_target] {
            cx.observe(input, |_, _, cx| cx.notify()).detach();
        }
        let project_input = cx.new(|cx| TextInput::new(cx, "Project name"));
        cx.subscribe_in(&project_input, window, |this, input, event: &TextInputEvent, window, cx| {
            if *event == TextInputEvent::Submit {
                let name = input.read(cx).content().to_string();
                if let (Some(body), Some(ws)) = (projects::create_body(&name, None), this.model.active_ws.clone()) {
                    this.mutate_path("POST", projects::create_path(&ws), Some(body));
                    input.update(cx, |i, cx| i.clear(window, cx));
                }
            }
        })
        .detach();
        let routine_phrase = cx.new(|cx| TextInput::new(cx, "When? e.g. every weekday at 9"));
        let routine_prompt = cx.new(|cx| TextInput::new(cx, "What should it do?"));
        for input in [&routine_phrase, &routine_prompt] {
            cx.observe(input, |_, _, cx| cx.notify()).detach();
        }
        let fact_input = cx.new(|cx| TextInput::new(cx, "Something to remember…"));
        cx.subscribe_in(&fact_input, window, |this, input, event: &TextInputEvent, window, cx| {
            if *event == TextInputEvent::Submit {
                let text = input.read(cx).content().to_string();
                if let Some(body) = facts::add_body(&text, "ws") {
                    this.mutate_ws("POST", "/facts".into(), Some(body));
                    input.update(cx, |i, cx| i.clear(window, cx));
                }
            }
        })
        .detach();
        let name_input = cx.new(|cx| TextInput::new(cx, "Workspace name"));
        cx.subscribe_in(&name_input, window, |this, input, event: &TextInputEvent, _window, cx| {
            if *event == TextInputEvent::Submit {
                let name = input.read(cx).content().trim().to_string();
                if !name.is_empty() {
                    this.patch_ws(settings::patch("name", name.into()));
                }
            }
        })
        .detach();
        cx.subscribe_in(&rename_input, window, |this, input, event: &TextInputEvent, window, cx| {
            if *event == TextInputEvent::Submit {
                let title = input.read(cx).content().to_string();
                this.rename_to(&title);
                this.renaming = false;
                window.focus(&this.input.focus_handle(cx), cx);
                cx.notify();
            }
        })
        .detach();
        cx.subscribe_in(&input, window, |this, input, event: &TextInputEvent, window, cx| {
            if *event == TextInputEvent::Submit {
                this.submit(input, window, cx);
            }
        })
        .detach();
        cx.observe(&input, |this, _, cx| {
            this.on_input_changed(cx);
            cx.notify();
        })
        .detach();
        window.focus(&input.focus_handle(cx), cx);
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(50)).await;
            if this.update(cx, |this, cx| this.pump(cx)).is_err() {
                break;
            }
        })
        .detach();
        let extra = pages::Extra::new(cx);
        let flow = flow::Flow::new(cx);
        cx.subscribe_in(&flow.home_input, window, |this, _, event: &TextInputEvent, _window, cx| {
            if *event == TextInputEvent::Submit {
                this.send_home(cx);
                cx.notify();
            }
        })
        .detach();
        let onb = onboarding::Onb::new(cx);
        let side = panels::Side::new(cx);
        cx.subscribe_in(&side.address, window, |this, input, event: &TextInputEvent, _window, cx| {
            if *event == TextInputEvent::Submit {
                let typed = input.read(cx).content().to_string();
                this.browse(&typed);
                cx.notify();
            }
        })
        .detach();
        #[cfg(feature = "webview")]
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(16)).await;
            if this.update(cx, |this, cx| this.web_tick(cx)).is_err() {
                break;
            }
        })
        .detach();
        AppView { extra, flow, onb, palette_open: false, palette_sel: 0, palette_input, view: View::Chat, settings_page: "general", name_input, fact_input, library_search, library_sel: None, library_view: None, library_page: String::new(), side, secret_input, secret_names: Vec::new(), subs: Vec::new(), signin: None, signin_code, mcp_name, mcp_target, pairing: None, provider_form: None, provider_check: String::new(), project_input, show_archived: false, move_open: false, show_archived_chats: false, routine_phrase, routine_prompt, model: Model::default(), rx, tx, api: None, _supervisor: None, input, rename_input, renaming: false, confirm: Confirm::default(), in_flight: InFlight::default(), scroll: ScrollHandle::new(), local_id: 0, commands: Vec::new(), models: ModelList::default(), picker_open: false, files: Vec::new(), last_query: None, panel: None }
    }

    /// Drains background messages into the model and refetches whatever events made stale.
    fn pump(&mut self, cx: &mut Context<Self>) {
        self.poll();
        let mut changed = false;
        while let Ok(msg) = self.rx.try_recv() {
            changed = true;
            self.handle(msg);
        }
        if let Some(api) = self.api.clone() {
            if std::mem::take(&mut self.model.needs.state) {
                load_state(api.clone(), self.tx.clone());
            }
            if std::mem::take(&mut self.model.needs.snapshot) {
                if let Some(ws) = self.model.active_ws.clone() {
                    load_snapshot(api.clone(), ws, self.tx.clone());
                }
            }
            if std::mem::take(&mut self.model.needs.messages) {
                if let (Some(ws), Some(chat)) = (self.model.active_ws.clone(), self.model.open_chat_id.clone()) {
                    load_messages(api, ws, chat, self.tx.clone());
                }
            }
        }
        if self.onb.loaded && !self.extra.app.onboarded {
            self.onb_start(cx);
            self.onb_enter(cx);
            self.onb_poll();
        }
        self.flow_pump(cx);
        self.side_pump(cx);
        if changed {
            self.scroll.scroll_to_bottom();
            cx.notify();
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Ready(api, sup) => {
                self.model.status = Connection::Connected;
                run_events(api.clone(), self.tx.clone());
                load_state(api.clone(), self.tx.clone());
                self.api = Some(api);
                self._supervisor = Some(sup);
            }
            Msg::StartFailed(e) => self.model.status = Connection::Disconnected(e),
            Msg::Event(e) => {
                if matches!(self.model.status, Connection::Disconnected(_)) {
                    self.model.status = Connection::Connected;
                    self.model.needs.state = true;
                }
                self.model.apply(e);
            }
            Msg::StreamLost(e) => {
                if self.api.is_some() {
                    self.model.status = Connection::Disconnected(e);
                }
            }
            Msg::State(s) => {
                self.extra.app = s.settings.clone();
                self.onb.loaded = true;
                let active = s.settings.active_workspace.clone().or_else(|| s.workspaces.first().map(|w| w.id.clone()));
                let changed = active != self.model.active_ws;
                self.model.workspaces = s.workspaces;
                self.model.computers = s.computers;
                self.model.active_ws = active.clone();
                if changed {
                    self.model.snapshot = Snapshot::default();
                    self.model.open_chat_id = None;
                    self.model.messages.clear();
                }
                if let (Some(api), Some(ws)) = (self.api.clone(), active) {
                    if changed {
                        self.models = ModelList::default();
                        load_models(api.clone(), ws.clone(), self.tx.clone());
                    }
                    load_snapshot(api, ws, self.tx.clone());
                }
            }
            Msg::Snapshot(ws, snap) => {
                self.in_flight.finish_all();
                if self.model.active_ws.as_deref() == Some(&ws) {
                    self.model.snapshot = snap;
                    if self.view == View::Settings && matches!(self.settings_page, "web" | "opendesign" | "data") {
                        self.refresh_page(self.settings_page);
                    }
                    if self.model.open_chat_id.is_none() {
                        if let Some(row) = self.model.sidebar().into_iter().find(|r| r.kind == RowKind::Chat) {
                            self.open_chat(row.id);
                        }
                    }
                }
            }
            Msg::Messages(ws, chat, messages) => {
                if self.model.active_ws.as_deref() == Some(&ws) && self.model.open_chat_id.as_deref() == Some(&chat) {
                    self.model.messages = messages;
                }
            }
            Msg::Permissions(ws, chat, list) => {
                if self.model.active_ws.as_deref() == Some(&ws) {
                    self.model.set_permissions(&chat, list);
                }
            }
            Msg::ArtifactView(artifact, result, page) => {
                if self.library_sel.as_deref() == Some(&artifact) {
                    self.library_view = Some(result);
                    self.library_page = page;
                }
            }
            Msg::Subscriptions(ws, list) => {
                if self.model.active_ws.as_deref() == Some(&ws) {
                    self.subs = list;
                }
            }
            Msg::Pairing(p) => self.pairing = p,
            Msg::ProviderCheck(text) => self.provider_check = text,
            Msg::Fetched(kind, ws, v) => {
                if self.model.active_ws.as_deref() == Some(&ws) {
                    self.fetched(kind, v);
                }
            }
            Msg::SignIn(sub, state) => self.signin = Some((sub, state)),
            Msg::Onb(kind, v) => self.onb_reply(kind, v),
            Msg::Secrets(ws, names) => {
                if self.model.active_ws.as_deref() == Some(&ws) {
                    self.secret_names = names;
                }
            }
            Msg::Files(query, list) => {
                if self.last_query.as_deref() == Some(&query) {
                    self.files = list;
                }
            }
            Msg::Models(ws, list) => {
                if self.model.active_ws.as_deref() == Some(&ws) {
                    self.models = list;
                }
            }
            Msg::Commands(ws, chat, list) => {
                if self.model.active_ws.as_deref() == Some(&ws) && self.model.open_chat_id.as_deref() == Some(&chat) {
                    self.commands = list;
                }
            }
            Msg::CommandDone(outcome) => match outcome {
                Outcome::Ran(_) => {}
                Outcome::NewChat => self.new_chat(),
                Outcome::Help => {
                    let body = self.commands.iter().map(|c| format!("/{}  {}", c.name, c.description)).collect::<Vec<_>>().join("\n");
                    self.panel = Some(("Commands".into(), body));
                }
                Outcome::Text { title, body } => self.panel = Some((title, body)),
                Outcome::Unsupported(name) if name == "model" || name == "effort" => self.picker_open = true,
                Outcome::Unsupported(name) => self.model.toast = Some(format!("/{name} opens a dialog the native app does not have yet.")),
            },
            Msg::ChatCreated(ws, chat) => {
                if self.model.active_ws.as_deref() == Some(&ws) {
                    let id = chat.id.clone();
                    self.model.snapshot.chats.push(chat);
                    self.view = View::Chat;
                    self.open_chat(id);
                    self.model.needs.snapshot = true;
                }
            }
            Msg::Error(e) => {
                self.in_flight.finish_all();
                self.model.toast = Some(e);
            }
        }
    }

    fn open_provider_form(&mut self, preset: Option<usize>, cx: &mut Context<Self>) {
        let inputs = self.provider_form.get_or_insert_with(|| {
            let ph = ["ID, e.g. ollama", "Name", "Base URL, https://…", "Model id", "Key env name (optional)", "API key (stored in the OS keychain)"];
            std::array::from_fn(|i| {
                let input = cx.new(|cx| if i == 5 { TextInput::new_masked(cx, ph[i]) } else { TextInput::new(cx, ph[i]) });
                cx.observe(&input, |_, _, cx| cx.notify()).detach();
                input
            })
        });
        if let Some((_, id, name, url, env, model)) = preset.map(|i| providers::PRESETS[i]) {
            for (input, v) in inputs.iter().zip([id, name, url, model, env]) {
                input.update(cx, |i, cx| i.set_content(v, cx));
            }
        }
        self.provider_check.clear();
    }

    fn close_provider_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for i in self.provider_form.take().iter().flatten() {
            i.update(cx, |i, cx| i.clear(window, cx));
        }
        self.provider_check.clear();
    }

    fn provider_values(&self, cx: &App) -> Option<providers::CustomForm> {
        let v: Vec<String> = self.provider_form.as_ref()?.iter().map(|i| i.read(cx).content().to_string()).collect();
        Some(providers::CustomForm { id: v[0].clone(), name: v[1].clone(), url: v[2].clone(), model: v[3].clone(), env: v[4].clone(), key: v[5].clone() })
    }

    /// Native file dialog; the chosen files go into the draft as `@` references.
    fn pick_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some("Attach".into()) });
        cx.spawn_in(window, async move |this, cx| {
            let result = picked.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(Ok(Some(paths))) => this.attach_paths(paths.iter().map(|p| p.to_string_lossy().into_owned()).collect(), window, cx),
                Ok(Ok(None)) | Err(_) => {}
                Ok(Err(e)) => {
                    this.model.toast = Some(format!("Could not open the file dialog: {e}"));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Appends `@` references for the files to the draft and focuses the composer.
    fn attach_paths(&mut self, paths: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        if paths.is_empty() || self.view != View::Chat {
            return;
        }
        let refs = files::references(&paths);
        let draft = self.input.read(cx).content().to_string();
        let sep = if draft.is_empty() || draft.ends_with(char::is_whitespace) { "" } else { " " };
        self.input.update(cx, |i, cx| i.set_content(&format!("{draft}{sep}{refs}"), cx));
        window.focus(&self.input.focus_handle(cx), cx);
        cx.notify();
    }

    fn open_chat(&mut self, id: String) {
        self.model.open_chat(id.clone(), Vec::new());
        self.commands.clear();
        self.panel = None;
        self.move_open = false;
        if let (Some(api), Some(ws)) = (self.api.clone(), self.model.active_ws.clone()) {
            load_messages(api.clone(), ws.clone(), id.clone(), self.tx.clone());
            load_permissions(api.clone(), ws.clone(), id.clone(), self.tx.clone());
            load_commands(api, ws, id, self.tx.clone());
        }
    }

    fn answer(&mut self, request: &str, decision: &'static str) {
        let (Some(api), Some(ws), Some(chat)) = (self.api.clone(), self.model.active_ws.clone(), self.model.open_chat_id.clone()) else { return };
        self.model.drop_permission(&chat, request);
        answer_permission(api, ws, chat, request.to_string(), decision, self.tx.clone());
    }

    /// Starts a file search when the text under the caret is a new `@` reference.
    fn on_input_changed(&mut self, cx: &mut Context<Self>) {
        let (text, caret) = {
            let i = self.input.read(cx);
            (i.content().to_string(), i.caret())
        };
        let query = mention_at(&text, caret).map(|m| m.query);
        if query.is_none() {
            self.files.clear();
            self.last_query = None;
            return;
        }
        if !files::needs_search(self.last_query.as_deref(), query.as_deref()) {
            return;
        }
        self.last_query = query.clone();
        if let (Some(api), Some(ws), Some(project)) = (self.api.clone(), self.model.active_ws.clone(), self.open_chat_info().map(|c| c.project.clone())) {
            search_files(api, ws, project, query.unwrap_or_default(), self.tx.clone());
        }
    }

    fn open_chat_info(&self) -> Option<&Chat> {
        let id = self.model.open_chat_id.as_ref()?;
        self.model.snapshot.chats.iter().find(|c| &c.id == id)
    }

    /// Sets the open chat's model (clearing its effort, which may not exist on the new model) or its effort.
    fn choose(&mut self, model: Option<String>, effort: Option<String>) {
        let (Some(api), Some(ws), Some(id)) = (self.api.clone(), self.model.active_ws.clone(), self.model.open_chat_id.clone()) else { return };
        let mut body = serde_json::Map::new();
        if let Some(chat) = self.model.snapshot.chats.iter_mut().find(|c| c.id == id) {
            if let Some(m) = &model {
                chat.model = m.clone();
                chat.effort = None;
                body.insert("model".into(), m.clone().into());
                body.insert("effort".into(), "".into());
            }
            if let Some(e) = &effort {
                chat.effort = Some(e.clone());
                body.insert("effort".into(), e.clone().into());
            }
        }
        patch_chat(api, ws, id, body.into(), self.tx.clone());
    }

    fn patch_ws(&mut self, body: serde_json::Value) {
        if let (Some(api), Some(ws)) = (self.api.clone(), self.model.active_ws.clone()) {
            patch_workspace(api, ws, body, self.tx.clone());
        }
    }

    fn mutate_ws(&mut self, method: &'static str, sub_path: String, body: Option<serde_json::Value>) {
        if let (Some(api), Some(ws)) = (self.api.clone(), self.model.active_ws.clone()) {
            let path = format!("/api/ws/{}{}", seg(&ws), sub_path);
            mutate(api, ws, method, path, body, self.tx.clone());
        }
    }

    /// Like `mutate_ws`, for a path that already carries the workspace (the `routines::*_path` helpers).
    fn mutate_path(&mut self, method: &'static str, path: String, body: Option<serde_json::Value>) {
        if let (Some(api), Some(ws)) = (self.api.clone(), self.model.active_ws.clone()) {
            mutate(api, ws, method, path, body, self.tx.clone());
        }
    }

    fn mutate_computer(&mut self, method: &'static str, path: String, body: Option<serde_json::Value>) {
        if let Some(api) = self.api.clone() {
            mutate_state(api, method, path, body, self.tx.clone());
        }
    }

    fn run_engine(&mut self, action: EngineAction) {
        if let (Some(api), Some(ws)) = (self.api.clone(), self.model.active_ws.clone()) {
            engine_action(api, ws, action, self.tx.clone());
        }
    }

    /// The palette's current matches (cloned, so handlers can use them after the borrow ends).
    fn palette_items(&self, cx: &App) -> Vec<palette::Item> {
        let all = palette::build(&self.model.workspaces, &self.model.snapshot, self.model.snapshot.workspace.kind == "org");
        palette::filter(&all, self.palette_input.read(cx).content()).into_iter().cloned().collect()
    }

    fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette_open = true;
        self.palette_sel = 0;
        self.palette_input.update(cx, |i, cx| i.set_content("", cx));
        window.focus(&self.palette_input.focus_handle(cx), cx);
        cx.notify();
    }

    fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.side.full && !self.palette_open {
            self.side.full = false;
            cx.notify();
        }
        if !self.palette_open && self.onb.nw_kind.is_some() {
            self.onb.nw_kind = None;
            cx.notify();
        }
        if self.palette_open {
            self.palette_open = false;
            window.focus(&self.input.focus_handle(cx), cx);
            cx.notify();
        }
    }

    fn run_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let items = self.palette_items(cx);
        let Some(item) = items.get(self.palette_sel.min(items.len().saturating_sub(1))).cloned() else { return };
        self.close_palette(window, cx);
        match item.kind {
            Kind::Action if item.id == "new-chat" => {
                self.view = View::Chat;
                self.new_chat();
            }
            Kind::Action => self.view = View::Settings,
            Kind::Workspace => {
                if let Some(api) = self.api.clone() {
                    activate(api, item.id, self.tx.clone());
                }
                self.view = View::Chat;
            }
            Kind::Chat => {
                self.view = View::Chat;
                self.open_chat(item.id);
            }
            Kind::Page => {
                if let Some(p) = settings::pages(false).into_iter().find(|p| p.id == item.id) {
                    self.settings_page = p.id;
                    self.open_page(p.id, cx);
                }
                self.view = View::Settings;
            }
        }
        cx.notify();
    }

    fn start_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.open_chat_info().map(|c| c.title.clone()).unwrap_or_default();
        self.rename_input.update(cx, |i, cx| i.set_content(&title, cx));
        self.renaming = true;
        window.focus(&self.rename_input.focus_handle(cx), cx);
    }

    fn rename_to(&mut self, title: &str) {
        let (Some(api), Some(ws), Some(id), Some(body)) = (self.api.clone(), self.model.active_ws.clone(), self.model.open_chat_id.clone(), chats::rename_body(title)) else { return };
        if let (Some(chat), Some(t)) = (self.model.snapshot.chats.iter_mut().find(|c| c.id == id), body["title"].as_str()) {
            chat.title = t.to_string();
        }
        patch_chat(api, ws, id, body, self.tx.clone());
    }

    fn close_chat(&mut self) {
        self.model.open_chat_id = None;
        self.model.messages.clear();
        self.commands.clear();
        self.confirm.cancel();
    }

    fn archive(&mut self) {
        if let (Some(api), Some(ws), Some(id)) = (self.api.clone(), self.model.active_ws.clone(), self.model.open_chat_id.clone()) {
            patch_chat(api, ws, id, chats::archive_body(true), self.tx.clone());
            self.close_chat();
        }
    }

    fn delete(&mut self) {
        let (Some(api), Some(ws), Some(id)) = (self.api.clone(), self.model.active_ws.clone(), self.model.open_chat_id.clone()) else { return };
        if self.confirm.press(&id) {
            delete_chat(api, ws, id, self.tx.clone());
            self.close_chat();
        }
    }

    fn move_to(&mut self, project: &str) {
        self.move_open = false;
        if let (Some(ws), Some(id)) = (self.model.active_ws.clone(), self.model.open_chat_id.clone()) {
            self.mutate_path("POST", chats::move_path(&ws, &id), Some(chats::move_body(project)));
        }
    }

    fn restore_chat(&mut self, id: String) {
        if let (Some(api), Some(ws)) = (self.api.clone(), self.model.active_ws.clone()) {
            patch_chat(api, ws, id, chats::archive_body(false), self.tx.clone());
        }
    }

    fn fork(&mut self) {
        if let (Some(api), Some(ws), Some(id)) = (self.api.clone(), self.model.active_ws.clone(), self.model.open_chat_id.clone()) {
            fork_chat(api, ws, id, self.tx.clone());
        }
    }

    fn toggle_full_access(&mut self) {
        let (Some(api), Some(ws), Some(id)) = (self.api.clone(), self.model.active_ws.clone(), self.model.open_chat_id.clone()) else { return };
        // The chip shows the server's value only; it changes when the fresh snapshot arrives.
        let Some(chat) = self.model.snapshot.chats.iter().find(|c| c.id == id) else { return };
        let on = chat.yolo != Some(true);
        if !self.in_flight.begin(&id) {
            return;
        }
        patch_chat(api, ws, id, chats::full_access_body(on), self.tx.clone());
    }

    fn stop(&mut self) {
        if let (Some(api), Some(ws), Some(chat)) = (self.api.clone(), self.model.active_ws.clone(), self.model.open_chat_id.clone()) {
            stop_chat(api, ws, chat, self.tx.clone());
        }
    }

    fn general_project(&self) -> Option<String> {
        let ps = &self.model.snapshot.projects;
        ps.iter().find(|p| p.is_general == Some(true) && !p.archived).or_else(|| ps.iter().find(|p| !p.archived)).map(|p| p.id.clone())
    }

    fn new_chat(&mut self) {
        if let (Some(api), Some(ws), Some(project)) = (self.api.clone(), self.model.active_ws.clone(), self.general_project()) {
            create_chat(api, ws, project, None, self.tx.clone());
        }
    }

    fn submit(&mut self, input: &Entity<TextInput>, window: &mut Window, cx: &mut Context<Self>) {
        let text = input.read(cx).content().trim().to_string();
        let (Some(api), Some(ws)) = (self.api.clone(), self.model.active_ws.clone()) else { return };
        if text.is_empty() {
            return;
        }
        input.update(cx, |i, cx| i.clear(window, cx));
        if parse_slash(&text).is_some() {
            // The supervisor decides what a slash command means (and refuses ones this agent lacks, with a reason).
            match self.model.open_chat_id.clone() {
                Some(chat) => run_command(api, ws, chat, text, self.tx.clone()),
                None => self.model.toast = Some("Open a chat first to run a command.".into()),
            }
            cx.notify();
            return;
        }
        self.local_id += 1;
        self.model.messages.push(ChatMessage { id: format!("local-{}", self.local_id), role: "user".into(), text: text.clone(), tools: vec![], error: None, created_at: 0, artifact: None });
        match self.model.open_chat_id.clone() {
            Some(chat) => send_text(api, ws, chat, text, self.tx.clone()),
            None => match self.general_project() {
                Some(project) => create_chat(api, ws, project, Some(text), self.tx.clone()),
                None => self.model.toast = Some("This workspace has no project to start a chat in.".into()),
            },
        }
        self.scroll.scroll_to_bottom();
        cx.notify();
    }
}

fn status_line(model: &Model) -> SharedString {
    match &model.status {
        Connection::Starting => "Starting the supervisor…".into(),
        Connection::Connected => "Connected".into(),
        Connection::Disconnected(e) => format!("Disconnected: {e}").into(),
    }
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.web_begin_frame(window);
        if self.onb.loaded && !self.extra.app.onboarded {
            return self.onboarding_view(cx).relative().text_color(rgb(TEXT)).children(self.new_workspace_dialog(cx));
        }
        let m = &self.model;
        let rail = div().flex().flex_col().gap_2().p_2().w(px(56.)).h_full().bg(rgb(RAIL)).children(m.workspaces.iter().map(|w| {
            let active = m.active_ws.as_deref() == Some(&w.id);
            // Org workspaces are purple, team ones cyan, personal ones the accent; the active one has a ring.
            let tint = match w.kind.as_str() {
                "org" => 0x9d7cd8,
                "team" => 0x56b6c2,
                _ => ACCENT,
            };
            let id = w.id.clone();
            div()
                .id(SharedString::from(format!("ws-{}", w.id)))
                .flex()
                .items_center()
                .justify_center()
                .size(px(40.))
                .rounded_lg()
                .cursor_pointer()
                .bg(rgb(PANEL))
                .border_2()
                .border_color(rgb(if active { tint } else { PANEL }))
                .text_color(rgb(tint))
                .font_weight(FontWeight::BOLD)
                .child(SharedString::from(w.initials.clone()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(api) = this.api.clone() {
                        activate(api, id.clone(), this.tx.clone());
                    }
                    cx.notify();
                }))
        }));

        let sidebar = div()
            .flex()
            .flex_col()
            .w(px(270.))
            .h_full()
            .bg(rgb(PANEL))
            .child(
                div()
                    .id("new-chat")
                    .m_2()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(rgb(ACCENT))
                    .text_color(rgb(0xffffff))
                    .child("＋ New chat")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.new_chat();
                        cx.notify();
                    })),
            )
            .child(div().id("sidebar-scroll").flex_1().min_h_0().overflow_y_scroll().px_2().children(m.sidebar().into_iter().map(|row| {
                let open = m.open_chat_id.as_deref() == Some(&row.id) && row.kind == RowKind::Chat;
                let id = row.id.clone();
                let indent = px(10. * row.depth as f32);
                let base = div().id(SharedString::from(format!("row-{}", row.id))).flex().items_center().gap_2().pl(indent).py_1().px_2().rounded_md().text_sm();
                match row.kind {
                    RowKind::Project => base.text_color(rgb(MUTED)).font_weight(FontWeight::BOLD).cursor_pointer().hover(|s| s.bg(rgb(USER_BUBBLE))).child(SharedString::from(row.title)).on_click(cx.listener(move |this, _, _, cx| {
                        this.open_project(id.clone());
                        cx.notify();
                    })),
                    RowKind::Chat => base
                        .cursor_pointer()
                        .bg(rgb(if open { USER_BUBBLE } else { PANEL }))
                        .text_color(rgb(TEXT))
                        .hover(|s| s.bg(rgb(USER_BUBBLE)))
                        .child(SharedString::from(if row.title.is_empty() { "New chat".to_string() } else { row.title }))
                        .when(row.working, |d| d.child(div().text_color(rgb(ACCENT)).child("●")))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_chat(id.clone());
                            cx.notify();
                        })),
                }
            })))
            .child({
                let archived = m.archived_chats();
                div()
                    .flex()
                    .flex_col()
                    .px_2()
                    .when(!archived.is_empty(), |d| {
                        d.child(
                            div()
                                .id("toggle-archived-chats")
                                .px_2()
                                .py_1()
                                .cursor_pointer()
                                .text_sm()
                                .text_color(rgb(MUTED))
                                .child(SharedString::from(format!("{} archived chats ({})", if self.show_archived_chats { "Hide" } else { "Show" }, archived.len())))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.show_archived_chats = !this.show_archived_chats;
                                    cx.notify();
                                })),
                        )
                    })
                    .when(self.show_archived_chats, |d| {
                        d.max_h(px(180.)).children(archived.into_iter().map(|(id, title)| {
                            div()
                                .id(SharedString::from(format!("arch-{id}")))
                                .flex()
                                .justify_between()
                                .px_2()
                                .py_1()
                                .text_sm()
                                .text_color(rgb(MUTED))
                                .child(SharedString::from(if title.is_empty() { "New chat".to_string() } else { title }))
                                .child(div().id(SharedString::from(format!("restore-{id}"))).cursor_pointer().text_color(rgb(ACCENT)).child("Restore").on_click(cx.listener(move |this, _, _, cx| {
                                    this.restore_chat(id.clone());
                                    cx.notify();
                                })))
                        }))
                    })
            });

        let chat_title = m
            .open_chat_id
            .as_ref()
            .and_then(|id| m.snapshot.chats.iter().find(|c| &c.id == id))
            .map(|c| if c.title.is_empty() { "New chat".to_string() } else { c.title.clone() })
            .unwrap_or_else(|| "WorkspaceAI".to_string());

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(rgb(PANEL))
            .child(div().font_weight(FontWeight::BOLD).child(SharedString::from(chat_title)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .when(m.open_chat_working(), |d| {
                        d.child(
                            div()
                                .id("stop")
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .cursor_pointer()
                                .bg(rgb(0x4a2b2b))
                                .text_sm()
                                .child("■ Stop")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.stop();
                                    cx.notify();
                                })),
                        )
                    })
                    .children(self.open_chat_info().map(|c| {
                        let label = self.models.label(&c.agent, &c.model, c.effort.as_deref());
                        div()
                            .id("model-chip")
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .bg(rgb(PANEL))
                            .text_sm()
                            .child(SharedString::from(format!("{label} ▾")))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.picker_open = !this.picker_open;
                                cx.notify();
                            }))
                    }))
                    .child(div().text_sm().text_color(rgb(MUTED)).child(status_line(m))),
            );

        let actions = self.open_chat_info().cloned().map(|c| {
            let id = c.id.clone();
            let armed = self.confirm.is_armed(&id);
            let full = c.yolo == Some(true);
            let button = |key: &'static str, label: String, bg: u32| div().id(key).px_2().py_1().rounded_md().cursor_pointer().bg(rgb(bg)).text_sm().child(SharedString::from(label));
            div()
                .flex()
                .items_center()
                .gap_2()
                .px_4()
                .py_1()
                .child(button("act-rename", "Rename".into(), PANEL).on_click(cx.listener(|this, _, window, cx| {
                    this.start_rename(window, cx);
                    cx.notify();
                })))
                .child(button("act-fork", "Fork".into(), PANEL).on_click(cx.listener(|this, _, _, cx| {
                    this.fork();
                    cx.notify();
                })))
                .child(button("act-archive", "Archive".into(), PANEL).on_click(cx.listener(|this, _, _, cx| {
                    this.archive();
                    cx.notify();
                })))
                .child(button("act-move", "Move…".into(), if self.move_open { USER_BUBBLE } else { PANEL }).on_click(cx.listener(|this, _, _, cx| {
                    this.move_open = !this.move_open;
                    cx.notify();
                })))
                .child(button("act-delete", if armed { "Click again to delete".into() } else { "Delete".into() }, if armed { 0x7a2b2b } else { PANEL }).on_click(cx.listener(|this, _, _, cx| {
                    this.delete();
                    cx.notify();
                })))
                .child(button("act-browser", "Browser".into(), if self.side.browser.is_some() && !self.side.art_open { USER_BUBBLE } else { PANEL }).on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_browser();
                    cx.notify();
                })))
                .child(button("act-full", if self.in_flight.is_pending(&c.id) { "Full access: saving…".into() } else if full { "Full access: on".into() } else { "Full access: off".into() }, if full { 0x7a4a1b } else { PANEL }).on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_full_access();
                    cx.notify();
                })))
        });
        let move_panel = self.move_open.then(|| self.open_chat_info().cloned()).flatten().map(|c| {
            div().id("move-list").mx_4().mt_2().p_2().max_h(px(220.)).overflow_y_scroll().rounded_md().bg(rgb(PANEL)).flex().flex_col().child(div().text_sm().text_color(rgb(MUTED)).child("Move to project")).children(
                projects::tree(&self.model.snapshot.projects, false).into_iter().filter(|n| n.id != c.project).map(|n| {
                    let pid = n.id.clone();
                    div().id(SharedString::from(format!("move-{}", n.id))).pl(px(8. + 14. * n.depth as f32)).py_1().rounded_md().cursor_pointer().hover(|s| s.bg(rgb(USER_BUBBLE))).text_sm().child(SharedString::from(format!("▤ {}", n.name))).on_click(cx.listener(move |this, _, _, cx| {
                        this.move_to(&pid);
                        cx.notify();
                    }))
                }),
            )
        });
        let rename_row = self.renaming.then(|| div().mx_4().mt_2().px_3().py_2().rounded_md().bg(rgb(PANEL)).child(self.rename_input.clone()));

        let picker = self.picker_open.then(|| self.open_chat_info().cloned()).flatten().map(|c| {
            let options = self.models.options_for(&c.agent);
            let efforts = self.models.efforts_for(&c.agent, &c.model);
            let row = |id: String, label: String, on: bool| div().id(SharedString::from(id)).px_2().py_1().rounded_md().cursor_pointer().bg(rgb(if on { USER_BUBBLE } else { PANEL })).hover(|s| s.bg(rgb(USER_BUBBLE))).text_sm().child(SharedString::from(label));
            div()
                .mx_4()
                .mt_2()
                .p_2()
                .rounded_md()
                .bg(rgb(PANEL))
                .flex()
                .flex_col()
                .gap_2()
                .child(div().text_sm().text_color(rgb(MUTED)).child(if options.is_empty() { "Models load once the engine is running…" } else { "Model" }))
                .child(div().id("model-list").max_h(px(220.)).overflow_y_scroll().flex().flex_col().children(options.into_iter().map(|o| {
                    let on = o.id == c.model;
                    let oid = o.id.clone();
                    row(format!("model-{}", o.id), if o.name.is_empty() { o.id.clone() } else { o.name.clone() }, on).on_click(cx.listener(move |this, _, _, cx| {
                        this.choose(Some(oid.clone()), None);
                        this.picker_open = false;
                        cx.notify();
                    }))
                })))
                .when(!efforts.is_empty(), |d| {
                    d.child(div().text_sm().text_color(rgb(MUTED)).child("Effort")).child(div().flex().gap_2().children(efforts.into_iter().map(|e| {
                        let on = c.effort.as_deref() == Some(e.as_str());
                        let ev = e.clone();
                        row(format!("effort-{e}"), e, on).on_click(cx.listener(move |this, _, _, cx| {
                            this.choose(None, Some(ev.clone()));
                            this.picker_open = false;
                            cx.notify();
                        }))
                    })))
                })
        });

        let messages = div().id("messages").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.scroll).p_4().flex().flex_col().gap_3().children(
            m.messages.iter().map(|msg| {
                let user = msg.role == "user";
                let shown = if msg.text.is_empty() && msg.tools.is_empty() && msg.error.is_none() { "…".to_string() } else { msg.text.clone() };
                let mut bubble = div().max_w(px(760.)).px_3().py_2().rounded_lg().bg(rgb(if user { USER_BUBBLE } else { BG }));
                bubble = if user { bubble.child(SharedString::from(shown)) } else { bubble.child(md_view::blocks(&markdown::parse(&shown))) };
                for t in &msg.tools {
                    bubble = bubble.child(div().text_sm().text_color(rgb(MUTED)).child(SharedString::from(format!("⚙ {} · {}{}", t.name, t.status, t.detail.as_deref().map(|d| format!(" · {d}")).unwrap_or_default()))));
                }
                if let Some(e) = &msg.error {
                    bubble = bubble.child(div().text_sm().text_color(rgb(0xff6b6b)).child(SharedString::from(e.clone())));
                }
                let text = msg.text.clone();
                let copy = div().id(SharedString::from(format!("copy-{}", msg.id))).px_2().rounded_md().cursor_pointer().bg(rgb(PANEL)).text_size(px(11.)).text_color(rgb(MUTED)).child("Copy").on_click(cx.listener(move |this, _, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
                    this.model.toast = Some("Copied to clipboard".into());
                    cx.notify();
                }));
                let bubble = bubble.child(copy).children(self.message_extras(msg, cx));
                if user {
                    div().flex().justify_end().child(bubble)
                } else {
                    div().flex().child(bubble)
                }
            }),
        )
        .children(m.pending_permissions().iter().map(|p| {
            let (a, b, c) = (p.id.clone(), p.id.clone(), p.id.clone());
            let button = |id: String, label: &'static str, bg: u32| div().id(SharedString::from(format!("perm-{id}-{label}"))).px_3().py_1().rounded_md().cursor_pointer().bg(rgb(bg)).text_sm().child(label);
            div()
                .flex()
                .flex_col()
                .gap_2()
                .max_w(px(760.))
                .px_3()
                .py_2()
                .rounded_lg()
                .border_1()
                .border_color(rgb(ACCENT))
                .child(div().font_weight(FontWeight::BOLD).child(SharedString::from(format!("Permission needed: {}", p.title))))
                .when(!p.detail.is_empty(), |d| d.child(div().font_family("monospace").text_size(px(13.)).text_color(rgb(MUTED)).child(SharedString::from(p.detail.clone()))))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(button(a.clone(), "Allow once", ACCENT).on_click(cx.listener(move |this, _, _, cx| {
                            this.answer(&a, "once");
                            cx.notify();
                        })))
                        .child(button(b.clone(), "Always allow", USER_BUBBLE).on_click(cx.listener(move |this, _, _, cx| {
                            this.answer(&b, "always");
                            cx.notify();
                        })))
                        .child(button(c.clone(), "Reject", 0x4a2b2b).on_click(cx.listener(move |this, _, _, cx| {
                            this.answer(&c, "reject");
                            cx.notify();
                        }))),
                )
        }));

        let toast = m.toast.clone().map(|t| {
            div()
                .id("toast")
                .mx_4()
                .px_3()
                .py_2()
                .rounded_md()
                .cursor_pointer()
                .bg(rgb(0x4a2b2b))
                .text_sm()
                .child(SharedString::from(format!("{t}  (click to dismiss)")))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.model.toast = None;
                    cx.notify();
                }))
        });

        let typed = self.input.read(cx).content().to_string();
        let suggested: Vec<ChatCommand> = command::suggestions(&typed, &self.commands).into_iter().cloned().collect();
        let suggest_box = (!suggested.is_empty()).then(|| {
            div().mx_3().mt_2().p_1().rounded_md().bg(rgb(PANEL)).flex().flex_col().children(suggested.into_iter().map(|c| {
                let name = c.name.clone();
                div()
                    .id(SharedString::from(format!("sg-{}", c.name)))
                    .flex()
                    .gap_3()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(USER_BUBBLE)))
                    .child(div().font_weight(FontWeight::BOLD).child(SharedString::from(format!("/{}", c.name))))
                    .child(div().text_color(rgb(MUTED)).text_sm().child(SharedString::from(c.hint.clone().unwrap_or_default())))
                    .child(div().text_color(rgb(MUTED)).text_sm().child(SharedString::from(c.description.clone())))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let text = format!("/{name} ");
                        this.input.update(cx, |i, cx| i.set_content(&text, cx));
                        window.focus(&this.input.focus_handle(cx), cx);
                    }))
            }))
        });
        let caret = self.input.read(cx).caret();
        let mention = mention_at(&typed, caret);
        let file_box = mention.clone().filter(|_| !self.files.is_empty()).map(|men| {
            div().mx_3().mt_2().p_1().rounded_md().bg(rgb(PANEL)).max_h(px(220.)).id("file-list").overflow_y_scroll().flex().flex_col().children(self.files.iter().map(|path| {
                let path = path.clone();
                let (typed, start) = (typed.clone(), men.start);
                div()
                    .id(SharedString::from(format!("file-{path}")))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .text_sm()
                    .font_family("monospace")
                    .hover(|s| s.bg(rgb(USER_BUBBLE)))
                    .child(SharedString::from(path.clone()))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let (text, new_caret) = insert_mention(&typed, start, caret, &path, false);
                        this.input.update(cx, |i, cx| i.set_content_at(&text, new_caret, cx));
                        this.files.clear();
                        this.last_query = None;
                        window.focus(&this.input.focus_handle(cx), cx);
                    }))
            }))
        });
        let panel = self.panel.clone().map(|(title, body)| {
            div()
                .id("panel")
                .mx_3()
                .mt_2()
                .p_3()
                .rounded_md()
                .bg(rgb(PANEL))
                .cursor_pointer()
                .child(div().font_weight(FontWeight::BOLD).child(SharedString::from(format!("{title}  (click to close)"))))
                .child(div().font_family("monospace").text_size(px(13.)).child(SharedString::from(body)))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.panel = None;
                    cx.notify();
                }))
        });

        let attach = ui::button("attach", "Attach", USER_BUBBLE).on_click(cx.listener(|this, _, window, cx| this.pick_files(window, cx)));
        let composer = div()
            .id("composer")
            .p_3()
            .flex()
            .items_start()
            .gap_2()
            .drag_over::<ExternalPaths>(|s, _, _, _| s.bg(rgba(0xd9775722)))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.attach_paths(paths.paths().iter().map(|p| p.to_string_lossy().into_owned()).collect(), window, cx);
            }))
            .child(div().flex_1().min_w_0().px_3().py_2().rounded_lg().bg(rgb(PANEL)).text_size(px(15.)).line_height(px(22.)).child(self.input.clone()))
            .child(attach);

        let main = div().flex().flex_col().flex_1().min_w_0().h_full().child(header).children(actions).children(move_panel).children(rename_row).children(picker).child(messages).children(toast).children(panel).children(suggest_box).children(file_box).child(composer);

        // ---- settings ----
        let is_org = m.snapshot.workspace.kind == "org";
        let pages = settings::pages(is_org);
        let cur = pages.iter().find(|p| p.id == self.settings_page).cloned().unwrap_or_else(|| pages[0].clone());
        let settings_nav = div().flex().flex_col().w(px(240.)).h_full().bg(rgb(PANEL)).p_2().gap_1().children(pages.iter().map(|p| {
            let id = p.id;
            let on = p.id == cur.id;
            div()
                .id(SharedString::from(format!("page-{}", p.id)))
                .px_3()
                .py_2()
                .rounded_md()
                .cursor_pointer()
                .bg(rgb(if on { USER_BUBBLE } else { PANEL }))
                .hover(|s| s.bg(rgb(USER_BUBBLE)))
                .flex()
                .justify_between()
                .child(SharedString::from(p.title))
                .when(p.locked, |d| d.child(div().text_color(rgb(MUTED)).text_sm().child("locked")))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.settings_page = id;
                    this.open_page(id, cx);
                    if id == "general" {
                        let name = this.model.snapshot.workspace.name.clone();
                        this.name_input.update(cx, |i, cx| i.set_content(&name, cx));
                    }
                    if id == "agents" {
                        if let (Some(api), Some(ws)) = (this.api.clone(), this.model.active_ws.clone()) {
                            load_subscriptions(api, ws, this.tx.clone());
                        }
                    }
                    if id == "providers" {
                        if let (Some(api), Some(ws)) = (this.api.clone(), this.model.active_ws.clone()) {
                            load_secrets(api, ws, this.tx.clone());
                        }
                    }
                    cx.notify();
                }))
        }));
        let w = &m.snapshot.workspace;
        let switch_row = |field: &'static str, label: &str, note: &str, on: bool, locked: bool| {
            let sw = ui::switch(format!("sw-{field}"), on, locked);
            let sw = if locked {
                sw
            } else {
                sw.on_click(cx.listener(move |this, _, _, cx| {
                    this.patch_ws(settings::patch(field, serde_json::json!(!on)));
                    cx.notify();
                }))
            };
            ui::row(label, note, sw)
        };
        let mut page = div().flex().flex_col().gap_3().p(px(28.)).max_w(px(840.)).child(ui::page_title(cur.title, ""));
        if cur.locked {
            page = page.child(div().p_3().rounded_lg().border_1().border_color(rgb(ACCENT)).text_sm().child("Set by your organization's policy. You can read it here but not change it."));
        }
        page = match cur.id {
            "general" => page
                .child(ui::row("Workspace name", "Press Enter to save.", div().w(px(260.)).px_2().py_1().rounded_md().bg(rgb(BG)).child(self.name_input.clone())))
                .child(self.accent_row(cx))
                .child(self.default_pickers(cx))
                .child(self.instructions_card(cx))
                .child(self.remove_row(cx)),
            "memory" => page
                .child(switch_row("useMemory", "Use memory", "Agents are told to use this workspace's memory, and each session opens with what it holds.", settings::switch_on(w.use_memory), cur.locked))
                .child(switch_row("memoryRecall", "Recall on every message", "Look each message up in memory and hand the agent what matched.", settings::switch_on(w.memory_recall), cur.locked))
                .child(switch_row("suggestMemories", "Suggest memories", "Offer to remember things from chats.", w.suggest_memories == Some(true), cur.locked))
                .child(div().text_size(px(18.)).font_weight(FontWeight::BOLD).mt_3().child("Facts"))
                .child(ui::row("Add a fact", "Press Enter. Agents in this workspace can use it.", div().w(px(320.)).px_2().py_1().rounded_md().bg(rgb(BG)).child(self.fact_input.clone())))
                .children({
                    let button = |id: String, label: &'static str, bg: u32| ui::button(id, label, bg);
                    let mut rows: Vec<gpui::AnyElement> = Vec::new();
                    for g in facts::group(&m.snapshot.facts, &m.snapshot.projects) {
                        rows.push(div().mt_2().text_color(rgb(MUTED)).font_weight(FontWeight::BOLD).child(SharedString::from(g.label.clone())).into_any_element());
                        for (f, suggested) in g.suggested.iter().map(|f| (*f, true)).chain(g.approved.iter().map(|f| (*f, false))) {
                            let (fid_del, fid_ok) = (f.id.clone(), f.id.clone());
                            let mut controls = div().flex().gap_2();
                            if suggested {
                                controls = controls.child(button(format!("fact-ok-{}", f.id), "Approve", ACCENT).on_click(cx.listener(move |this, _, _, cx| {
                                    this.mutate_ws("PATCH", format!("/facts/{}", seg(&fid_ok)), Some(facts::approve_body()));
                                    cx.notify();
                                })));
                            }
                            controls = controls.child(button(format!("fact-del-{}", f.id), "Delete", ui::DANGER).on_click(cx.listener(move |this, _, _, cx| {
                                this.mutate_ws("DELETE", format!("/facts/{}", seg(&fid_del)), None);
                                cx.notify();
                            })));
                            rows.push(ui::row(&f.text, if suggested { "Suggested — approve to keep it" } else { "" }, controls).into_any_element());
                        }
                    }
                    if !m.snapshot.facts.is_empty() {
                        let armed = self.confirm.is_armed("clear-facts");
                        rows.push(
                            button("fact-clear".into(), if armed { "Click again to delete every fact" } else { "Delete all facts" }, if armed { 0x7a2b2b } else { ui::DANGER })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if this.confirm.press("clear-facts") {
                                        this.mutate_ws("DELETE", "/facts".into(), None);
                                    }
                                    cx.notify();
                                }))
                                .into_any_element(),
                        );
                    }
                    rows
                }),
            "engine" => {
                let e = &m.snapshot.engine;
                let mut p = page
                    .child(ui::row("State", "Every chat is an opencode session.", div().font_weight(FontWeight::BOLD).child(SharedString::from(if e.state.is_empty() { "unknown".to_string() } else { e.state.clone() }))))
                    .child(ui::row("Version", "", div().text_color(rgb(MUTED)).child(SharedString::from(if e.version.is_empty() { "—".to_string() } else { e.version.clone() }))))
                    .child(ui::row("Address", "", div().font_family("monospace").text_color(rgb(MUTED)).child(SharedString::from(if e.url.is_empty() { "—".to_string() } else { e.url.clone() }))));
                if let Some(err) = &e.error {
                    p = p.child(div().p_3().rounded_lg().bg(rgb(ui::DANGER)).text_sm().child(SharedString::from(err.clone())));
                }
                for sc in &e.sidecars {
                    p = p.child(ui::row(&sc.name, "Helper process", div().text_color(rgb(MUTED)).child(SharedString::from(sc.state.clone()))));
                }
                let p = p.child(div().flex().gap_2().children(engine_actions(&e.state).into_iter().map(|a| {
                    ui::button(format!("engine-{}", a.label()), a.label(), if a == EngineAction::Stop { ui::DANGER } else { ACCENT }).on_click(cx.listener(move |this, _, _, cx| {
                        this.run_engine(a);
                        cx.notify();
                    }))
                })));
                self.engine_more(p, cur.locked, cx)
            }
            "agents" => {
                let ws_id = m.active_ws.clone().unwrap_or_default();
                let locked = cur.locked;
                let mut p = page;
                for sub in &self.subs {
                    let mut controls = div().flex().gap_2();
                    if !locked {
                        for a in subscriptions::actions(sub) {
                            let (ws_a, id_a) = (ws_id.clone(), sub.id.clone());
                            controls = controls.child(ui::button(format!("sub-{}-{}", sub.id, a.label()), a.label(), if a == SubAction::Disconnect { USER_BUBBLE } else { ACCENT }).on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(api) = this.api.clone() {
                                    change_subscription(api, ws_a.clone(), subscriptions::action_path(&ws_a, &id_a, a), this.tx.clone());
                                }
                                cx.notify();
                            })));
                        }
                    }
                    if !locked && subscriptions::can_sign_in(sub) && (!sub.signed_in || subscriptions::needs_sign_in(sub)) {
                        let (ws_s, id_s) = (ws_id.clone(), sub.id.clone());
                        controls = controls.child(ui::button(format!("sub-{}-signin", sub.id), "Sign in", ACCENT).on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(api) = this.api.clone() {
                                this.signin = Some((id_s.clone(), SignIn { state: "waiting".into(), ..SignIn::default() }));
                                start_signin(api, ws_s.clone(), id_s.clone(), this.tx.clone());
                            }
                            cx.notify();
                        })));
                    }
                    let mut note = subscriptions::summary(sub);
                    if subscriptions::needs_sign_in(sub) {
                        note.push_str("\nThe login expired. Sign in again.");
                    }
                    p = p.child(ui::row(if sub.name.is_empty() { &sub.id } else { &sub.name }, &note, controls));
                    if let Some((sid, st)) = self.signin.clone().filter(|(sid, st)| *sid == sub.id && st.state != "idle") {
                        let mut card = div().flex().flex_col().gap_2().p_3().ml_4().rounded_lg().bg(rgb(PANEL)).child(div().child(SharedString::from(subscriptions::signin_text(&st))));
                        if st.waiting() {
                            if let Some(url) = st.url.clone() {
                                card = card.child(div().text_sm().text_color(rgb(MUTED)).child(SharedString::from(format!("No browser window? Open {url}"))));
                            }
                            let (ws_c, id_c, ws_x, id_x) = (ws_id.clone(), sid.clone(), ws_id.clone(), sid.clone());
                            card = card.child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(div().flex_1().px_2().py_1().rounded_md().bg(rgb(BG)).child(self.signin_code.clone()))
                                    .child(ui::button("signin-send", "Send code", ACCENT).on_click(cx.listener(move |this, _, window, cx| {
                                        let typed = this.signin_code.read(cx).content().to_string();
                                        if let (Some(body), Some(api)) = (subscriptions::code_body(&typed), this.api.clone()) {
                                            signin_call(api, ws_c.clone(), id_c.clone(), subscriptions::signin_code_path(&ws_c, &id_c), Some(body), this.tx.clone());
                                            this.signin_code.update(cx, |i, cx| i.clear(window, cx));
                                        }
                                        cx.notify();
                                    })))
                                    .child(ui::button("signin-cancel", "Cancel", USER_BUBBLE).on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(api) = this.api.clone() {
                                            signin_call(api, ws_x.clone(), id_x.clone(), subscriptions::signin_cancel_path(&ws_x, &id_x), None, this.tx.clone());
                                        }
                                        cx.notify();
                                    }))),
                            );
                        }
                        p = p.child(card);
                    }
                }
                if self.subs.is_empty() {
                    p = p.child(div().text_color(rgb(MUTED)).child("Looking for installed agents…"));
                }
                p
            }
            "connectors" => {
                let ws_id = m.active_ws.clone().unwrap_or_default();
                let locked = cur.locked;
                let mut p = page;
                if !locked {
                    let (name, target) = (self.mcp_name.read(cx).content().to_string(), self.mcp_target.read(cx).content().to_string());
                    let body = connectors::add_body(&name, &target);
                    let ws_add = ws_id.clone();
                    p = p.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .rounded_lg()
                            .bg(rgb(PANEL))
                            .child(div().font_weight(FontWeight::BOLD).child("Add a connector"))
                            .child(div().text_sm().text_color(rgb(MUTED)).child("A web address connects a hosted MCP server. Anything else runs as a command on this computer."))
                            .child(div().px_2().py_1().rounded_md().bg(rgb(BG)).child(self.mcp_name.clone()))
                            .child(div().px_2().py_1().rounded_md().bg(rgb(BG)).child(self.mcp_target.clone()))
                            .child(ui::button("mcp-add", "Add", if body.is_some() { ACCENT } else { USER_BUBBLE }).on_click(cx.listener(move |this, _, window, cx| {
                                if let Some(body) = body.clone() {
                                    this.mutate_path("POST", connectors::add_path(&ws_add), Some(body));
                                    for i in [&this.mcp_name, &this.mcp_target] {
                                        i.update(cx, |i, cx| i.clear(window, cx));
                                    }
                                }
                                cx.notify();
                            }))),
                    );
                }
                for c in &m.snapshot.workspace.mcp {
                    let mut controls = div().flex().items_center().gap_3();
                    if connectors::can_toggle(c) && !locked {
                        let (ws_t, id_t, on) = (ws_id.clone(), c.id.clone(), c.on);
                        controls = controls.child(ui::switch(format!("mcp-sw-{}", c.id), c.on, false).on_click(cx.listener(move |this, _, _, cx| {
                            this.mutate_path("PATCH", connectors::item_path(&ws_t, &id_t), Some(connectors::toggle_body(!on)));
                            cx.notify();
                        })));
                    } else {
                        controls = controls.child(ui::switch(format!("mcp-sw-{}", c.id), c.on, true));
                    }
                    if connectors::can_remove(c) && !locked {
                        let (ws_d, id_d) = (ws_id.clone(), c.id.clone());
                        let armed = self.confirm.is_armed(&format!("m:{}", c.id));
                        controls = controls.child(ui::button(format!("mcp-del-{}", c.id), if armed { "Click again to remove" } else { "Remove" }, if armed { 0x7a2b2b } else { ui::DANGER }).on_click(cx.listener(move |this, _, _, cx| {
                            if this.confirm.press(&format!("m:{id_d}")) {
                                this.mutate_path("DELETE", connectors::item_path(&ws_d, &id_d), None);
                            }
                            cx.notify();
                        })));
                    }
                    let note = format!("{}{}", connectors::describe(c), if c.locked == Some(true) { " · required by your organization" } else { "" });
                    p = p.child(ui::row(if c.name.is_empty() { &c.id } else { &c.name }, &note, controls));
                }
                if m.snapshot.workspace.mcp.is_empty() {
                    p = p.child(div().text_color(rgb(MUTED)).child("No connectors yet."));
                }
                p
            }
            "providers" => {
                let ws_id = m.active_ws.clone().unwrap_or_default();
                let locked = cur.locked;
                let mut p = page.child(ui::row(
                    "API key",
                    "Paste a key here, then press “Save key” on a provider. It goes straight to your system keychain and is never shown again.",
                    div().w(px(300.)).px_2().py_1().rounded_md().bg(rgb(BG)).child(self.secret_input.clone()),
                ));
                if !locked {
                    p = p.child(match self.provider_form.clone() {
                        None => div().child(ui::button("pv-custom", "Add a custom provider", ACCENT).on_click(cx.listener(|this, _, _, cx| {
                            this.open_provider_form(None, cx);
                            cx.notify();
                        }))),
                        Some(inputs) => {
                            let form = self.provider_values(cx).unwrap_or_default();
                            let (ws_a, ws_t) = (ws_id.clone(), ws_id.clone());
                            let (add_body, test_body) = (providers::custom_body(&form), providers::test_body(&form));
                            let mut card = div().flex().flex_col().gap_2().p_3().rounded_lg().bg(rgb(PANEL)).child(div().font_weight(FontWeight::BOLD).child("Custom provider"));
                            let mut presets = div().flex().gap_2();
                            for (i, pre) in providers::PRESETS.iter().enumerate() {
                                presets = presets.child(ui::button(format!("pv-preset-{i}"), pre.0, USER_BUBBLE).on_click(cx.listener(move |this, _, _, cx| {
                                    this.open_provider_form(Some(i), cx);
                                    cx.notify();
                                })));
                            }
                            card = card.child(presets);
                            for input in inputs.iter() {
                                card = card.child(div().px_2().py_1().rounded_md().bg(rgb(BG)).child(input.clone()));
                            }
                            if let Err(why) = &add_body {
                                card = card.child(div().text_sm().text_color(rgb(MUTED)).child(*why));
                            }
                            if !self.provider_check.is_empty() {
                                card = card.child(div().text_sm().child(SharedString::from(self.provider_check.clone())));
                            }
                            div().child(
                                card.child(
                                    div()
                                        .flex()
                                        .gap_2()
                                        .child(ui::button("pv-test", "Test connection", USER_BUBBLE).on_click(cx.listener(move |this, _, _, cx| {
                                            match (&test_body, this.api.clone()) {
                                                (Ok(body), Some(api)) => {
                                                    this.provider_check = "Testing…".into();
                                                    test_provider(api, ws_t.clone(), body.clone(), this.tx.clone());
                                                }
                                                (Err(why), _) => this.provider_check = (*why).into(),
                                                _ => {}
                                            }
                                            cx.notify();
                                        })))
                                        .child(ui::button("pv-add", "Add", if add_body.is_ok() { ACCENT } else { USER_BUBBLE }).on_click(cx.listener(move |this, _, window, cx| {
                                            if let Ok(body) = &add_body {
                                                this.mutate_path("POST", providers::providers_path(&ws_a), Some(body.clone()));
                                                this.close_provider_form(window, cx);
                                            }
                                            cx.notify();
                                        })))
                                        .child(ui::button("pv-cancel", "Cancel", USER_BUBBLE).on_click(cx.listener(|this, _, window, cx| {
                                            this.close_provider_form(window, cx);
                                            cx.notify();
                                        }))),
                                ),
                            )
                        }
                    });
                }
                for pr in &m.snapshot.workspace.providers {
                    let state = providers::key_state(pr, &self.secret_names);
                    let env = pr.key_env.clone().unwrap_or_default();
                    let mut controls = div().flex().gap_2();
                    if !locked && state != KeyState::NotNeeded {
                        let (env_save, ws_save) = (env.clone(), ws_id.clone());
                        controls = controls.child(ui::button(format!("pv-save-{}", pr.id), "Save key", ACCENT).on_click(cx.listener(move |this, _, window, cx| {
                            let typed = this.secret_input.read(cx).content().to_string();
                            if let (Some(body), Some(name), Some(api)) = (providers::secret_body(&typed), providers::secret_name(&env_save), this.api.clone()) {
                                change_secret(api, ws_save.clone(), "PUT", providers::secret_path(&ws_save, &name), Some(body), this.tx.clone());
                                this.secret_input.update(cx, |i, cx| i.clear(window, cx));
                            } else {
                                this.model.toast = Some("Paste a key first.".into());
                            }
                            cx.notify();
                        })));
                        if state == KeyState::Stored {
                            let (env_rm, ws_rm) = (env.clone(), ws_id.clone());
                            controls = controls.child(ui::button(format!("pv-rmkey-{}", pr.id), "Remove key", USER_BUBBLE).on_click(cx.listener(move |this, _, _, cx| {
                                if let (Some(name), Some(api)) = (providers::secret_name(&env_rm), this.api.clone()) {
                                    change_secret(api, ws_rm.clone(), "DELETE", providers::secret_path(&ws_rm, &name), None, this.tx.clone());
                                }
                                cx.notify();
                            })));
                        }
                    }
                    if !locked && pr.base_url.is_some() {
                        let prov = pr.clone();
                        controls = controls.child(ui::button(format!("pv-edit-{}", pr.id), "Edit", USER_BUBBLE).on_click(cx.listener(move |this, _, _, cx| {
                            this.start_edit_provider(&prov, cx);
                            cx.notify();
                        })));
                    }
                    if !locked {
                        let (id_del, ws_del) = (pr.id.clone(), ws_id.clone());
                        let armed = self.confirm.is_armed(&format!("pv:{}", pr.id));
                        controls = controls.child(ui::button(format!("pv-del-{}", pr.id), if armed { "Click again to remove" } else { "Remove provider" }, if armed { 0x7a2b2b } else { ui::DANGER }).on_click(cx.listener(move |this, _, _, cx| {
                            if this.confirm.press(&format!("pv:{id_del}")) {
                                this.mutate_path("DELETE", providers::provider_path(&ws_del, &id_del), None);
                            }
                            cx.notify();
                        })));
                    }
                    let note = format!("{} · {} model{} · {}", if pr.note.is_empty() { "provider" } else { &pr.note }, pr.models.len(), if pr.models.len() == 1 { "" } else { "s" }, state.text());
                    p = p.child(ui::row(if pr.name.is_empty() { &pr.id } else { &pr.name }, &note, controls));
                }
                if m.snapshot.workspace.providers.is_empty() {
                    p = p.child(div().text_color(rgb(MUTED)).child("No providers configured."));
                }
                if !locked {
                    p = p.children(self.provider_edit_card(cx)).child(self.secrets_card(cx));
                }
                p
            }
            "computers" => {
                let ws_id = m.active_ws.clone().unwrap_or_default();
                let tz = workspaceai_shared::schedule::local_tz();
                let now = chrono::Utc::now();
                let mut p = page.child(ui::button("pc-pair", "Pair a computer", ACCENT).on_click(cx.listener(|this, _, _, cx| {
                    if let Some(api) = this.api.clone() {
                        request_pairing(api, this.tx.clone());
                    }
                    cx.notify();
                })));
                if let Some(pair) = self.pairing.clone() {
                    p = p.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_4()
                            .p_3()
                            .rounded_lg()
                            .bg(rgb(PANEL))
                            .border_1()
                            .border_color(rgb(ACCENT))
                            .child(div().font_family("monospace").text_size(px(30.)).child(SharedString::from(pair.code.clone())))
                            .child(div().flex_1().text_sm().child(SharedString::from(computers::pairing_text(&pair, &tz, now))))
                            .child(ui::button("pc-pair-done", "Done", USER_BUBBLE).on_click(cx.listener(|this, _, _, cx| {
                                this.pairing = None;
                                this.model.needs.state = true;
                                cx.notify();
                            }))),
                    );
                }
                for c in &m.computers {
                    let allowed = computers::allowed_for(c, &ws_id);
                    let (c_acc, ws_acc) = (c.clone(), ws_id.clone());
                    let (id_wake, id_del) = (c.id.clone(), c.id.clone());
                    let armed = self.confirm.is_armed(&format!("c:{}", c.id));
                    let sw = ui::switch(format!("pc-sw-{}", c.id), allowed, false).on_click(cx.listener(move |this, _, _, cx| {
                        this.mutate_computer("PATCH", computers::item_path(&c_acc.id), Some(computers::access_body(&c_acc, &ws_acc, !allowed)));
                        cx.notify();
                    }));
                    let mut controls = div().flex().items_center().gap_3().child(div().text_sm().text_color(rgb(MUTED)).child(if allowed { "Allowed here" } else { "Not allowed here" })).child(sw);
                    if computers::can_wake(c) {
                        controls = controls.child(ui::button(format!("pc-wake-{}", c.id), "Wake", ACCENT).on_click(cx.listener(move |this, _, _, cx| {
                            this.mutate_computer("POST", computers::wake_path(&id_wake), None);
                            cx.notify();
                        })));
                    }
                    controls = controls.child(ui::button(format!("pc-del-{}", c.id), if armed { "Click again to unlink" } else { "Unlink" }, if armed { 0x7a2b2b } else { ui::DANGER }).on_click(cx.listener(move |this, _, _, cx| {
                        if this.confirm.press(&format!("c:{id_del}")) {
                            this.mutate_computer("DELETE", computers::item_path(&id_del), None);
                        }
                        cx.notify();
                    })));
                    let last = c.activity.first().map(|a| format!("\nLatest: {}", a.text)).unwrap_or_default();
                    let note = format!("{} · {} · {}{}", computers::status_text(c, &tz, now), computers::capability_text(&c.capabilities), c.os, last);
                    p = p.child(ui::row(&c.name, &note, controls));
                }
                if m.computers.is_empty() {
                    p = p.child(div().text_color(rgb(MUTED)).child("No computers linked yet. Pair one to run routines on it."));
                }
                p
            }
            "projects" => {
                let ws_id = m.active_ws.clone().unwrap_or_default();
                let mut p = page
                    .child(ui::row("Project name", "Type a name and press Enter for a new top-level project. “+ Sub” and “Rename” on a row use this name.", div().w(px(300.)).px_2().py_1().rounded_md().bg(rgb(BG)).child(self.project_input.clone())))
                    .child(
                        ui::button("proj-archived", if self.show_archived { "Hide archived" } else { "Show archived" }, PANEL).on_click(cx.listener(|this, _, _, cx| {
                            this.show_archived = !this.show_archived;
                            cx.notify();
                        })),
                    );
                for n in projects::tree(&m.snapshot.projects, self.show_archived) {
                    let (id_sub, id_ren, id_arc, id_del) = (n.id.clone(), n.id.clone(), n.id.clone(), n.id.clone());
                    let (ws_sub, ws_ren, ws_arc, ws_del) = (ws_id.clone(), ws_id.clone(), ws_id.clone(), ws_id.clone());
                    let (archived, armed) = (n.archived, self.confirm.is_armed(&format!("p:{}", n.id)));
                    let mut controls = div().flex().gap_2();
                    if !archived {
                        controls = controls
                            .child(ui::button(format!("proj-sub-{}", n.id), "+ Sub", ACCENT).on_click(cx.listener(move |this, _, window, cx| {
                                let name = this.project_input.read(cx).content().to_string();
                                if let Some(body) = projects::create_body(&name, Some(&id_sub)) {
                                    this.mutate_path("POST", projects::create_path(&ws_sub), Some(body));
                                    this.project_input.update(cx, |i, cx| i.clear(window, cx));
                                }
                                cx.notify();
                            })))
                            .child(ui::button(format!("proj-ren-{}", n.id), "Rename", USER_BUBBLE).on_click(cx.listener(move |this, _, window, cx| {
                                let name = this.project_input.read(cx).content().to_string();
                                if let Some(body) = projects::rename_body(&name) {
                                    this.mutate_path("PATCH", projects::item_path(&ws_ren, &id_ren), Some(body));
                                    this.project_input.update(cx, |i, cx| i.clear(window, cx));
                                }
                                cx.notify();
                            })));
                    }
                    controls = controls.child(ui::button(format!("proj-arc-{}", n.id), if archived { "Restore" } else { "Archive" }, USER_BUBBLE).on_click(cx.listener(move |this, _, _, cx| {
                        let path = if archived { projects::restore_path(&ws_arc, &id_arc) } else { projects::archive_path(&ws_arc, &id_arc) };
                        this.mutate_path("POST", path, None);
                        cx.notify();
                    })));
                    if n.can_delete {
                        controls = controls.child(ui::button(format!("proj-del-{}", n.id), if armed { "Click again to delete" } else { "Delete" }, if armed { 0x7a2b2b } else { ui::DANGER }).on_click(cx.listener(move |this, _, _, cx| {
                            if this.confirm.press(&format!("p:{id_del}")) {
                                this.mutate_path("DELETE", projects::item_path(&ws_del, &id_del), None);
                            }
                            cx.notify();
                        })));
                    }
                    let chats = m.snapshot.chats.iter().filter(|c| c.project == n.id && !c.archived).count();
                    let note = format!("{} chat{}{}{}", chats, if chats == 1 { "" } else { "s" }, if n.is_general { " · always at the top" } else { "" }, if archived { " · archived" } else { "" });
                    p = p.child(div().pl(px(22. * n.depth as f32)).child(ui::row(&n.name, &note, controls)));
                }
                p
            }
            "routines" => {
                let tz = workspaceai_shared::schedule::local_tz();
                let phrase = self.routine_phrase.read(cx).content().to_string();
                let prompt = self.routine_prompt.read(cx).content().to_string();
                let pv = routines::preview(&phrase, &tz, chrono::Utc::now());
                let can_create = !phrase.trim().is_empty() && pv.ok && !prompt.trim().is_empty();
                let create_body = routines::create_body(&phrase, &prompt, &tz);
                let ws_id = m.active_ws.clone().unwrap_or_default();
                let mut p = page.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .p_3()
                        .rounded_lg()
                        .bg(rgb(PANEL))
                        .child(div().font_weight(FontWeight::BOLD).child("New routine"))
                        .child(div().px_2().py_1().rounded_md().bg(rgb(BG)).child(self.routine_phrase.clone()))
                        .child(div().px_2().py_1().rounded_md().bg(rgb(BG)).child(self.routine_prompt.clone()))
                        .child(div().text_sm().text_color(rgb(if phrase.trim().is_empty() { MUTED } else if pv.ok { 0x7bd88f } else { 0xff6b6b })).child(SharedString::from(if phrase.trim().is_empty() { "Write when it should run, then what it should do.".to_string() } else { pv.text.clone() })))
                        .child(
                            ui::button("routine-create", "Create routine", if can_create { ACCENT } else { RAISED_OFF }).on_click(cx.listener({
                                let ws_id = ws_id.clone();
                                move |this, _, window, cx| {
                                    if let Some(body) = create_body.clone().filter(|_| can_create) {
                                        this.mutate_path("POST", format!("/api/ws/{}/routines", seg(&ws_id)), Some(body));
                                        this.routine_phrase.update(cx, |i, cx| i.clear(window, cx));
                                        this.routine_prompt.update(cx, |i, cx| i.clear(window, cx));
                                    }
                                    cx.notify();
                                }
                            })),
                        ),
                );
                let now = chrono::Utc::now();
                for r in &m.snapshot.routines {
                    let (id_run, id_pause, id_del) = (r.id.clone(), r.id.clone(), r.id.clone());
                    let (ws_run, ws_pause, ws_del) = (ws_id.clone(), ws_id.clone(), ws_id.clone());
                    let paused = r.paused;
                    let armed = self.confirm.is_armed(&format!("r:{}", r.id));
                    let what = match &r.what {
                        workspaceai_client::types::RoutineWhat::Prompt { prompt } => prompt.chars().take(110).collect::<String>(),
                        workspaceai_client::types::RoutineWhat::Graph { .. } => "Runs a graph (not editable in the native app)".into(),
                        workspaceai_client::types::RoutineWhat::Artifact { .. } => "Refreshes an artifact".into(),
                    };
                    let note = format!("{} · {}\n{}", routines::trigger_text(r, now), routines::last_run_text(r), what);
                    let controls = div()
                        .flex()
                        .gap_2()
                        .child(ui::button(format!("rt-run-{}", r.id), "Run now", ACCENT).on_click(cx.listener(move |this, _, _, cx| {
                            this.mutate_path("POST", routines::run_path(&ws_run, &id_run), None);
                            cx.notify();
                        })))
                        .child(ui::button(format!("rt-pause-{}", r.id), if paused { "Resume" } else { "Pause" }, USER_BUBBLE).on_click(cx.listener(move |this, _, _, cx| {
                            this.mutate_path("PATCH", routines::item_path(&ws_pause, &id_pause), Some(routines::pause_body(!paused)));
                            cx.notify();
                        })))
                        .child(ui::button(format!("rt-del-{}", r.id), if armed { "Click again to delete" } else { "Delete" }, if armed { 0x7a2b2b } else { ui::DANGER }).on_click(cx.listener(move |this, _, _, cx| {
                            if this.confirm.press(&format!("r:{id_del}")) {
                                this.mutate_path("DELETE", routines::item_path(&ws_del, &id_del), None);
                            }
                            cx.notify();
                        })));
                    p = p.child(ui::row(if r.name.is_empty() { &r.phrase } else { &r.name }, &note, controls));
                }
                if m.snapshot.routines.is_empty() {
                    p = p.child(div().text_color(rgb(MUTED)).child("No routines yet."));
                }
                p
            }
            other => self.extra_page(other, page, cur.locked, cx),
        };
        let settings_main = div().id("settings-scroll").flex_1().min_w_0().h_full().overflow_y_scroll().child(page);

        // ---- library ----
        let tz = workspaceai_shared::schedule::local_tz();
        let now = chrono::Utc::now();
        let query = self.library_search.read(cx).content().to_string();
        let lib_rows = library::rows(&m.snapshot.artifacts, &m.snapshot.projects, &query);
                let library_list = div()
            .flex()
            .flex_col()
            .w(px(340.))
            .h_full()
            .bg(rgb(PANEL))
            .child(div().m_2().px_3().py_2().rounded_md().bg(rgb(BG)).child(self.library_search.clone()))
            .child(div().id("library-scroll").flex_1().min_h_0().overflow_y_scroll().px_2().children(lib_rows.iter().map(|r| {
                let on = self.library_sel.as_deref() == Some(&r.id);
                let id = r.id.clone();
                div()
                    .id(SharedString::from(format!("art-{}", r.id)))
                    .flex()
                    .flex_col()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(rgb(if on { USER_BUBBLE } else { PANEL }))
                    .hover(|s| s.bg(rgb(USER_BUBBLE)))
                    .child(div().child(SharedString::from(if r.title.is_empty() { "Untitled".to_string() } else { r.title.clone() })))
                    .child(div().text_sm().text_color(rgb(MUTED)).child(SharedString::from(format!("{} · {} · {} version{} · {}", r.kind, r.project, r.versions, if r.versions == 1 { "" } else { "s" }, library::updated_text(r.updated_at, &tz, now)))))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_artifact(id.clone());
                        cx.notify();
                    }))

            })))
            .when(lib_rows.is_empty(), |d| d.child(div().p_4().text_color(rgb(MUTED)).child("No artifacts yet. Ask an agent to make a doc, deck or sheet.")));
        let selected_art = self.library_sel.as_ref().and_then(|id| m.snapshot.artifacts.iter().find(|a| &a.id == id));
        let library_main = div().id("library-view").flex_1().min_w_0().h_full().p(px(28.)).flex().flex_col().gap_3().child(match (selected_art, &self.library_view) {
            (None, _) => div().text_color(rgb(MUTED)).child("Pick an artifact on the left.").into_any_element(),
            (Some(a), view) => {
                let format = view.as_ref().and_then(|r| r.as_ref().ok()).map_or(String::new(), |v| format!(" · {}", v.format));
                div().flex().flex_col().flex_1().min_h_0().gap_3().max_w(px(1100.)).child(ui::page_title(&a.title, &format!("{} · version {}{format}", a.kind, a.version))).child(self.artifact_body(a, cx)).into_any_element()
            }
        });

        let lib_on = self.view == View::Library;
        let lib_button = div()
            .id("library-btn")
            .flex()
            .items_center()
            .justify_center()
            .size(px(40.))
            .rounded_lg()
            .cursor_pointer()
            .bg(rgb(if lib_on { ACCENT } else { PANEL }))
            .child("▤")
            .on_click(cx.listener(|this, _, _, cx| {
                this.view = if this.view == View::Library { View::Chat } else { View::Library };
                cx.notify();
            }));
        let rail = rail.child(
            div()
                .id("add-workspace")
                .flex()
                .items_center()
                .justify_center()
                .size(px(40.))
                .rounded_lg()
                .cursor_pointer()
                .border_1()
                .border_color(rgb(MUTED))
                .text_color(rgb(MUTED))
                .child("＋")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.open_new_workspace("team", cx);
                    cx.notify();
                })),
        );
        let rail_btn = |id: &'static str, glyph: &'static str, on: bool| div().id(id).flex().items_center().justify_center().size(px(40.)).rounded_lg().cursor_pointer().bg(rgb(if on { ACCENT } else { PANEL })).child(glyph);
        let pending = self.approvals_count();
        let rail = rail
            .child(rail_btn("home-btn", "⌂", self.view == View::Home).on_click(cx.listener(|this, _, _, cx| this.open_home(cx))))
            .child(rail_btn("create-btn", "✦", self.view == View::Create).on_click(cx.listener(|this, _, _, cx| this.open_create(cx))))
            .child(rail_btn("approvals-btn", if pending > 0 { "!" } else { "✓" }, self.view == View::Approvals).on_click(cx.listener(|this, _, _, cx| {
                this.open_approvals();
                cx.notify();
            })))
            .child(lib_button);
        let gear_on = self.view == View::Settings;
        let rail = rail.child(
            div()
                .id("settings-btn")
                .mt_auto()
                .flex()
                .items_center()
                .justify_center()
                .size(px(40.))
                .rounded_lg()
                .cursor_pointer()
                .bg(rgb(if gear_on { ACCENT } else { PANEL }))
                .child("⚙")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.view = if this.view == View::Settings { View::Chat } else { View::Settings };
                    if this.view == View::Settings {
                        let name = this.model.snapshot.workspace.name.clone();
                        this.name_input.update(cx, |i, cx| i.set_content(&name, cx));
                    }
                    cx.notify();
                })),
        );
        let (left, right) = match self.view {
            View::Settings => (settings_nav.into_any_element(), settings_main.into_any_element()),
            View::Library => (library_list.into_any_element(), library_main.into_any_element()),
            View::Chat => {
                let side = self.side_panel(cx);
                let wide = side.as_ref().is_some_and(|(_, w)| *w);
                (sidebar.into_any_element(), div().flex().flex_1().min_w_0().h_full().when(!wide, |d| d.child(main)).children(side.map(|(p, _)| p)).into_any_element())
            }
            View::Home => (sidebar.into_any_element(), self.home_view(cx).into_any_element()),
            View::Create => (self.create_nav(cx).into_any_element(), self.create_view(cx).into_any_element()),
            View::Approvals => (sidebar.into_any_element(), self.approvals_view(cx).into_any_element()),
            View::Project => (sidebar.into_any_element(), self.project_view(cx).into_any_element()),
        };

        let overlay = self.palette_open.then(|| {
            let items = self.palette_items(cx);
            let sel = self.palette_sel.min(items.len().saturating_sub(1));
            div()
                .id("palette-backdrop")
                .absolute()
                .inset_0()
                .flex()
                .justify_center()
                .pt(px(90.))
                .bg(rgba(0x000000aa))
                .on_click(cx.listener(|this, _, window, cx| this.close_palette(window, cx)))
                .child(
                    div()
                        .key_context("Palette")
                        .on_action(cx.listener(|this, _: &PaletteDown, _, cx| {
                            this.palette_sel += 1;
                            cx.notify();
                        }))
                        .on_action(cx.listener(|this, _: &PaletteUp, _, cx| {
                            this.palette_sel = this.palette_sel.saturating_sub(1);
                            cx.notify();
                        }))
                        .id("palette")
                        .w(px(620.))
                        .h_auto()
                        .max_h(px(520.))
                        .flex()
                        .flex_col()
                        .rounded_lg()
                        .bg(rgb(PANEL))
                        .border_1()
                        .border_color(rgb(USER_BUBBLE))
                        .child(div().px_4().py_3().border_b_1().border_color(rgb(USER_BUBBLE)).child(self.palette_input.clone()))
                        .child(div().id("palette-list").flex().flex_col().p_1().overflow_y_scroll().children(items.into_iter().enumerate().map(|(i, it)| {
                            let id = it.id.clone();
                            div()
                                .id(SharedString::from(format!("pal-{i}")))
                                .flex()
                                .justify_between()
                                .px_3()
                                .py_2()
                                .rounded_md()
                                .cursor_pointer()
                                .bg(rgb(if i == sel { USER_BUBBLE } else { PANEL }))
                                .child(SharedString::from(it.label))
                                .child(div().text_sm().text_color(rgb(MUTED)).child(SharedString::from(it.hint)))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.palette_sel = i;
                                    let _ = &id;
                                    this.run_palette(window, cx);
                                }))
                        }))),
                )
        });

        div()
            .key_context("App")
            .on_action(cx.listener(|this, _: &OpenPalette, window, cx| this.open_palette(window, cx)))
            .on_action(cx.listener(|this, _: &ClosePalette, window, cx| this.close_palette(window, cx)))
            .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _, cx| {
                // Ctrl/Cmd+1..9 switches workspace, like the web app.
                let mods = ev.keystroke.modifiers;
                if (if cfg!(target_os = "macos") { mods.platform } else { mods.control }) && !mods.alt && !mods.shift {
                    if let (Some(id), Some(api)) = (this.model.workspace_for_shortcut(&ev.keystroke.key).map(String::from), this.api.clone()) {
                        if this.model.active_ws.as_deref() != Some(&id) {
                            activate(api, id, this.tx.clone());
                        }
                        this.view = View::Chat;
                        cx.notify();
                    }
                }
            }))
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _, _| {
                this.side.art.borrow().release_focus();
                this.side.br.borrow().release_focus();
            }))
            .relative()
            .flex()
            .size_full()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .child(rail)
            .child(left)
            .child(right)
            .children(self.artifact_fullscreen(cx))
            .children(overlay)
            .children(self.new_workspace_dialog(cx))
    }
}

/// `--smoke`: starts the supervisor, reads `/api/state`, stops it. Proves the backend wiring without a window.
fn smoke() -> Result<(), String> {
    let (info, _sup) = Supervisor::start(&repo_root(), &node_binary())?;
    let api = Api::new(&info);
    let state: AppState = fetch(&api, "/api/state")?;
    println!("SMOKE ok: {} workspaces, active {:?}, {} computers", state.workspaces.len(), state.settings.active_workspace, state.computers.len());
    if let Some(ws) = state.settings.active_workspace.or_else(|| state.workspaces.first().map(|w| w.id.clone())) {
        let snap: Snapshot = fetch(&api, &format!("/api/ws/{}/snapshot", seg(&ws)))?;
        println!("SMOKE ok: {} projects, {} chats", snap.projects.len(), snap.chats.len());
        match api.get(&providers::secrets_list_path(&ws)) {
            Ok(v) => println!("SMOKE ok: {} providers, {} stored secrets (names only)", snap.workspace.providers.len(), v["names"].as_array().map_or(0, Vec::len)),
            Err(e) => println!("SMOKE secrets: {e}"),
        }
        println!("SMOKE ok: {} artifacts", snap.artifacts.len());
        if let Some(a) = snap.artifacts.first() {
            match api.get(&library::content_path(&ws, &a.id, None)).ok().and_then(|v| library::parse_view(&v)) {
                Some(v) => println!("SMOKE ok: first artifact {:?} is {} ({} chars)", a.title, v.format, v.content.chars().count()),
                None => println!("SMOKE artifact content: could not read {:?}", a.title),
            }
        }
        println!("SMOKE ok: {} connectors", snap.workspace.mcp.len());
        match api.get(&subscriptions::list_path(&ws)) {
            Ok(v) => {
                let subs = subscriptions::parse_list(&v);
                println!("SMOKE ok: {} agent subscriptions ({} connected)", subs.len(), subs.iter().filter(|s| s.connected).count());
            }
            Err(e) => println!("SMOKE subscriptions: {e}"),
        }
        println!("SMOKE ok: {} routines ({} paused)", snap.routines.len(), snap.routines.iter().filter(|r| r.paused).count());
        println!("SMOKE ok: {} memory facts ({} suggested)", snap.facts.len(), snap.facts.iter().filter(|f| f.status == "suggested").count());
        println!("SMOKE ok: workspace {:?} ({}), memory {:?}, engine {:?}", snap.workspace.name, snap.workspace.kind, snap.workspace.use_memory, snap.engine.state);
        if let Some(p) = snap.projects.iter().find(|p| !p.archived) {
            match api.get(&files::search_path(&ws, &p.id, "")) {
                Ok(v) => println!("SMOKE ok: {} files for an empty @ query in {:?}", files::parse_files(&v).len(), p.name),
                Err(e) => println!("SMOKE files: {e}"),
            }
        }
        match fetch::<ModelList>(&api, &format!("/api/ws/{}/models", seg(&ws))) {
            Ok(m) => println!("SMOKE ok: {} opencode models, {} other agents", m.options_for("opencode").len(), m.agents.len()),
            Err(e) => println!("SMOKE models: {e}"),
        }
    }
    Ok(())
}

const OPEN_PALETTE: &str = if cfg!(target_os = "macos") { "cmd-k" } else { "ctrl-k" };

fn app_bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new(OPEN_PALETTE, OpenPalette, None),
        KeyBinding::new("escape", ClosePalette, None),
        KeyBinding::new("down", PaletteDown, Some("Palette")),
        KeyBinding::new("up", PaletteUp, Some("Palette")),
    ]
}

/// `--term <cmd> [args…]`, or one quoted string split on whitespace; no command = `$SHELL`.
fn term_argv(rest: &[String]) -> Vec<String> {
    match rest {
        [] => vec![std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())],
        [one] => one.split_whitespace().map(String::from).collect(),
        many => many.to_vec(),
    }
}

/// Debug harness: one window, only a terminal.
fn run_term(argv: Vec<String>) {
    gpui_platform::application().run(move |cx: &mut App| {
        terminal_view::bind_keys(cx);
        let bounds = Bounds::centered(None, size(px(900.), px(600.)), cx);
        cx.open_window(WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() }, |window, cx| {
            cx.new(|cx| terminal_view::TermHarness::new(argv, window, cx))
        })
        .unwrap();
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn term_argv_forms() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(super::term_argv(&s(&["htop -d 5"])), s(&["htop", "-d", "5"]));
        assert_eq!(super::term_argv(&s(&["bash", "-c", "echo hi"])), s(&["bash", "-c", "echo hi"]));
        assert_eq!(super::term_argv(&[]).len(), 1);
    }

    #[test]
    fn app_key_bindings_are_valid_keystrokes() {
        // KeyBinding::new panics on a keystroke GPUI cannot parse; that once killed the app at startup.
        assert_eq!(super::app_bindings().len(), 4);
    }
}

fn main() {
    if std::env::args().any(|a| a == "--smoke") {
        if let Err(e) = smoke() {
            eprintln!("SMOKE failed: {e}");
            std::process::exit(1);
        }
        return;
    }
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--term") {
        return run_term(term_argv(&args[i + 1..]));
    }
    webview::init();
    gpui_platform::application().run(|cx: &mut App| {
        text_input::bind_keys(cx);
        cx.bind_keys(app_bindings());
        let bounds = Bounds::centered(None, size(px(1200.), px(780.)), cx);
        cx.open_window(WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() }, |window, cx| cx.new(|cx| AppView::new(window, cx))).unwrap();
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
