//! WorkspaceAI native app (GPUI). Slice A: workspaces, projects, chats, messages and sending.
//! The window renders `workspaceai_client::model::Model`; background threads talk to the supervisor and report
//! back through one channel that the window drains on a timer.

mod md_view;
mod text_input;

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
use workspaceai_client::model::{Connection, Model, RowKind};
use workspaceai_client::models::ModelList;
use workspaceai_client::supervisor::Supervisor;
use workspaceai_client::command::{self, Outcome};
use workspaceai_client::markdown;
use workspaceai_shared::commands::{parse_slash, ChatCommand};
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

struct AppView {
    model: Model,
    rx: Receiver<Msg>,
    tx: Sender<Msg>,
    api: Option<Api>,
    _supervisor: Option<Supervisor>,
    input: Entity<TextInput>,
    scroll: ScrollHandle,
    local_id: u64,
    commands: Vec<ChatCommand>,
    models: ModelList,
    picker_open: bool,
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
        cx.subscribe_in(&input, window, |this, input, event: &TextInputEvent, window, cx| {
            if *event == TextInputEvent::Submit {
                this.submit(input, window, cx);
            }
        })
        .detach();
        cx.observe(&input, |_, _, cx| cx.notify()).detach();
        window.focus(&input.focus_handle(cx), cx);
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(50)).await;
            if this.update(cx, |this, cx| this.pump(cx)).is_err() {
                break;
            }
        })
        .detach();
        AppView { model: Model::default(), rx, tx, api: None, _supervisor: None, input, scroll: ScrollHandle::new(), local_id: 0, commands: Vec::new(), models: ModelList::default(), picker_open: false, panel: None }
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

        let main = div().flex().flex_col().flex_1().min_w_0().h_full().child(header).children(picker).child(messages).children(toast).children(panel).children(suggest_box).child(composer);

        div().flex().size_full().bg(rgb(BG)).text_color(rgb(TEXT)).child(rail).child(sidebar).child(main)
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
