//! WorkspaceAI native app (GPUI). Slice A: workspaces, projects, chats, messages and sending.
//! The window renders `workspaceai_client::model::Model`; background threads talk to the supervisor and report
//! back through one channel that the window drains on a timer.

mod md_view;
mod text_input;
mod ui;

use gpui::{
    div, prelude::*, px, rgb, size, App, Bounds, Context, Entity, Focusable, FontWeight, ScrollHandle, SharedString, Window, WindowBounds,
    WindowOptions,
};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;
use text_input::{TextInput, TextInputEvent};
use workspaceai_client::api::Api;
use workspaceai_client::events;
use workspaceai_client::files;
use workspaceai_client::model::{Connection, Model, RowKind};
use workspaceai_client::models::ModelList;
use workspaceai_client::settings::{self, engine_actions, EngineAction};
use workspaceai_client::supervisor::Supervisor;
use workspaceai_client::chats::{self, Confirm};
use workspaceai_client::command::{self, Outcome};
use workspaceai_client::markdown;
use workspaceai_shared::commands::{insert_mention, mention_at, parse_slash, ChatCommand};
use workspaceai_client::types::{AppState, Chat, ChatMessage, PermissionRequest, Snapshot, UiEvent};

const BG: u32 = 0x1b1b1f;
const PANEL: u32 = 0x232329;
const RAIL: u32 = 0x151518;
const TEXT: u32 = 0xe6e6e8;
const MUTED: u32 = 0x8b8b95;
const ACCENT: u32 = 0xd97757;
const USER_BUBBLE: u32 = 0x33333b;

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
    ChatCreated(String, Chat),
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
        let _ = tx.send(match fetch::<Snapshot>(&api, &format!("/api/ws/{ws}/snapshot")) {
            Ok(s) => Msg::Snapshot(ws, s),
            Err(e) => Msg::Error(e),
        });
    });
}

