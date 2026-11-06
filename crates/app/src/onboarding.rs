//! First-run setup (the ten steps of `Onboarding.tsx`), the New workspace / join organization dialog, and the
//! small requests behind General's delete and Data's export. Pure logic is in `workspaceai_client::{onboarding,
//! spaces}`; this module is the window code. It is a child of the crate root, so it can use `AppView`'s fields.

use crate::text_input::TextInput;
use crate::ui;
use crate::{activate, load_state, mutate_state, AppView, Msg, View, ACCENT, BG, MUTED, PANEL, RAISED_OFF, RAIL, TEXT, USER_BUBBLE};
use gpui::{div, prelude::*, px, rgb, rgba, AnyElement, Context, Div, Entity, FontWeight, SharedString};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};
use workspaceai_client::api::Api;
use workspaceai_client::onboarding::{self as ob, Detect, Draft, Gate, OdStatus, Selection, Session};
use workspaceai_client::spaces;
use workspaceai_client::types::Subscription;
use workspaceai_client::workspace as wsx;

const SUCCESS: u32 = 0x7fd88f;
const ERROR: u32 = 0xe06c75;
const WARN: u32 = 0xf5a742;
const TEXT2: u32 = 0xd4d4d4;

#[derive(Clone, Default, PartialEq)]
pub enum Check {
    #[default]
    Idle,
    Busy,
    Done(bool, String),
}

impl Check {
    fn done(r: (bool, String)) -> Check {
        Check::Done(r.0, r.1)
    }
    fn ok(&self) -> bool {
        matches!(self, Check::Done(true, _))
    }
    fn line(&self, idle: &str) -> Div {
        let (text, color) = match self {
            Check::Idle => (idle.to_string(), MUTED),
            Check::Busy => ("…".into(), MUTED),
            Check::Done(ok, t) => (t.clone(), if *ok { SUCCESS } else { ERROR }),
        };
        div().text_sm().text_color(rgb(color)).child(SharedString::from(text))
    }
}

/// The wizard's state and inputs, and the dialog for a new workspace.
pub struct Onb {
    pub loaded: bool,
    started: bool,
    step: usize,
    step_max: usize,
    tour: usize,
    remote: bool,
    detect: Option<Detect>,
    oc_conn: Check,
    od: Option<OdStatus>,
    od_later: bool,
    od_polled: Option<Instant>,
    subs: Option<Vec<Subscription>>,
    subs_on: HashSet<String>,
    subs_err: String,
    sessions: Option<Vec<Session>>,
    scan_err: String,
    sel: Selection,
    sync: bool,
    accent: &'static str,
    prov: Option<String>,
    prov_test: Check,
    ollama: Vec<String>,
    web_mode: &'static str,
    web_prov: &'static str,
    web_test: Check,
    web_fetch: bool,
    web_ask: bool,
    mem: &'static str,
    folders: Vec<String>,
    watch: bool,
    tpl: &'static str,
    agents_dirty: bool,
    proj_last: String,
    busy: bool,
    scan_busy: bool,
    subs_busy: bool,
    binary: Entity<TextInput>,
    oc_url: Entity<TextInput>,
    oc_user: Entity<TextInput>,
    oc_pass: Entity<TextInput>,
    ws_name: Entity<TextInput>,
    key: Entity<TextInput>,
    url: Entity<TextInput>,
    model: Entity<TextInput>,
    web_key: Entity<TextInput>,
    searx: Entity<TextInput>,
    folder: Entity<TextInput>,
    proj_name: Entity<TextInput>,
    proj_folder: Entity<TextInput>,
    agents: Entity<TextInput>,
    // New workspace dialog
    pub(crate) nw_kind: Option<&'static str>,
    nw_busy: bool,
    nw_name: Entity<TextInput>,
    nw_email: Entity<TextInput>,
    nw_issuer: Entity<TextInput>,
}

impl Onb {
    pub fn new(cx: &mut Context<AppView>) -> Self {
        let make = |cx: &mut Context<AppView>, ph: &str, init: &str, masked: bool| {
            let i = cx.new(|cx| {
                let mut t = if masked { TextInput::new_masked(cx, ph) } else { TextInput::new(cx, ph) };
                if !init.is_empty() {
                    t.set_content(init, cx);
                }
                t
            });
            cx.observe(&i, |_, _, cx| cx.notify()).detach();
            i
        };
        let agents = cx.new(|cx| {
            let mut t = TextInput::new_multiline(cx, "Instructions for this project");
            t.set_content(&ob::agents_template("Software", "My project"), cx);
            t
        });
        let proj_name = make(cx, "Project name", "My project", false);
        // Typing a name suggests a folder and refreshes the AGENTS.md text while it is still the template.
        cx.observe(&proj_name, |this, input, cx| {
            let name = input.read(cx).content().to_string();
            if name != this.onb.proj_last {
                this.onb.proj_last = name.clone();
                let folder = format!("~/code/{}", ob::slugify(&name, "project"));
                this.onb.proj_folder.update(cx, |i, cx| i.set_content(&folder, cx));
                if !this.onb.agents_dirty {
                    let text = ob::agents_template(this.onb.tpl, if name.is_empty() { "My project" } else { &name });
                    this.onb.agents.update(cx, |i, cx| i.set_content(&text, cx));
                }
            }
            cx.notify();
        })
        .detach();
        cx.observe(&agents, |this, input, cx| {
            let name = this.onb.proj_name.read(cx).content().to_string();
            if input.read(cx).content() != ob::agents_template(this.onb.tpl, if name.is_empty() { "My project" } else { &name }) {
                this.onb.agents_dirty = true;
            }
            cx.notify();
        })
        .detach();
        Onb {
            loaded: false,
            started: false,
            step: 0,
            step_max: 0,
            tour: 0,
            remote: false,
            detect: None,
            oc_conn: Check::Idle,
            od: None,
            od_later: false,
            od_polled: None,
            subs: None,
            subs_on: HashSet::new(),
            subs_err: String::new(),
            sessions: None,
            scan_err: String::new(),
            sel: Selection::default(),
            sync: true,
            accent: "default",
            prov: None,
            prov_test: Check::Idle,
            ollama: vec!["qwen3-coder".into()],
            web_mode: "exa",
            web_prov: "Tavily",
            web_test: Check::Idle,
            web_fetch: true,
            web_ask: false,
            mem: "both",
            folders: vec!["~/code".into()],
            watch: true,
            tpl: "Software",
            agents_dirty: false,
            proj_last: "My project".into(),
            busy: false,
            scan_busy: false,
            subs_busy: false,
            binary: make(cx, "opencode", "opencode", false),
            oc_url: make(cx, "http://127.0.0.1:4096", "http://127.0.0.1:4096", false),
            oc_user: make(cx, "Username", "opencode", false),
            oc_pass: make(cx, "OPENCODE_SERVER_PASSWORD", "", true),
            ws_name: make(cx, "Workspace name", "Personal", false),
            key: make(cx, "paste your key", "", true),
            url: make(cx, "https://[YOUR-ENDPOINT]/v1", "https://[YOUR-ENDPOINT]/v1", false),
            model: make(cx, "Model id", "my-model", false),
            web_key: make(cx, "paste your key", "", true),
            searx: make(cx, "http://localhost:8888", "http://localhost:8888", false),
            folder: make(cx, "~/code/another", "", false),
            proj_name,
            proj_folder: make(cx, "~/code/my-project", "~/code/my-project", false),
            agents,
            nw_kind: None,
            nw_busy: false,
            nw_name: make(cx, "Workspace name", "", false),
            nw_email: make(cx, "you@example.com", "", false),
            nw_issuer: make(cx, "https://login.example.com/realms/org", "", false),
        }
    }
}

