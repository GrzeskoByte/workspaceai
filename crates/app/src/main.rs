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
use workspaceai_client::supervisor::Supervisor;
use workspaceai_client::markdown;
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
        window.focus(&input.focus_handle(cx), cx);
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(50)).await;
            if this.update(cx, |this, cx| this.pump(cx)).is_err() {
                break;
            }
        })
        .detach();
        AppView { model: Model::default(), rx, tx, api: None, _supervisor: None, input, scroll: ScrollHandle::new(), local_id: 0 }
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
        if let (Some(api), Some(ws)) = (self.api.clone(), self.model.active_ws.clone()) {
            load_messages(api.clone(), ws.clone(), id.clone(), self.tx.clone());
            load_permissions(api, ws, id, self.tx.clone());
        }
    }

    fn answer(&mut self, request: &str, decision: &'static str) {
        let (Some(api), Some(ws), Some(chat)) = (self.api.clone(), self.model.active_ws.clone(), self.model.open_chat_id.clone()) else { return };
        self.model.drop_permission(&chat, request);
        answer_permission(api, ws, chat, request.to_string(), decision, self.tx.clone());
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
                    .child(div().text_sm().text_color(rgb(MUTED)).child(status_line(m))),
            );

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

        let composer = div().p_3().child(div().px_3().py_2().rounded_lg().bg(rgb(PANEL)).text_size(px(15.)).line_height(px(22.)).child(self.input.clone()));

        let main = div().flex().flex_col().flex_1().min_w_0().h_full().child(header).child(messages).children(toast).child(composer);

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