fn load_messages(api: Api, ws: String, chat: String, tx: Sender<Msg>) {
    spawn(move || {
        let r = api.get(&format!("/api/ws/{ws}/chats/{chat}/messages")).map_err(|e| e.to_string()).and_then(|v| {
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
        let r = api.get(&format!("/api/ws/{ws}/chats/{chat}/permissions")).map_err(|e| e.to_string()).and_then(|v| {
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
        let r = api.post(&format!("/api/ws/{ws}/chats/{chat}/fork"), None).map_err(|e| e.to_string()).and_then(|v| serde_json::from_value::<Chat>(v).map_err(|e| e.to_string()));
        let _ = tx.send(match r {
            Ok(c) => Msg::ChatCreated(ws, c),
            Err(e) => Msg::Error(e),
        });
    });
}

fn delete_chat(api: Api, ws: String, chat: String, tx: Sender<Msg>) {
    spawn(move || match api.del(&format!("/api/ws/{ws}/chats/{chat}")) {
        Ok(_) => load_snapshot(api, ws, tx),
        Err(e) => {
            let _ = tx.send(Msg::Error(e.to_string()));
        }
    });
}

/// Changes one workspace setting; the reply is the fresh snapshot.
fn patch_workspace(api: Api, ws: String, body: serde_json::Value, tx: Sender<Msg>) {
    spawn(move || {
        let _ = tx.send(match api.patch(&format!("/api/ws/{ws}"), &body).map_err(|e| e.to_string()).and_then(|v| serde_json::from_value::<Snapshot>(v).map_err(|e| e.to_string())) {
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

fn load_models(api: Api, ws: String, tx: Sender<Msg>) {
    spawn(move || {
        // The list is slow when an engine has to start; a failure just leaves the picker empty.
        if let Ok(list) = fetch::<ModelList>(&api, &format!("/api/ws/{ws}/models")) {
            let _ = tx.send(Msg::Models(ws, list));
        }
    });
}

/// Changes a chat's model or effort, then refreshes the snapshot so every view agrees.
fn patch_chat(api: Api, ws: String, chat: String, body: serde_json::Value, tx: Sender<Msg>) {
    spawn(move || match api.patch(&format!("/api/ws/{ws}/chats/{chat}"), &body) {
        Ok(_) => load_snapshot(api, ws, tx),
        Err(e) => {
            let _ = tx.send(Msg::Error(e.to_string()));
        }
    });
}

fn load_commands(api: Api, ws: String, chat: String, tx: Sender<Msg>) {
    spawn(move || {
        let r = api.get(&format!("/api/ws/{ws}/chats/{chat}/commands")).map_err(|e| e.to_string()).and_then(|v| {
            serde_json::from_value::<Vec<ChatCommand>>(v["commands"].clone()).map_err(|e| format!("unexpected commands reply: {e}"))
        });
        if let Ok(list) = r {
            let _ = tx.send(Msg::Commands(ws, chat, list));
        }
    });
}

fn run_command(api: Api, ws: String, chat: String, text: String, tx: Sender<Msg>) {
    spawn(move || {
        let _ = tx.send(match api.post(&format!("/api/ws/{ws}/chats/{chat}/command"), Some(&serde_json::json!({ "text": text }))) {
            Ok(v) => Msg::CommandDone(command::interpret(&v)),
            Err(e) => Msg::Error(e.to_string()),
        });
    });
}

fn answer_permission(api: Api, ws: String, chat: String, request: String, decision: &'static str, tx: Sender<Msg>) {
    spawn(move || {
        if let Err(e) = api.post(&format!("/api/ws/{ws}/chats/{chat}/permission/{request}"), Some(&serde_json::json!({ "decision": decision }))) {
            let _ = tx.send(Msg::Error(e.to_string()));
            load_permissions(api, ws, chat, tx);
        }
    });
}

fn stop_chat(api: Api, ws: String, chat: String, tx: Sender<Msg>) {
    spawn(move || {
        if let Err(e) = api.post(&format!("/api/ws/{ws}/chats/{chat}/stop"), None) {
            let _ = tx.send(Msg::Error(e.to_string()));
        }
    });
}

fn send_text(api: Api, ws: String, chat: String, text: String, tx: Sender<Msg>) {
    spawn(move || {
        if let Err(e) = api.post(&format!("/api/ws/{ws}/chats/{chat}/send"), Some(&serde_json::json!({ "text": text }))) {
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
        let r = api.post(&format!("/api/ws/{ws}/chats"), Some(&body)).map_err(|e| e.to_string()).and_then(|v| serde_json::from_value::<Chat>(v).map_err(|e| e.to_string()));
        let _ = tx.send(match r {
            Ok(c) => Msg::ChatCreated(ws, c),
            Err(e) => Msg::Error(e),
        });
    });
}

fn activate(api: Api, ws: String, tx: Sender<Msg>) {
    spawn(move || {
        let _ = tx.send(match api.post(&format!("/api/workspaces/{ws}/activate"), None).map_err(|e| e.to_string()).and_then(|v| serde_json::from_value::<AppState>(v).map_err(|e| e.to_string())) {
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Chat,
    Settings,
}

struct AppView {
    view: View,
    settings_page: &'static str,
    name_input: Entity<TextInput>,
    model: Model,
    rx: Receiver<Msg>,
    tx: Sender<Msg>,
    api: Option<Api>,
    _supervisor: Option<Supervisor>,
    input: Entity<TextInput>,
    rename_input: Entity<TextInput>,
    renaming: bool,
    confirm: Confirm,
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
        AppView { view: View::Chat, settings_page: "general", name_input, model: Model::default(), rx, tx, api: None, _supervisor: None, input, rename_input, renaming: false, confirm: Confirm::default(), scroll: ScrollHandle::new(), local_id: 0, commands: Vec::new(), models: ModelList::default(), picker_open: false, files: Vec::new(), last_query: None, panel: None }
    }

    /// Drains background messages into the model and refetches whatever events made stale.
    fn pump(&mut self, cx: &mut Context<Self>) {
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
                let active = s.settings.active_workspace.clone().or_else(|| s.workspaces.first().map(|w| w.id.clone()));
                let changed = active != self.model.active_ws;
                self.model.workspaces = s.workspaces;
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
                if self.model.active_ws.as_deref() == Some(&ws) {
                    self.model.snapshot = snap;
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
                    self.open_chat(id);
                    self.model.needs.snapshot = true;
                }
            }
            Msg::Error(e) => self.model.toast = Some(e),
        }
    }

    fn open_chat(&mut self, id: String) {
        self.model.open_chat(id.clone(), Vec::new());
        self.commands.clear();
        self.panel = None;
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

    fn run_engine(&mut self, action: EngineAction) {
        if let (Some(api), Some(ws)) = (self.api.clone(), self.model.active_ws.clone()) {
            engine_action(api, ws, action, self.tx.clone());
        }
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

    fn fork(&mut self) {
        if let (Some(api), Some(ws), Some(id)) = (self.api.clone(), self.model.active_ws.clone(), self.model.open_chat_id.clone()) {
            fork_chat(api, ws, id, self.tx.clone());
        }
    }

    fn toggle_full_access(&mut self) {
        let (Some(api), Some(ws), Some(id)) = (self.api.clone(), self.model.active_ws.clone(), self.model.open_chat_id.clone()) else { return };
        let Some(chat) = self.model.snapshot.chats.iter_mut().find(|c| c.id == id) else { return };
        let on = chat.yolo != Some(true);
        chat.yolo = Some(on);
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
        self.model.messages.push(ChatMessage { id: format!("local-{}", self.local_id), role: "user".into(), text: text.clone(), tools: vec![], error: None, created_at: 0 });
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let m = &self.model;
        let rail = div().flex().flex_col().gap_2().p_2().w(px(56.)).h_full().bg(rgb(RAIL)).children(m.workspaces.iter().map(|w| {
            let active = m.active_ws.as_deref() == Some(&w.id);
            let id = w.id.clone();
            div()
                .id(SharedString::from(format!("ws-{}", w.id)))
                .flex()
                .items_center()
                .justify_center()
                .size(px(40.))
                .rounded_lg()
                .cursor_pointer()
                .bg(rgb(if active { ACCENT } else { PANEL }))
                .text_color(rgb(if active { 0xffffff } else { TEXT }))
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
                    RowKind::Project => base.text_color(rgb(MUTED)).font_weight(FontWeight::BOLD).child(SharedString::from(row.title)),
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
            })));

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
                .child(button("act-delete", if armed { "Click again to delete".into() } else { "Delete".into() }, if armed { 0x7a2b2b } else { PANEL }).on_click(cx.listener(|this, _, _, cx| {
                    this.delete();
                    cx.notify();
                })))
                .child(button("act-full", if full { "Full access: on".into() } else { "Full access: off".into() }, if full { 0x7a4a1b } else { PANEL }).on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_full_access();
                    cx.notify();
                })))
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
                div().flex().when(user, |d| d.justify_end()).child(bubble)
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

        let composer = div().p_3().child(div().px_3().py_2().rounded_lg().bg(rgb(PANEL)).text_size(px(15.)).line_height(px(22.)).child(self.input.clone()));

        let main = div().flex().flex_col().flex_1().min_w_0().h_full().child(header).children(actions).children(rename_row).children(picker).child(messages).children(toast).children(panel).children(suggest_box).children(file_box).child(composer);

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
                    if id == "general" {
                        let name = this.model.snapshot.workspace.name.clone();
                        this.name_input.update(cx, |i, cx| i.set_content(&name, cx));
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
                .child(ui::row("Default model", "Used for new chats. Change it per chat from the chat header.", div().text_color(rgb(MUTED)).child(SharedString::from(if w.default_model.is_empty() { "Default".to_string() } else { w.default_model.clone() }))))
                .child(ui::row("Workspace instructions", "Read-only here for now. Edit them in the web app.", div().max_w(px(320.)).text_sm().text_color(rgb(MUTED)).child(SharedString::from(w.instructions.chars().take(160).collect::<String>())))),
            "memory" => page
                .child(switch_row("useMemory", "Use memory", "Agents are told to use this workspace's memory, and each session opens with what it holds.", settings::switch_on(w.use_memory), cur.locked))
                .child(switch_row("memoryRecall", "Recall on every message", "Look each message up in memory and hand the agent what matched.", settings::switch_on(w.memory_recall), cur.locked))
                .child(switch_row("suggestMemories", "Suggest memories", "Offer to remember things from chats.", w.suggest_memories == Some(true), cur.locked)),
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
                p.child(div().flex().gap_2().children(engine_actions(&e.state).into_iter().map(|a| {
                    ui::button(format!("engine-{}", a.label()), a.label(), if a == EngineAction::Stop { ui::DANGER } else { ACCENT }).on_click(cx.listener(move |this, _, _, cx| {
                        this.run_engine(a);
                        cx.notify();
                    }))
                })))
            }
            _ => page.child(div().p_4().rounded_lg().bg(rgb(PANEL)).text_color(rgb(MUTED)).child("This page is not in the native app yet. Use the web app for it for now.")),
        };
        let settings_main = div().id("settings-scroll").flex_1().min_w_0().h_full().overflow_y_scroll().child(page);

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
        let (left, right) = if gear_on { (settings_nav.into_any_element(), settings_main.into_any_element()) } else { (sidebar.into_any_element(), main.into_any_element()) };

        div().flex().size_full().bg(rgb(BG)).text_color(rgb(TEXT)).child(rail).child(left).child(right)
    }
}

/// `--smoke`: starts the supervisor, reads `/api/state`, stops it. Proves the backend wiring without a window.
fn smoke() -> Result<(), String> {
    let (info, _sup) = Supervisor::start(&repo_root(), &node_binary())?;
    let api = Api::new(&info);
    let state: AppState = fetch(&api, "/api/state")?;
    println!("SMOKE ok: {} workspaces, active {:?}", state.workspaces.len(), state.settings.active_workspace);
    if let Some(ws) = state.settings.active_workspace.or_else(|| state.workspaces.first().map(|w| w.id.clone())) {
        let snap: Snapshot = fetch(&api, &format!("/api/ws/{ws}/snapshot"))?;
        println!("SMOKE ok: {} projects, {} chats", snap.projects.len(), snap.chats.len());
        println!("SMOKE ok: workspace {:?} ({}), memory {:?}, engine {:?}", snap.workspace.name, snap.workspace.kind, snap.workspace.use_memory, snap.engine.state);
        if let Some(p) = snap.projects.iter().find(|p| !p.archived) {
            match api.get(&files::search_path(&ws, &p.id, "")) {
                Ok(v) => println!("SMOKE ok: {} files for an empty @ query in {:?}", files::parse_files(&v).len(), p.name),
                Err(e) => println!("SMOKE files: {e}"),
            }
        }
        match fetch::<ModelList>(&api, &format!("/api/ws/{ws}/models")) {
            Ok(m) => println!("SMOKE ok: {} opencode models, {} other agents", m.options_for("opencode").len(), m.agents.len()),
            Err(e) => println!("SMOKE models: {e}"),
        }
    }
    Ok(())
}

fn main() {
    if std::env::args().any(|a| a == "--smoke") {
        if let Err(e) = smoke() {
            eprintln!("SMOKE failed: {e}");
            std::process::exit(1);
        }
        return;
    }
    gpui_platform::application().run(|cx: &mut App| {
        text_input::bind_keys(cx);
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