fn text(cx: &gpui::App, i: &Entity<TextInput>) -> String {
    i.read(cx).content().to_string()
}

/// Runs a request on a thread and hands the reply (or `{ok:false, error}`) to the window as `Msg::Onb`.
fn send(api: Api, kind: &'static str, method: &'static str, path: String, body: Option<Value>, tx: Sender<Msg>) {
    crate::spawn(move || {
        let v = api.request(method, &path, body.as_ref()).unwrap_or_else(|e| json!({ "ok": false, "found": false, "error": e.to_string(), "sessions": [] }));
        let _ = tx.send(Msg::Onb(kind, v));
    });
}

// ---- small widgets ----

fn h1(t: &str) -> Div {
    div().text_size(px(30.)).font_weight(FontWeight::BOLD).child(SharedString::from(t.to_string()))
}
fn lead(t: impl Into<SharedString>) -> Div {
    div().text_color(rgb(TEXT2)).child(t.into())
}
pub(crate) fn small(t: impl Into<SharedString>) -> Div {
    div().text_sm().text_color(rgb(MUTED)).child(t.into())
}
pub(crate) fn labeled(label: &str, input: &Entity<TextInput>) -> Div {
    div().flex().flex_col().gap_1().flex_1().child(small(label.to_string())).child(div().px_2().py_1().rounded_md().bg(rgb(BG)).child(input.clone()))
}
pub(crate) fn panel() -> Div {
    div().flex().flex_col().gap_3().p_4().rounded_lg().bg(rgb(PANEL))
}
fn step_col() -> Div {
    div().flex().flex_col().gap_3()
}

pub(crate) fn opt(cx: &mut Context<AppView>, id: String, title: &str, note: &str, on: bool, enabled: bool, f: impl Fn(&mut AppView) + 'static) -> AnyElement {
    let d = div()
        .id(SharedString::from(id))
        .flex()
        .flex_col()
        .gap_1()
        .flex_1()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(rgb(if on { ACCENT } else { RAISED_OFF }))
        .bg(rgb(if on { USER_BUBBLE } else { PANEL }))
        .child(div().font_weight(FontWeight::BOLD).child(SharedString::from(title.to_string())))
        .child(small(note.to_string()));
    if enabled {
        d.cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                f(this);
                cx.notify();
            }))
            .into_any_element()
    } else {
        d.opacity(0.55).into_any_element()
    }
}

pub(crate) fn btn(cx: &mut Context<AppView>, id: &str, label: &str, bg: u32, enabled: bool, f: impl Fn(&mut AppView, &mut Context<AppView>) + 'static) -> AnyElement {
    let b = ui::button(id.to_string(), label.to_string(), bg);
    if enabled {
        b.on_click(cx.listener(move |this, _, _, cx| {
            f(this, cx);
            cx.notify();
        }))
        .into_any_element()
    } else {
        b.opacity(0.5).into_any_element()
    }
}

fn switch_row(cx: &mut Context<AppView>, id: &str, label: &str, on: bool, f: impl Fn(&mut AppView) + 'static) -> Div {
    div().flex().items_center().justify_between().child(div().child(SharedString::from(label.to_string()))).child(ui::switch(id.to_string(), on, false).on_click(cx.listener(move |this, _, _, cx| {
        f(this);
        cx.notify();
    })))
}

impl AppView {
    fn oc_has(&self) -> bool {
        if self.onb.remote { self.onb.oc_conn.ok() } else { self.onb.detect.as_ref().is_some_and(|d| d.found) }
    }

    fn claude_sub(&self) -> Option<&Subscription> {
        self.onb.subs.as_ref()?.iter().find(|s| s.id == "claude")
    }

    fn claude_ready(&self) -> bool {
        self.claude_sub().is_some_and(|s| s.installed && s.signed_in)
    }

    fn prov_kind(&self) -> String {
        ob::provider_kind(self.onb.prov.as_deref(), self.oc_has(), self.onb.sessions.as_ref().map_or(0, |s| s.len()))
    }

    fn found_sessions(&self) -> Vec<Session> {
        if self.oc_has() { self.onb.sessions.clone().unwrap_or_default() } else { Vec::new() }
    }

    /// Starts what the current step shows: the session scan and the subscription check.
    pub fn onb_enter(&mut self, cx: &mut Context<Self>) {
        let Some(api) = self.api.clone() else { return };
        match ob::STEPS[self.onb.step].0 {
            "sessions" if self.onb.sessions.is_none() && self.oc_has() && !self.onb.scan_busy => {
                self.onb.scan_busy = true;
                let body = ob::scan_body(self.onb.remote, &text(cx, &self.onb.binary), &text(cx, &self.onb.oc_url), &text(cx, &self.onb.oc_user), &text(cx, &self.onb.oc_pass));
                send(api, "sessions", "POST", ob::SCAN_PATH.into(), Some(body), self.tx.clone());
            }
            "agents" if self.onb.subs.is_none() && !self.onb.subs_busy => {
                self.onb.subs_busy = true;
                send(api, "subs", "GET", ob::SUBS_PATH.into(), None, self.tx.clone())
            }
            _ => {}
        }
    }

    /// First time the setup shows: look for opencode and Open Design.
    pub fn onb_start(&mut self, cx: &mut Context<Self>) {
        if self.onb.started || self.api.is_none() || self.extra.app.onboarded {
            return;
        }
        self.onb.started = true;
        self.onb_recheck(cx);
        if let Some(api) = self.api.clone() {
            send(api, "od", "GET", ob::OD_STATUS_PATH.into(), None, self.tx.clone());
        }
    }

    fn onb_recheck(&mut self, cx: &mut Context<Self>) {
        self.onb.detect = None;
        if let Some(api) = self.api.clone() {
            send(api, "detect", "GET", ob::detect_path(&text(cx, &self.onb.binary)), None, self.tx.clone());
        }
    }

    fn onb_go(&mut self, step: usize, cx: &mut Context<Self>) {
        self.onb.step = step.min(ob::STEPS.len() - 1);
        self.onb.step_max = self.onb.step_max.max(self.onb.step);
        self.onb_enter(cx);
    }

    /// Re-polls Open Design while it installs.
    pub fn onb_poll(&mut self) {
        if self.onb.od.as_ref().is_some_and(|o| o.installing) && self.onb.od_polled.is_none_or(|t| t.elapsed() > Duration::from_millis(1000)) {
            self.onb.od_polled = Some(Instant::now());
            if let Some(api) = self.api.clone() {
                send(api, "od", "GET", ob::OD_STATUS_PATH.into(), None, self.tx.clone());
            }
        }
    }

    pub fn onb_reply(&mut self, kind: &'static str, v: Value) {
        match kind {
            "detect" => self.onb.detect = serde_json::from_value(v).ok().or(Some(Detect::default())),
            "od" => {
                self.onb.od = Some(serde_json::from_value(v.clone()).unwrap_or_else(|_| OdStatus { error: Some("Status unavailable".into()), ..OdStatus::default() }));
                if let Some(e) = v["error"].as_str().filter(|_| v["ok"].as_bool() == Some(false)) {
                    self.onb.od.as_mut().map(|o| o.error = Some(e.to_string()));
                }
            }
            "connect" => {
                self.onb.oc_conn = Check::done(ob::remote_result(&v));
                self.onb.sessions = None;
            }
            "sessions" => {
                self.onb.scan_busy = false;
                let (list, err) = ob::parse_scan(&v);
                self.onb.sessions = Some(list);
                self.onb.scan_err = err;
            }
            "subs" => {
                self.onb.subs_busy = false;
                match serde_json::from_value::<Vec<Subscription>>(v.clone()) {
                    Ok(rows) => {
                        self.onb.subs_on = rows.iter().filter(|r| r.installed && r.signed_in).map(|r| r.id.clone()).collect();
                        self.onb.subs = Some(rows);
                        self.onb.subs_err.clear();
                    }
                    Err(_) => {
                        self.onb.subs = Some(Vec::new());
                        self.onb.subs_err = v["error"].as_str().unwrap_or("Could not check subscriptions").to_string();
                    }
                }
            }
            "prov-test" => {
                let kind = self.prov_kind();
                self.onb.prov_test = Check::done(ob::provider_result(&v, &kind, self.onb.ollama.len()));
            }
            "web-test" => self.onb.web_test = Check::done(ob::web_result(&v)),
            "finished" => {
                self.onb.busy = false;
                self.model.toast = Some(ob::finish_toast(&v));
                self.view = View::Chat;
                if let Some(t) = ob::failed_toast(&v) {
                    self.model.toast = Some(t);
                }
                if let Some(api) = self.api.clone() {
                    load_state(api, self.tx.clone());
                }
            }
            "demo" => {
                self.onb.busy = false;
                self.model.toast = Some("Demo data loaded".into());
                self.view = View::Chat;
                if let Some(api) = self.api.clone() {
                    load_state(api, self.tx.clone());
                }
            }
            "failed" => self.onb.busy = false,
            "ws-created" => {
                self.onb.nw_busy = false;
                self.onb.nw_kind = None;
                self.view = View::Chat;
                self.model.toast = v["toast"].as_str().map(str::to_string);
            }
            "ws-failed" => self.onb.nw_busy = false,
            "toast" => self.model.toast = v["text"].as_str().map(str::to_string),
            _ => {}
        }
    }

    fn onb_draft(&self, cx: &gpui::App) -> Draft {
        let o = &self.onb;
        let found = self.found_sessions();
        let a = &self.extra.app;
        Draft {
            remote: o.remote,
            binary: text(cx, &o.binary),
            url: text(cx, &o.oc_url),
            user: text(cx, &o.oc_user),
            pass: text(cx, &o.oc_pass),
            ws_name: text(cx, &o.ws_name),
            accent: o.accent.into(),
            provider: self.prov_kind(),
            key: text(cx, &o.key),
            endpoint: text(cx, &o.url),
            model: text(cx, &o.model),
            ollama: o.ollama.clone(),
            web_mode: o.web_mode.into(),
            web_provider: o.web_prov.into(),
            web_key: text(cx, &o.web_key),
            searx: text(cx, &o.searx),
            web_fetch: o.web_fetch,
            web_ask: o.web_ask,
            memory: o.mem.into(),
            folders: o.folders.clone(),
            watch: o.watch,
            proj_name: text(cx, &o.proj_name),
            proj_folder: text(cx, &o.proj_folder),
            agents: text(cx, &o.agents),
            import_ids: o.sel.ids(&found, true),
            skipped_ids: o.sel.ids(&found, false),
            sync: o.sync,
            theme: if a.theme.is_empty() { "claude".into() } else { a.theme.clone() },
            reduce_motion: a.reduce_motion,
            follow_system: a.follow_system,
            subscriptions: {
                let mut v: Vec<String> = o.subs_on.iter().cloned().collect();
                v.sort();
                v
            },
        }
    }

    fn onb_tested(&self) -> bool {
        let k = self.prov_kind();
        self.onb.prov_test.ok() || k == "existing" || k == "opencode" || (k == "claude" && self.claude_ready())
    }

    fn onb_next(&mut self, cx: &mut Context<Self>) {
        if self.onb.step == ob::STEPS.len() - 1 {
            let Some(api) = self.api.clone() else { return };
            let body = ob::finish_body(&self.onb_draft(cx));
            self.onb.busy = true;
            let tx = self.tx.clone();
            crate::spawn(move || {
                let _ = tx.send(match api.post(ob::FINISH_PATH, Some(&body)) {
                    Ok(v) => Msg::Onb("finished", v),
                    Err(e) => {
                        let _ = tx.send(Msg::Error(e.to_string()));
                        Msg::Onb("failed", Value::Null)
                    }
                });
            });
        } else {
            self.onb_go(self.onb.step + 1, cx);
        }
    }

    /// Opens the New workspace dialog (`kind` is team, personal or org).
    pub fn open_new_workspace(&mut self, kind: &'static str, cx: &mut Context<Self>) {
        self.onb.nw_kind = Some(kind);
        for i in [&self.onb.nw_name, &self.onb.nw_email, &self.onb.nw_issuer] {
            i.update(cx, |i, cx| i.set_content("", cx));
        }
    }

    fn create_workspace(&mut self, cx: &mut Context<Self>) {
        let Some(kind) = self.onb.nw_kind else { return };
        let Some((path, body)) = spaces::create_request(kind, &text(cx, &self.onb.nw_name), &text(cx, &self.onb.nw_email), &text(cx, &self.onb.nw_issuer)) else {
            self.model.toast = Some("Name the workspace".into());
            return;
        };
        let Some(api) = self.api.clone() else { return };
        self.onb.nw_busy = true;
        let name = body["name"].as_str().unwrap_or("").to_string();
        let tx = self.tx.clone();
        crate::spawn(move || match api.post(path, Some(&body)) {
            Ok(v) => {
                let _ = tx.send(Msg::Onb("ws-created", json!({ "toast": spaces::created_toast(kind, &name) })));
                if let Some(id) = v["id"].as_str() {
                    activate(api, id.to_string(), tx);
                }
            }
            Err(e) => {
                let _ = tx.send(Msg::Error(e.to_string()));
                let _ = tx.send(Msg::Onb("ws-failed", Value::Null));
            }
        });
    }

    /// Delete (or leave) the active workspace, then switch to another one.
    pub fn remove_workspace(&mut self) {
        let (Some(api), Some(ws)) = (self.api.clone(), self.model.active_ws.clone()) else { return };
        let next = spaces::next_workspace(&self.model.workspaces, &ws);
        let name = self.model.snapshot.workspace.name.clone();
        let tx = self.tx.clone();
        crate::spawn(move || match api.del(&spaces::remove_path(&ws)) {
            Ok(_) => {
                let _ = tx.send(Msg::Onb("toast", json!({ "text": spaces::removed_toast(&name) })));
                match next {
                    Some(id) => activate(api, id, tx),
                    None => load_state(api, tx),
                }
            }
            Err(e) => {
                let _ = tx.send(Msg::Error(e.to_string()));
            }
        });
    }

    /// Downloads the active workspace's archive into Downloads.
    pub fn export_workspace(&mut self) {
        let (Some(api), Some(ws)) = (self.api.clone(), self.model.active_ws.clone()) else { return };
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = spaces::download_dir(&home);
        self.model.toast = Some("Exporting…".into());
        let tx = self.tx.clone();
        crate::spawn(move || {
            let _ = tx.send(match api.download_to(&spaces::export_path(&ws), &dir) {
                Ok(p) => Msg::Onb("toast", json!({ "text": format!("Exported to {}", p.display()) })),
                Err(e) => Msg::Error(format!("Export failed: {e}")),
            });
        });
    }

    fn set_app_setting(&mut self, body: Value) {
        if let Some(api) = self.api.clone() {
            mutate_state(api, "PATCH", wsx::APP_SETTINGS_PATH.into(), Some(body), self.tx.clone());
        }
    }

    // ---- views ----

    pub fn onboarding_view(&mut self, cx: &mut Context<Self>) -> Div {
        let step = self.onb.step;
        let key = ob::STEPS[step].0;
        let found = self.found_sessions();
        let n_sel = self.onb.sel.count(&found);
        let ws_name = text(cx, &self.onb.ws_name);
        let gate = Gate { remote: self.onb.remote, oc_has: self.oc_has(), od_ready: self.onb.od.as_ref().is_some_and(|o| o.installed) || self.onb.od_later, ws_name: ws_name.clone(), proj_name: text(cx, &self.onb.proj_name), proj_folder: text(cx, &self.onb.proj_folder) };
        let hint = ob::blocked(key, &gate);
        let next_label = ob::next_label(step, self.onb_tested(), found.len(), n_sel, self.onb.web_mode == "off");
        let busy = self.onb.busy;

        let steps = div().flex().flex_col().gap_1().children(ob::STEPS.iter().enumerate().map(|(k, st)| {
            let reachable = k <= self.onb.step_max;
            let row = div()
                .id(SharedString::from(format!("ob-step-{k}")))
                .flex()
                .items_center()
                .gap_3()
                .px_2()
                .py_1()
                .rounded_md()
                .child(
                    div()
                        .size(px(22.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .border_2()
                        .border_color(rgb(if k <= step { ACCENT } else { 0x3c3c3c }))
                        .bg(rgb(if k < step { ACCENT } else { PANEL }))
                        .text_size(px(11.))
                        .font_weight(FontWeight::BOLD)
                        .child(SharedString::from(if k < step { "✓".to_string() } else { (k + 1).to_string() })),
                )
                .child(div().flex().flex_col().child(div().text_sm().text_color(rgb(if k == step { TEXT } else { MUTED })).child(SharedString::from(st.1.to_string()))).child(div().text_size(px(11.)).text_color(rgb(MUTED)).child(SharedString::from(st.2.to_string()))));
            if reachable {
                row.cursor_pointer().on_click(cx.listener(move |this, _, _, cx| {
                    this.onb_go(k, cx);
                    cx.notify();
                }))
            } else {
                row.opacity(0.6)
            }
        }));
        let aside = div()
            .flex()
            .flex_col()
            .gap_4()
            .w(px(280.))
            .flex_none()
            .p(px(22.))
            .bg(rgb(RAIL))
            .child(div().font_family("monospace").font_weight(FontWeight::BOLD).text_size(px(16.)).child("workspaceai"))
            .child(steps)
            .child(div().mt_auto().text_sm().text_color(rgb(MUTED)).child("Everything here can be changed later in Settings."))
            .child(btn(cx, "ob-demo", "Skip setup · use demo data", RAISED_OFF, !busy, |this, _| {
                if let Some(api) = this.api.clone() {
                    this.onb.busy = true;
                    let tx = this.tx.clone();
                    crate::spawn(move || {
                        let _ = tx.send(match api.post(ob::DEMO_PATH, None) {
                            Ok(v) => Msg::Onb("demo", v),
                            Err(e) => {
                                let _ = tx.send(Msg::Error(e.to_string()));
                                Msg::Onb("failed", Value::Null)
                            }
                        });
                    });
                }
            }));

        let body = match key {
            "welcome" => self.step_welcome(cx),
            "engine" => self.step_engine(cx),
            "agents" => self.step_agents(cx, &ws_name),
            "sessions" => self.step_sessions(cx, &found, n_sel),
            "workspace" => self.step_workspace(cx, &ws_name),
            "model" => self.step_model(cx, &ws_name),
            "web" => self.step_web(cx, &ws_name),
            "memory" => self.step_memory(cx, &ws_name),
            "project" => self.step_project(cx),
            _ => self.step_look(cx, n_sel),
        };

        let progress = div().flex_1().h(px(4.)).rounded_full().bg(rgb(USER_BUBBLE)).child(div().h(px(4.)).rounded_full().bg(rgb(ACCENT)).w(gpui::relative((step + 1) as f32 / ob::STEPS.len() as f32)));
        let footer = div()
            .flex()
            .items_center()
            .gap_3()
            .pt_2()
            .child(progress)
            .child(small(hint.unwrap_or("")))
            .child(btn(cx, "ob-back", "Back", RAISED_OFF, step > 0 && !busy, |this, cx| this.onb_go(this.onb.step.saturating_sub(1), cx)))
            .child(btn(cx, "ob-next", if busy { "Setting up…" } else { next_label }, ACCENT, hint.is_none() && !busy, |this, cx| this.onb_next(cx)));

        let main = div().id("ob-main").flex().flex_col().flex_1().min_w_0().h_full().px(px(40.)).py(px(28.)).gap_4().overflow_y_scroll().child(div().flex().flex_col().gap_4().flex_1().child(body)).child(footer);
        div().size_full().flex().items_center().justify_center().bg(rgb(0x0e0e10)).p_6().child(div().flex().w(px(1080.)).max_w_full().h(px(760.)).max_h_full().rounded_lg().overflow_hidden().bg(rgb(BG)).child(aside).child(main))
    }

    fn step_welcome(&mut self, cx: &mut Context<Self>) -> Div {
        step_col()
            .pt(px(30.))
            .child(small("WELCOME"))
            .child(div().text_size(px(46.)).line_height(px(50.)).font_weight(FontWeight::BOLD).child("Your workspaces,\nyour models,\nyour machine."))
            .child(lead("WorkspaceAI keeps personal and work completely apart. Chats run on opencode, artifacts are made with Open Design, and everything stays on this computer unless you share it."))
            .child(div().flex().gap_3().children(ob::CARDS.iter().enumerate().map(|(i, c)| opt(cx, format!("ob-card-{i}"), c.0, c.1, self.onb.tour == i + 1, true, move |this| this.onb.tour = i + 1))))
            .child(small(ob::TOUR[self.onb.tour]))
    }

    fn step_engine(&mut self, cx: &mut Context<Self>) -> Div {
        let remote = self.onb.remote;
        let mut s = step_col()
            .child(h1("Connect opencode"))
            .child(lead("Every chat here is an opencode session. Let WorkspaceAI start opencode for this workspace, or connect to a server you already run — then the same sessions show here and in the opencode TUI."))
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(opt(cx, "ob-oc-managed".into(), "Start it for me", "WorkspaceAI runs opencode serve per workspace on this computer", !remote, true, |this| {
                        this.onb.remote = false;
                        this.onb.sessions = None;
                    }))
                    .child(opt(cx, "ob-oc-remote".into(), "Connect to a running server", "Local or remote opencode serve, with its sessions and config", remote, true, |this| {
                        this.onb.remote = true;
                        this.onb.sessions = None;
                    })),
            );
        if !remote {
            let d = self.onb.detect.clone();
            let dot = match &d {
                None => WARN,
                Some(d) if d.found => SUCCESS,
                _ => ERROR,
            };
            s = s.child(
                panel()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(div().size(px(10.)).rounded_full().bg(rgb(dot)))
                            .child(div().flex_1().child(SharedString::from(format!("opencode · {}", ob::detect_text(d.as_ref())))))
                            .child(btn(cx, "ob-recheck", "Check again", RAISED_OFF, true, |this, cx| this.onb_recheck(cx))),
                    )
                    .child(div().flex().child(labeled("Binary", &self.onb.binary)))
                    .child(small("Bound to 127.0.0.1 with a random password per launch. Attach the TUI any time from a chat’s menu."))
                    .when(d.as_ref().is_some_and(|d| !d.found), |p| p.child(div().text_sm().text_color(rgb(ERROR)).child("Install opencode (curl -fsSL https://opencode.ai/install | bash) and press Check again."))),
            );
        } else {
            let conn = self.onb.oc_conn.clone();
            s = s.child(
                panel()
                    .child(div().flex().gap_3().child(labeled("Server URL", &self.onb.oc_url)).child(labeled("Username", &self.onb.oc_user)).child(labeled("Password", &self.onb.oc_pass)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(btn(cx, "ob-connect", if conn == Check::Busy { "Connecting…" } else if conn.ok() { "Reconnect" } else { "Connect" }, ACCENT, conn != Check::Busy, |this, cx| {
                                this.onb.oc_conn = Check::Busy;
                                this.onb.sessions = None;
                                if let Some(api) = this.api.clone() {
                                    let body = ob::remote_body(&text(cx, &this.onb.oc_url), &text(cx, &this.onb.oc_user), &text(cx, &this.onb.oc_pass));
                                    send(api, "connect", "POST", ob::TEST_REMOTE_PATH.into(), Some(body), this.tx.clone());
                                }
                            }))
                            .child(conn.line("Connect to continue.")),
                    )
                    .child(small("The password goes to your keychain and is sent as basic auth. For a remote machine use HTTPS or an SSH tunnel.")),
            );
        }
        s.child(self.od_card(cx))
    }

    fn od_card(&mut self, cx: &mut Context<Self>) -> Div {
        let st = self.onb.od.clone();
        let installed = st.as_ref().is_some_and(|s| s.installed);
        let installing = st.as_ref().is_some_and(|s| s.installing);
        let later = self.onb.od_later;
        let pct = st.as_ref().map_or(0, |s| s.pct());
        let dot = if installed { SUCCESS } else if installing { WARN } else { 0x606060 };
        panel()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().size(px(10.)).rounded_full().bg(rgb(dot)))
                    .child(div().flex_1().child(SharedString::from(format!("Open Design · {}", st.as_ref().map_or("checking…".to_string(), |s| s.text(later))))))
                    .child(btn(cx, "ob-od-install", if installed { "Installed ✓" } else if installing { "Installing…" } else { "Install" }, ACCENT, st.is_some() && !installed && !installing, |this, _| {
                        if let Some(api) = this.api.clone() {
                            let tx = this.tx.clone();
                            crate::spawn(move || {
                                if let Err(e) = api.post(wsx::OD_INSTALL_PATH, None) {
                                    let _ = tx.send(Msg::Error(e.to_string()));
                                }
                                send(api, "od", "GET", ob::OD_STATUS_PATH.into(), None, tx);
                            });
                        }
                    })),
            )
            .child(div().h(px(6.)).rounded_full().bg(rgb(USER_BUBBLE)).child(div().h(px(6.)).rounded_full().bg(rgb(ACCENT)).w(gpui::relative(pct as f32 / 100.))))
            .child(small("The artifact engine: every doc, deck, prototype and sheet is saved, versioned and linted here, with design systems and PDF/PowerPoint export."))
            .when(!installed, |p| p.child(btn(cx, "ob-od-later", if later { "Install now instead" } else { "Install later" }, RAISED_OFF, true, |this, _| this.onb.od_later = !this.onb.od_later)))
            .children(st.as_ref().and_then(|s| s.error.clone()).map(|e| div().text_sm().text_color(rgb(ERROR)).child(SharedString::from(e))))
            .children(st.as_ref().filter(|s| s.installing).and_then(|s| s.log.clone()).map(|l| div().p_2().max_h(px(90.)).overflow_hidden().rounded_md().bg(rgb(BG)).font_family("monospace").text_size(px(11.)).child(SharedString::from(l))))
    }

    fn step_agents(&mut self, cx: &mut Context<Self>, ws_name: &str) -> Div {
        let name = if ws_name.trim().is_empty() { "this workspace" } else { ws_name.trim() };
        let mut s = step_col().child(h1("Connect your subscriptions")).child(lead(format!("Use the accounts you already pay for. WorkspaceAI copies the login on this computer into {name}’s own folder — no token is written to a config file, and another workspace never sees this history.")));
        if self.onb.subs.is_none() {
            s = s.child(small("Looking for opencode, Claude Code and Grok…"));
        }
        if !self.onb.subs_err.is_empty() {
            s = s.child(div().text_sm().text_color(rgb(ERROR)).child(SharedString::from(self.onb.subs_err.clone())));
        }
        s = s.child(div().flex().justify_end().child(btn(cx, "ob-subs-again", "Check again", RAISED_OFF, true, |this, cx| {
            this.onb.subs = None;
            this.onb_enter(cx);
        })));
        for sub in self.onb.subs.clone().unwrap_or_default() {
            let can = sub.installed && sub.signed_in;
            let on = self.onb.subs_on.contains(&sub.id);
            let id = sub.id.clone();
            let title = format!("{}   ·   {}   ·   {}", sub.name, if sub.installed { sub.version.clone().unwrap_or_else(|| "installed".into()) } else { "not installed".into() }, if on { "✓ connect" } else if can { "connect" } else { "unavailable" });
            let note = format!("{}{}", sub.detail, sub.account.as_ref().map(|a| format!(" · {a}")).unwrap_or_default());
            s = s.child(div().flex().child(opt(cx, format!("ob-sub-{}", sub.id), &title, &note, on, can, move |this| {
                if !this.onb.subs_on.remove(&id) {
                    this.onb.subs_on.insert(id.clone());
                }
            })));
        }
        s.child(small("Optional. You can connect or disconnect any of them later in Settings › Agents. If a subscription is not signed in yet, run its login command on this computer and press Continue again."))
    }

    fn step_sessions(&mut self, cx: &mut Context<Self>, found: &[Session], n_sel: usize) -> Div {
        let groups = ob::group_sessions(found);
        let o = &self.onb;
        let mut s = step_col().child(h1("Bring your opencode chats")).child(lead(ob::sessions_lead(self.oc_has(), o.remote, &text(cx, &o.oc_url), o.sessions.is_some(), found.len(), groups.len())));
        if !o.scan_err.is_empty() {
            s = s.child(div().text_sm().text_color(rgb(ERROR)).child(SharedString::from(o.scan_err.clone())));
        }
        if o.sessions.is_some() && found.is_empty() && self.oc_has() {
            s = s.child(panel().child(div().font_weight(FontWeight::BOLD).child("No earlier sessions")).child(small("Chats you start here are new opencode sessions; you can open any of them in the TUI later.")));
        }
        let now = chrono::Utc::now().timestamp_millis();
        let mut list = div().id("ob-sessions").flex().flex_col().gap_2().max_h(px(330.)).overflow_y_scroll();
        for (gi, g) in groups.iter().enumerate() {
            let on = o.sel.dir_on(&g.dir);
            let dir = g.dir.clone();
            let mut card = panel().p_3().gap_1().child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().font_family("monospace").text_sm().font_weight(FontWeight::BOLD).child(SharedString::from(g.dir.clone())))
                    .child(div().flex_1().text_sm().text_color(rgb(MUTED)).child(SharedString::from(format!("→ {}", ob::target_text(&g.dir, g.parent.as_deref(), on)))))
                    .child(ui::switch(format!("ob-dir-{gi}"), on, false).on_click(cx.listener(move |this, _, _, cx| {
                        this.onb.sel.toggle_dir(&dir);
                        cx.notify();
                    }))),
            );
            for (si, x) in g.items.iter().enumerate() {
                let io = o.sel.is_on(x);
                let id = x.id.clone();
                let row = div()
                    .id(SharedString::from(format!("ob-sess-{gi}-{si}")))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .child(div().w(px(16.)).text_color(rgb(ACCENT)).child(if io { "✓" } else { "" }))
                    .child(div().flex_1().min_w_0().overflow_hidden().child(SharedString::from(x.title.clone())))
                    .child(div().font_family("monospace").text_size(px(11.)).text_color(rgb(MUTED)).child(SharedString::from(format!("{}… · {}", x.id.chars().take(12).collect::<String>(), ob::time_ago(x.updated, now)))));
                card = card.child(if on {
                    row.cursor_pointer().hover(|s| s.bg(rgb(USER_BUBBLE))).on_click(cx.listener(move |this, _, _, cx| {
                        this.onb.sel.toggle_session(&id);
                        cx.notify();
                    })).into_any_element()
                } else {
                    row.opacity(0.5).into_any_element()
                });
            }
            list = list.child(card);
        }
        s = s.child(list);
        if !found.is_empty() {
            let all = found.to_vec();
            s = s
                .child(switch_row(cx, "ob-sync", "Keep in sync both ways — sessions started in the TUI appear here; chats started here appear in the TUI.", o.sync, |this| this.onb.sync = !this.onb.sync))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(div().font_weight(FontWeight::BOLD).child(SharedString::from(ob::import_count_text(n_sel))))
                        .child(btn(cx, "ob-sel-all", "Select all", RAISED_OFF, true, |this, _| this.onb.sel.select_all()))
                        .child(btn(cx, "ob-sel-none", "Start fresh", RAISED_OFF, true, move |this, _| this.onb.sel.start_fresh(&all))),
                );
        }
        s
    }

    fn step_workspace(&mut self, cx: &mut Context<Self>, ws_name: &str) -> Div {
        let acc = self.onb.accent;
        let hex = settings_accent_hex(acc);
        step_col()
            .child(h1("Your personal workspace"))
            .child(lead("Your own keys, accounts and files. Nothing here is visible from a work workspace. You can join an organization later from + in the rail."))
            .child(div().w(px(420.)).flex().child(labeled("Name", &self.onb.ws_name)))
            .child(div().font_weight(FontWeight::MEDIUM).child("Accent"))
            .child(div().flex().gap_3().children(wsx::ACCENTS.iter().enumerate().map(|(i, (id, label))| {
                let (id, _label): (&'static str, &str) = (id, label);
                let on = acc == id;
                div().id(SharedString::from(format!("ob-accent-{i}"))).size(px(34.)).rounded_full().cursor_pointer().bg(rgb(settings_accent_hex(id))).border_2().border_color(rgb(if on { TEXT } else { BG })).on_click(cx.listener(move |this, _, _, cx| {
                    this.onb.accent = id;
                    cx.notify();
                }))
            })))
            .child(
                panel().flex_row().items_center().max_w(px(520.)).child(div().size(px(40.)).rounded_lg().bg(rgb(USER_BUBBLE)).flex().items_center().justify_center().font_weight(FontWeight::BOLD).text_color(rgb(hex)).child(SharedString::from(ob::initials_of(if ws_name.is_empty() { "P" } else { ws_name })))).child(
                    div().flex().flex_col().child(div().font_weight(FontWeight::BOLD).child(SharedString::from(if ws_name.trim().is_empty() { "Personal".to_string() } else { ws_name.trim().to_string() }))).child(div().font_family("monospace").text_sm().text_color(rgb(MUTED)).child(SharedString::from(format!("~/.harness/ws/{}", ob::slugify(ws_name, "personal"))))),
                ),
            )
    }

    fn step_model(&mut self, cx: &mut Context<Self>, ws_name: &str) -> Div {
        let k = self.prov_kind();
        let name = if ws_name.trim().is_empty() { "Personal" } else { ws_name.trim() };
        let installed = self.claude_sub().is_some_and(|s| s.installed);
        let ready = self.claude_ready();
        let account = self.claude_sub().and_then(|s| s.account.clone());
        let mut kinds: Vec<&'static str> = Vec::new();
        if self.claude_sub().is_some() {
            kinds.push("claude");
        }
        if self.oc_has() {
            kinds.push("existing");
        }
        kinds.extend(["opencode", "anthropic", "openrouter", "ollama", "custom"]);
        let mut grid = div().flex().flex_wrap().gap_3();
        for kind in kinds {
            let t = ob::provider_title(kind);
            let note = ob::provider_note(kind, ready, installed);
            grid = grid.child(div().w(px(220.)).flex().child(opt(cx, format!("ob-prov-{kind}"), t, note, k == kind, kind != "claude" || ready, move |this| {
                this.onb.prov = Some(kind.into());
                this.onb.prov_test = Check::Idle;
            })));
        }
        let mut card = panel();
        if matches!(k.as_str(), "anthropic" | "openrouter" | "custom") {
            card = card.child(div().flex().child(labeled(&if k == "custom" { "API key (optional)".to_string() } else { format!("{} API key", if k == "anthropic" { "Anthropic" } else { "OpenRouter" }) }, &self.onb.key)));
        }
        if k == "custom" {
            card = card.child(div().flex().gap_3().child(labeled("Base URL", &self.onb.url)).child(labeled("Model id", &self.onb.model)));
        }
        if k == "ollama" {
            card = card.child(div().child("Ollama answers on localhost:11434. Pick the models to use:")).child(div().flex().flex_wrap().gap_2().children(ob::OLLAMA_MODELS.iter().map(|m| {
                let on = self.onb.ollama.iter().any(|x| x == m);
                let m: &'static str = m;
                ui::button(format!("ob-oll-{m}"), format!("{} {m}", if on { "✓" } else { "+" }), if on { ACCENT } else { RAISED_OFF }).on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(i) = this.onb.ollama.iter().position(|x| x == m) {
                        this.onb.ollama.remove(i);
                    } else {
                        this.onb.ollama.push(m.into());
                    }
                    cx.notify();
                }))
            })));
        }
        card = card.child(div().text_sm().child(match k.as_str() {
            "existing" => "Uses the providers your opencode already has — keys stay where opencode keeps them.".to_string(),
            "claude" => format!("Chats run in Claude Code on your Claude plan{}; its models (Opus, Sonnet, Haiku) show in the chat picker. The login is copied into this workspace only. OpenCode Zen’s free models stay available for opencode chats, routines and the artifact pipeline.", account.map(|a| format!(" ({a})")).unwrap_or_default()),
            "opencode" => "OpenCode Zen’s free models answer without a key. Add your own providers any time in Settings › Model providers.".to_string(),
            _ => String::new(),
        }));
        let tested = self.onb_tested();
        let test = self.onb.prov_test.clone();
        card = card.child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .when(k != "claude", |d| {
                    d.child(btn(cx, "ob-prov-test", if test == Check::Busy { "Testing…" } else { "Test connection" }, RAISED_OFF, test != Check::Busy, |this, cx| {
                        this.onb.prov_test = Check::Busy;
                        if let Some(api) = this.api.clone() {
                            let body = ob::provider_body(&this.prov_kind(), &text(cx, &this.onb.key), &text(cx, &this.onb.url), &text(cx, &this.onb.model));
                            send(api, "prov-test", "POST", ob::TEST_PROVIDER_PATH.into(), Some(body), this.tx.clone());
                        }
                    }))
                })
                .child(test.line(if tested { "Ready to use." } else { "Test before continuing, or set it up later." })),
        );
        step_col().child(h1("Connect a model")).child(lead(format!("For {name}. Keys go to your keychain; config files only hold a reference."))).child(grid).child(card)
    }

    fn step_web(&mut self, cx: &mut Context<Self>, ws_name: &str) -> Div {
        let o = &self.onb;
        let name = if ws_name.trim().is_empty() { "Personal" } else { ws_name.trim() };
        let mode = o.web_mode;
        let mut modes = div().flex().gap_3();
        for (m, t, n) in ob::WEB_MODES {
            modes = modes.child(opt(cx, format!("ob-web-{m}"), t, n, mode == m, true, move |this| {
                this.onb.web_mode = m;
                this.onb.web_test = Check::Idle;
            }));
        }
        let mut card = panel();
        if mode == "mcp" {
            card = card.child(div().flex().items_center().gap_2().child(div().font_weight(FontWeight::MEDIUM).child("Provider")).children(wsx::WEB_PROVIDERS.iter().map(|p| {
                let p: &'static str = p;
                ui::button(format!("ob-wp-{p}"), p, if o.web_prov == p { ACCENT } else { RAISED_OFF }).on_click(cx.listener(move |this, _, _, cx| {
                    this.onb.web_prov = p;
                    this.onb.web_test = Check::Idle;
                    cx.notify();
                }))
            })));
            card = card.child(div().flex().child(labeled(&format!("{} API key", o.web_prov), &o.web_key)));
        }
        if mode == "searxng" {
            card = card.child(div().flex().child(labeled("SearXNG URL", &o.searx)));
        }
        card = card.child(switch_row(cx, "ob-web-fetch", "Read pages by URL (webfetch)", o.web_fetch, |this| this.onb.web_fetch = !this.onb.web_fetch)).child(switch_row(cx, "ob-web-ask", "Ask before each search", o.web_ask, |this| this.onb.web_ask = !this.onb.web_ask));
        if mode != "off" {
            let t = o.web_test.clone();
            card = card.child(div().flex().items_center().gap_3().child(btn(cx, "ob-web-test", if t == Check::Busy { "Checking…" } else { "Try it" }, RAISED_OFF, t != Check::Busy, |this, cx| {
                this.onb.web_test = Check::Busy;
                if let Some(api) = this.api.clone() {
                    let body = ob::web_body(this.onb.web_mode, this.onb.web_prov, &text(cx, &this.onb.web_key), &text(cx, &this.onb.searx));
                    send(api, "web-test", "POST", ob::TEST_WEB_PATH.into(), Some(body), this.tx.clone());
                }
            })).child(t.line("Optional — check it works.")));
        }
        step_col().child(h1("Web search")).child(lead(format!("Let the agent look things up for {name}. opencode has two tools: websearch finds pages and webfetch reads one by URL. You can turn search on or off per chat."))).child(modes).child(card)
    }

    fn step_memory(&mut self, cx: &mut Context<Self>, ws_name: &str) -> Div {
        let o = &self.onb;
        let name = if ws_name.trim().is_empty() { "Personal" } else { ws_name.trim() };
        let mut s = step_col().child(h1("How should it remember?")).child(lead(format!("For {name}. Each workspace picks its own memory.")));
        for (m, t, n) in ob::MEMORY {
            s = s.child(div().flex().child(opt(cx, format!("ob-mem-{m}"), t, n, o.mem == m, true, move |this| this.onb.mem = m)));
        }
        if matches!(o.mem, "graph" | "both") {
            let mut card = panel().gap_2().child(div().font_weight(FontWeight::BOLD).child("Folders Graphify should map"));
            for (i, f) in o.folders.iter().enumerate() {
                let f2 = f.clone();
                card = card.child(div().flex().items_center().gap_3().child(div().flex_1().font_family("monospace").text_sm().child(SharedString::from(f.clone()))).child(btn(cx, &format!("ob-folder-rm-{i}"), "Remove", RAISED_OFF, true, move |this, _| this.onb.folders.retain(|x| *x != f2))));
            }
            card = card
                .child(div().flex().items_center().gap_3().child(div().flex_1().px_2().py_1().rounded_md().bg(rgb(BG)).child(o.folder.clone())).child(btn(cx, "ob-folder-add", "Add folder", RAISED_OFF, true, |this, cx| {
                    let f = text(cx, &this.onb.folder).trim().to_string();
                    if !f.is_empty() && !this.onb.folders.contains(&f) {
                        this.onb.folders.push(f);
                    }
                    this.onb.folder.update(cx, |i, cx| i.set_content("", cx));
                })))
                .child(switch_row(cx, "ob-watch", "Keep the graph updated as code changes", o.watch, |this| this.onb.watch = !this.onb.watch));
            s = s.child(card);
        }
        s
    }

    fn step_project(&mut self, cx: &mut Context<Self>) -> Div {
        let o = &self.onb;
        step_col()
            .child(h1("Your first project"))
            .child(lead("Chats always belong to a project. A project is a folder; sub-projects are folders inside it. A General project is made for quick chats."))
            .child(div().flex().gap_3().child(labeled("Project name", &o.proj_name)).child(labeled("Folder", &o.proj_folder)))
            .child(div().flex().items_center().gap_2().child(div().font_weight(FontWeight::MEDIUM).child("AGENTS.md template")).children(ob::TEMPLATES.iter().map(|t| {
                let t: &'static str = t;
                ui::button(format!("ob-tpl-{t}"), t, if o.tpl == t { ACCENT } else { RAISED_OFF }).on_click(cx.listener(move |this, _, _, cx| {
                    this.onb.tpl = t;
                    this.onb.agents_dirty = false;
                    let name = text(cx, &this.onb.proj_name);
                    let body = ob::agents_template(t, if name.is_empty() { "My project" } else { &name });
                    this.onb.agents.update(cx, |i, cx| i.set_content(&body, cx));
                    cx.notify();
                }))
            })))
            .child(small("Project instructions · saved as AGENTS.md in the folder"))
            .child(div().id("ob-agents").h(px(180.)).overflow_y_scroll().px_3().py_2().rounded_md().bg(rgb(BG)).font_family("monospace").text_size(px(12.)).child(o.agents.clone()))
    }

    fn step_look(&mut self, cx: &mut Context<Self>, n_sel: usize) -> Div {
        let a = self.extra.app.clone();
        let theme = if a.theme.is_empty() { "claude".to_string() } else { a.theme.clone() };
        let mut themes = div().flex().gap_3();
        for (id, t, n) in ob::THEMES {
            themes = themes.child(opt(cx, format!("ob-theme-{id}"), t, n, theme == id, true, move |this| this.set_app_setting(wsx::theme_body(id))));
        }
        let draft = self.onb_draft(cx);
        let rows = ob::summary(&draft, &draft.url, self.onb.od.as_ref().is_some_and(|o| o.installed), n_sel, self.onb.prov_test.ok());
        step_col()
            .child(h1("Choose a look"))
            .child(themes)
            .child(switch_row(cx, "ob-follow", "Follow system light and dark", a.follow_system, move |this| this.set_app_setting(json!({ "followSystem": !a.follow_system }))))
            .child(switch_row(cx, "ob-reduce", "Reduce motion", self.extra.app.reduce_motion, |this| {
                let v = !this.extra.app.reduce_motion;
                this.set_app_setting(json!({ "reduceMotion": v }));
            }))
            .child(panel().gap_1().max_w(px(620.)).child(small("SUMMARY")).children(rows.into_iter().map(|(k, v)| div().flex().justify_between().gap_3().text_sm().child(div().text_color(rgb(MUTED)).child(k)).child(SharedString::from(v)))))
    }

    /// The New workspace / join an organization dialog, over everything.
    pub fn new_workspace_dialog(&mut self, cx: &mut Context<Self>) -> Option<gpui::Stateful<Div>> {
        let kind = self.onb.nw_kind?;
        let busy = self.onb.nw_busy;
        let mut types = div().flex().gap_2();
        for (id, label, note) in spaces::TYPES {
            types = types.child(opt(cx, format!("nw-type-{id}"), label, note, kind == id, true, move |this| this.onb.nw_kind = Some(id)));
        }
        let mut card = div()
            .id("nw-card")
            .w(px(560.))
            .max_w_full()
            .flex()
            .flex_col()
            .gap_3()
            .p(px(22.))
            .rounded_lg()
            .bg(rgb(PANEL))
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(div().text_size(px(22.)).font_weight(FontWeight::BOLD).child("New workspace"))
            .child(types)
            .child(div().flex().child(labeled("Name", &self.onb.nw_name)));
        if kind == "org" {
            card = card.child(small(spaces::ORG_NOTE)).child(div().flex().gap_3().child(labeled("Email", &self.onb.nw_email)).child(labeled("SSO issuer", &self.onb.nw_issuer)));
        }
        card = card.child(small(spaces::NEW_NOTE)).child(div().flex().justify_end().gap_2().child(btn(cx, "nw-cancel", "Cancel", RAISED_OFF, true, |this, _| this.onb.nw_kind = None)).child(btn(cx, "nw-create", spaces::submit_label(kind, busy), ACCENT, !busy, |this, cx| this.create_workspace(cx))));
        Some(div().id("nw-backdrop").absolute().inset_0().flex().justify_center().pt(px(120.)).bg(rgba(0x000000aa)).on_click(cx.listener(|this, _, _, cx| {
            this.onb.nw_kind = None;
            cx.notify();
        })).child(card))
    }
}

fn settings_accent_hex(id: &str) -> u32 {
    match id {
        "teal" => 0x56b6c2,
        "amber" => 0xf5a742,
        "blue" => 0x5c9cf5,
        "green" => 0x7fd88f,
        _ => ACCENT,
    }
}
