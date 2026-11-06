//! Home, the New artifact pipeline, the Approvals overview, the Project page, and the artifact actions of the
//! Library (versions, export, share, delete). Logic and requests are in `workspaceai_client::{home, create,
//! approvals, artifacts, instructions}`; this module is the window code, a child of the crate root.

use crate::onboarding::{btn, labeled, opt, panel, small};
use crate::text_input::TextInput;
use crate::ui;
use crate::{answer_permission, load_artifact, load_permissions, AppView, Msg, View, ACCENT, BG, MUTED, PANEL, RAISED_OFF, USER_BUBBLE};
use gpui::{div, prelude::*, px, rgb, ClipboardItem, Context, Div, Entity, FontWeight, SharedString};
use serde_json::Value;
use workspaceai_client::api::seg;
use workspaceai_client::approvals;
use workspaceai_client::artifacts::{self, ExportFormat};
use workspaceai_client::create::{self, Critique, Direction};
use workspaceai_client::home::{self, Go};
use workspaceai_client::instructions::{self, Layer};
use workspaceai_client::library::{self, Body};
use workspaceai_client::markdown;
use workspaceai_client::projects;
use workspaceai_client::types::Snapshot;
use workspaceai_client::workspace::{self as wsx, DesignSystem};

pub const PROJECT_TABS: [(&str, &str); 3] = [("chats", "Chats"), ("artifacts", "Artifacts"), ("instructions", "Instructions")];

pub struct Flow {
    // Home
    pub(crate) home_input: Entity<TextInput>,
    mode: &'static str,
    home_proj: Option<String>,
    // Create
    stage: usize,
    ty: &'static str,
    project: Option<String>,
    title: Entity<TextInput>,
    brief: Entity<TextInput>,
    ask: bool,
    sent: (String, String, String),
    chat: Option<String>,
    questions: Vec<String>,
    answers: Vec<Entity<TextInput>>,
    pending_questions: Option<Vec<String>>,
    directions: Vec<Direction>,
    direction: usize,
    systems: Vec<DesignSystem>,
    system: String,
    ds_query: Entity<TextInput>,
    art: Option<String>,
    art_view: Option<library::View>,
    /// The page Open Design stores for the shown version (what the web view shows).
    art_page: String,
    formats: Vec<ExportFormat>,
    tab: &'static str,
    crit: Option<Critique>,
    tweak: Entity<TextInput>,
    busy: String,
    // Project page
    proj: Option<String>,
    ptab: &'static str,
    raw: Value,
    layers: Vec<Layer>,
    agents: Entity<TextInput>,
    pending_agents: Option<String>,
    // Library artifact actions
    lib_ver: Option<u32>,
    lib_formats: Vec<ExportFormat>,
    clipboard: Option<String>,
}

impl Flow {
    pub fn new(cx: &mut Context<AppView>) -> Flow {
        let make = |cx: &mut Context<AppView>, ph: &str, multi: bool| {
            let i = cx.new(|cx| if multi { TextInput::new_multiline(cx, ph) } else { TextInput::new(cx, ph) });
            cx.observe(&i, |_, _, cx| cx.notify()).detach();
            i
        };
        Flow {
            home_input: make(cx, "Ask anything — it stays in the project you pick", false),
            mode: "chat",
            home_proj: None,
            stage: 0,
            ty: "Doc",
            project: None,
            title: make(cx, "Beta landing page", false),
            brief: make(cx, "What is it, who is it for, and what must it include?", true),
            ask: true,
            sent: Default::default(),
            chat: None,
            questions: vec![],
            answers: vec![],
            pending_questions: None,
            directions: vec![],
            direction: 0,
            systems: vec![],
            system: create::DEFAULT_SYSTEM.into(),
            ds_query: make(cx, "Search design systems", false),
            art: None,
            art_view: None,
            art_page: String::new(),
            formats: vec![],
            tab: "plan",
            crit: None,
            tweak: make(cx, "Describe a change", true),
            busy: String::new(),
            proj: None,
            ptab: "chats",
            raw: Value::Null,
            layers: vec![],
            agents: make(cx, "Rules for this project…", true),
            pending_agents: None,
            lib_ver: None,
            lib_formats: vec![],
            clipboard: None,
        }
    }
}

fn dim(text: impl Into<SharedString>) -> Div {
    div().text_color(rgb(MUTED)).child(text.into())
}

fn field(input: &Entity<TextInput>) -> Div {
    div().px_2().py_1().rounded_md().bg(rgb(BG)).child(input.clone())
}

fn tile(id: impl Into<SharedString>, title: &str, sub: &str) -> gpui::Stateful<Div> {
    div()
        .id(id.into())
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .rounded_lg()
        .cursor_pointer()
        .bg(rgb(PANEL))
        .hover(|s| s.bg(rgb(USER_BUBBLE)))
        .child(div().font_weight(FontWeight::BOLD).child(SharedString::from(title.to_string())))
        .child(small(sub.to_string()))
}

fn page(title: &str, lead: &str) -> Div {
    div().flex().flex_col().gap_3().max_w(px(900.)).child(ui::page_title(title, lead))
}

/// A row of chips for the live projects; `pick` is told which one was clicked.
fn project_chips(cx: &mut Context<AppView>, prefix: &str, snap: &Snapshot, current: &str, pick: impl Fn(&mut AppView, String) + Clone + 'static) -> Div {
    let mut d = div().flex().flex_wrap().gap_1();
    for n in projects::tree(&snap.projects, false) {
        let id = n.id.clone();
        let pick = pick.clone();
        d = d.child(ui::button(format!("{prefix}-{}", n.id), format!("{}{}", "  ".repeat(n.depth), n.name), if n.id == current { ACCENT } else { RAISED_OFF }).on_click(cx.listener(move |this, _, _, cx| {
            pick(this, id.clone());
            cx.notify();
        })));
    }
    d
}

fn chip(cx: &mut Context<AppView>, id: String, label: &str, on: bool, f: impl Fn(&mut AppView, &mut Context<AppView>) + 'static) -> gpui::Stateful<Div> {
    ui::button(id, label.to_string(), if on { ACCENT } else { RAISED_OFF }).on_click(cx.listener(move |this, _, _, cx| {
        f(this, cx);
        cx.notify();
    }))
}

impl AppView {
    pub(crate) fn ws_id(&self) -> String {
        self.model.active_ws.clone().unwrap_or_default()
    }

    pub(crate) fn flow_lib_ver(&self) -> Option<u32> {
        self.flow.lib_ver
    }

    fn flow_open(&mut self, view: View) {
        self.view = view;
        self.model.toast = None;
    }

    // ---------- entry points ----------

    pub fn open_home(&mut self, cx: &mut Context<Self>) {
        self.flow_open(View::Home);
        cx.notify();
    }

    pub fn open_create(&mut self, cx: &mut Context<Self>) {
        self.flow_open(View::Create);
        if self.flow.systems.is_empty() {
            self.get_into("flow-systems", create::design_systems_path(&self.ws_id()));
        }
        cx.notify();
    }

    pub fn open_approvals(&mut self) {
        self.flow_open(View::Approvals);
        if let (Some(api), Some(ws)) = (self.api.clone(), self.model.active_ws.clone()) {
            for c in approvals::waiting_chats(&self.model.snapshot) {
                load_permissions(api.clone(), ws.clone(), c, self.tx.clone());
            }
        }
    }

    pub fn open_project(&mut self, id: String) {
        self.flow_open(View::Project);
        self.flow.proj = Some(id);
        self.flow.layers.clear();
        self.flow.pending_agents = Some(String::new());
        self.project_fetch();
    }

    /// Selects an artifact in the Library and fetches what it shows.
    pub fn select_artifact(&mut self, id: String) {
        self.library_sel = Some(id.clone());
        self.library_view = None;
        self.library_page.clear();
        self.flow.lib_ver = None;
        self.flow.lib_formats.clear();
        self.confirm.cancel();
        if let Some(api) = self.api.clone() {
            load_artifact(api, self.ws_id(), id.clone(), None, self.tx.clone());
        }
        self.get_into("flow-lib-formats", artifacts::exports_path(&self.ws_id(), &id));
    }

    fn project_fetch(&mut self) {
        let (ws, Some(p)) = (self.ws_id(), self.flow.proj.clone()) else { return };
        self.get_into("flow-psnap", format!("/api/ws/{}/snapshot", seg(&ws)));
        self.get_into("flow-layers", instructions::layers_path(&ws, &p));
        self.get_into("flow-agents", instructions::agents_path(&ws, &p));
    }

    // ---------- replies and per-tick work ----------

    /// Handles a `flow-*` reply from `pages::fetched`. A failed request arrives as `{ok: false}`.
    pub fn flow_fetched(&mut self, kind: &'static str, v: Value) {
        let failed = v["ok"] == false;
        if matches!(kind, "flow-start" | "flow-questions" | "flow-directions" | "flow-generate" | "flow-critique" | "flow-fix" | "flow-handoff" | "flow-remember") {
            self.flow.busy.clear();
        }
        if failed {
            return;
        }
        let ws = self.ws_id();
        match kind {
            "flow-systems" => {
                self.flow.systems = wsx::parse_design_systems(&v);
                self.flow.system = create::initial_system(&self.flow.systems, &self.model.snapshot.workspace.design_system);
            }
            "flow-start" => {
                let Some(chat) = v["chat"].as_str() else { return };
                self.flow.chat = Some(chat.to_string());
                let (ty, title, brief) = self.flow.sent.clone();
                if self.flow.ask {
                    self.flow.busy = "Thinking of questions…".into();
                    self.call("POST", create::step_path(&ws, chat, "questions"), Some(create::questions_body(&ty, &title, &brief)), "flow-questions", None);
                } else {
                    self.flow_directions(Vec::new());
                }
            }
            "flow-questions" => {
                self.flow.pending_questions = Some(create::parse_questions(&v));
                self.flow.stage = 1;
            }
            "flow-directions" => {
                self.flow.directions = create::parse_directions(&v);
                self.flow.direction = 0;
                self.flow.stage = 2;
            }
            "flow-generate" | "flow-fix" => {
                if let Some(id) = v["id"].as_str() {
                    self.flow.art = Some(id.to_string());
                    self.flow.crit = None;
                    self.flow.art_view = None;
                    self.get_into("flow-content", format!("{}/content", artifacts::item_path(&ws, id)));
                    self.get_into("flow-formats", artifacts::exports_path(&ws, id));
                    self.model.toast = Some(format!("{} saved · v{}", v["type"].as_str().unwrap_or("Artifact"), v["version"]));
                }
            }
            "flow-content" => {
                self.flow.art_view = library::parse_view(&v);
                self.flow.art_page = workspaceai_client::web::page_of(&v);
            }
            "flow-formats" => self.flow.formats = artifacts::parse_formats(&v),
            "flow-lib-formats" => self.flow.lib_formats = artifacts::parse_formats(&v),
            "flow-critique" => self.flow.crit = create::parse_critique(&v),
            "flow-handoff" => {
                if let Some(chat) = v["chat"].as_str() {
                    self.flow.stage = 6;
                    self.view = View::Chat;
                    self.open_chat(chat.to_string());
                }
            }
            "flow-remember" => {
                self.flow.stage = 7;
                self.model.toast = Some(format!("{} memory suggestions — approve them in Settings › Memory", v["facts"].as_array().map_or(0, Vec::len)));
            }
            "flow-toast" => self.model.toast = v["text"].as_str().map(String::from),
            "flow-share" => {
                let (text, url) = artifacts::share_result(&v);
                self.flow.clipboard = url;
                self.model.toast = Some(text);
            }
            "flow-restored" => {
                if let Some(id) = self.library_sel.clone() {
                    self.flow.lib_ver = None;
                    self.library_view = None;
                    if let Some(api) = self.api.clone() {
                        load_artifact(api, ws, id, None, self.tx.clone());
                    }
                }
            }
            "flow-psnap" => self.flow.raw = v,
            "flow-layers" => self.flow.layers = instructions::parse_layers(&v),
            "flow-agents" => self.flow.pending_agents = Some(instructions::parse_agents(&v)),
            _ => {}
        }
    }

    /// Work that needs the window context: new input entities and the clipboard.
    pub fn flow_pump(&mut self, cx: &mut Context<Self>) {
        if let Some(qs) = self.flow.pending_questions.take() {
            self.flow.answers = qs
                .iter()
                .map(|_| {
                    let i = cx.new(|cx| TextInput::new(cx, "Your answer (optional)"));
                    cx.observe(&i, |_, _, cx| cx.notify()).detach();
                    i
                })
                .collect();
            self.flow.questions = qs;
        }
        if let Some(t) = self.flow.pending_agents.take() {
            self.flow.agents.update(cx, |i, cx| i.set_content(&t, cx));
        }
        if let Some(url) = self.flow.clipboard.take() {
            cx.write_to_clipboard(ClipboardItem::new_string(url));
        }
    }

    // ---------- actions ----------

    fn home_project(&self) -> Option<String> {
        let open = self.model.open_chat_id.as_ref().and_then(|c| self.model.snapshot.chats.iter().find(|x| &x.id == c)).map(|c| c.project.clone());
        home::target_project(&self.model.snapshot, self.flow.home_proj.as_deref(), open.as_deref())
    }

    pub(crate) fn send_home(&mut self, cx: &mut Context<Self>) {
        let text = self.flow.home_input.read(cx).content().to_string();
        if self.flow.mode == "loop" {
            self.model.toast = Some("Loops are graphs, and the graph editor is not in the native app yet.".into());
            return;
        }
        let (Some(project), Some(api), ws) = (self.home_project(), self.api.clone(), self.ws_id()) else { return };
        let Some(body) = home::chat_body(&project, self.flow.mode, &text) else {
            self.model.toast = Some("Type a message first".into());
            return;
        };
        self.flow.home_input.update(cx, |i, cx| i.set_content("", cx));
        self.flow.mode = "chat";
        let tx = self.tx.clone();
        crate::spawn(move || {
            let r = api.post(&home::chats_path(&ws), Some(&body)).map_err(|e| e.to_string()).and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()));
            let _ = tx.send(match r {
                Ok(c) => Msg::ChatCreated(ws, c),
                Err(e) => Msg::Error(e),
            });
        });
    }

    fn create_project(&self) -> String {
        self.flow.project.clone().or_else(|| self.home_project()).unwrap_or_default()
    }

    fn flow_start(&mut self, ask: bool, cx: &mut Context<Self>) {
        let (title, brief) = (self.flow.title.read(cx).content().to_string(), self.flow.brief.read(cx).content().to_string());
        let ty = self.flow.ty;
        let Some(body) = create::start_body(&self.create_project(), ty, &title, &brief) else { return };
        self.flow.ask = ask;
        self.flow.sent = (ty.to_string(), title, brief);
        self.flow.busy = "Opening the pipeline chat…".into();
        let ws = self.ws_id();
        self.call("POST", create::start_path(&ws), Some(body), "flow-start", None);
    }

    fn flow_directions(&mut self, qa: Vec<(String, String)>) {
        let (Some(chat), ws) = (self.flow.chat.clone(), self.ws_id()) else { return };
        let (ty, title, brief) = self.flow.sent.clone();
        self.flow.busy = "Sketching three directions…".into();
        self.call("POST", create::step_path(&ws, &chat, "directions"), Some(create::directions_body(&ty, &title, &brief, &qa)), "flow-directions", None);
    }

    fn flow_answers(&self, cx: &Context<Self>) -> Vec<(String, String)> {
        self.flow.questions.iter().zip(&self.flow.answers).map(|(q, a)| (q.clone(), a.read(cx).content().to_string())).collect()
    }

    fn system_title(&self) -> String {
        self.flow.systems.iter().find(|d| d.id == self.flow.system).map_or(String::new(), |d| d.title.clone())
    }

    fn flow_generate(&mut self) {
        let (Some(chat), Some(d), ws) = (self.flow.chat.clone(), self.flow.directions.get(self.flow.direction).cloned(), self.ws_id()) else { return };
        self.flow.stage = 4;
        self.flow.art = None;
        self.flow.art_view = None;
        let (ty, title, _) = self.flow.sent.clone();
        self.flow.busy = format!("Generating with {}…", self.system_title());
        let body = create::generate_body(&ty, &title, &d, &self.flow.system, &self.system_title());
        self.call("POST", create::step_path(&ws, &chat, "generate"), Some(body), "flow-generate", None);
    }

    fn flow_fix(&mut self, issues: Vec<create::Issue>, busy: &str) {
        let (Some(chat), Some(art), ws) = (self.flow.chat.clone(), self.flow.art.clone(), self.ws_id()) else { return };
        self.flow.busy = busy.into();
        self.call("POST", create::step_path(&ws, &chat, "fix"), Some(create::fix_body(&art, &issues)), "flow-fix", None);
    }

    /// Saves one export of an artifact into Downloads.
    pub fn export_artifact(&mut self, art: String, format: String, version: u32, ext: String) {
        let (Some(api), ws) = (self.api.clone(), self.ws_id()) else { return };
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = workspaceai_client::spaces::download_dir(&home);
        self.model.toast = Some("Exporting…".into());
        let (tx, path) = (self.tx.clone(), artifacts::export_path(&ws, &art, &format, version));
        crate::spawn(move || {
            let _ = tx.send(match api.download_to(&path, &dir) {
                Ok(p) => Msg::Fetched("flow-toast", ws, serde_json::json!({ "text": format!("Exported to {}", p.display()) })),
                Err(e) => Msg::Error(format!("Export {ext} failed: {e}")),
            });
        });
    }

    fn reply_ask(&mut self, chat: &str, id: &str, decision: &'static str) {
        let (Some(api), ws) = (self.api.clone(), self.ws_id()) else { return };
        self.model.drop_permission(chat, id);
        self.model.toast = Some(approvals::decision_toast(decision).into());
        answer_permission(api, ws, chat.to_string(), id.to_string(), decision, self.tx.clone());
    }

    // ---------- Home ----------

    pub fn home_view(&self, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let snap = &self.model.snapshot;
        let is_org = snap.workspace.kind == "org";
        let proj = self.home_project().unwrap_or_default();
        let proj_name = snap.projects.iter().find(|p| p.id == proj).map_or("—".to_string(), |p| p.name.clone());
        let hour = chrono::Timelike::hour(&chrono::Local::now());
        let mut modes = div().flex().flex_wrap().gap_1();
        for (id, label, _) in home::MODES {
            modes = modes.child(chip(cx, format!("home-mode-{id}"), label, self.flow.mode == id, move |this, _| this.flow.mode = id));
        }
        let composer = panel()
            .child(field(&self.flow.home_input))
            .child(small(home::placeholder(self.flow.mode)))
            .child(div().flex().flex_wrap().items_center().gap_2().child(small(format!("Project: {proj_name}"))).child(modes))
            .child(project_chips(cx, "home-proj", snap, &proj, |this, id| this.flow.home_proj = Some(id)))
            .child(div().flex().justify_end().child(btn(cx, "home-send", "Send", ACCENT, true, |this, cx| this.send_home(cx))));
        let mut sugg = div().flex().flex_wrap().justify_center().gap_2();
        for (i, t) in home::suggestions(is_org, &proj_name).into_iter().enumerate() {
            sugg = sugg.child(chip(cx, format!("home-sugg-{i}"), &t.clone(), false, move |this, cx| this.flow.home_input.update(cx, |inp, cx| inp.set_content(&t, cx))));
        }
        let needs = home::needs(snap);
        let mut c1 = div().flex().flex_col().flex_1().min_w_0().gap_2().child(div().text_sm().font_weight(FontWeight::BOLD).text_color(rgb(MUTED)).child("NEEDS YOU"));
        for (i, n) in needs.iter().enumerate() {
            let go = n.go;
            c1 = c1.child(tile(format!("home-need-{i}"), &n.title, &n.sub).on_click(cx.listener(move |this, _, _, cx| {
                this.settings_page = if go == Go::Memory { "memory" } else { "routines" };
                this.open_page(this.settings_page, cx);
                this.view = View::Settings;
                cx.notify();
            })));
        }
        if needs.is_empty() {
            c1 = c1.child(small("Nothing waiting on you."));
        }
        let mut c2 = div().flex().flex_col().flex_1().min_w_0().gap_2().child(div().text_sm().font_weight(FontWeight::BOLD).text_color(rgb(MUTED)).child("CONTINUE"));
        let recent = home::recent(snap);
        for c in &recent {
            let id = c.id.clone();
            let p = snap.projects.iter().find(|p| p.id == c.project).map_or("", |p| p.name.as_str());
            let working = if snap.chat_activity.contains_key(&c.id) { " ●" } else { "" };
            c2 = c2.child(tile(format!("home-chat-{}", c.id), &format!("{}{working}", if c.title.is_empty() { "New chat" } else { &c.title }), p).on_click(cx.listener(move |this, _, _, cx| {
                this.view = View::Chat;
                this.open_chat(id.clone());
                cx.notify();
            })));
        }
        if recent.is_empty() {
            c2 = c2.child(small("Your chats will show up here."));
        }
        let mut c3 = div().flex().flex_col().flex_1().min_w_0().gap_2().child(div().text_sm().font_weight(FontWeight::BOLD).text_color(rgb(MUTED)).child("COMING UP"));
        let now = chrono::Utc::now();
        for r in home::upcoming(snap) {
            c3 = c3.child(tile(format!("home-rt-{}", r.id), &r.name, &workspaceai_client::routines::trigger_text(r, now)).on_click(cx.listener(|this, _, _, cx| {
                this.settings_page = "routines";
                this.open_page("routines", cx);
                this.view = View::Settings;
                cx.notify();
            })));
        }
        c3 = c3.child(tile("home-rt-new", "+ Make something run on a schedule", "Routines").on_click(cx.listener(|this, _, _, cx| {
            this.settings_page = "routines";
            this.open_page("routines", cx);
            this.view = View::Settings;
            cx.notify();
        })));
        div().id("home-scroll").flex_1().min_w_0().h_full().overflow_y_scroll().flex().justify_center().child(
            div()
                .w_full()
                .max_w(px(860.))
                .p(px(32.))
                .pt(px(56.))
                .flex()
                .flex_col()
                .gap_4()
                .child(div().flex().justify_center().text_size(px(38.)).font_weight(FontWeight::BOLD).child(SharedString::from(home::greeting(hour))))
                .child(composer)
                .child(sugg)
                .child(div().flex().gap_3().child(c1).child(c2).child(c3)),
        )
    }

    // ---------- Create ----------

    pub fn create_nav(&self, cx: &mut Context<Self>) -> Div {
        let mut d = div().flex().flex_col().w(px(220.)).h_full().bg(rgb(PANEL)).p_3().gap_1().child(div().text_sm().font_weight(FontWeight::BOLD).text_color(rgb(MUTED)).pb_2().child("NEW ARTIFACT"));
        for (i, s) in create::STAGES.iter().enumerate() {
            let on = i == self.flow.stage;
            let enabled = create::stage_enabled(i, self.flow.stage, self.flow.art.is_some());
            let mark = if i < self.flow.stage { "✓".to_string() } else { (i + 1).to_string() };
            let row = div().id(SharedString::from(format!("stage-{i}"))).flex().items_center().gap_2().px_2().py_1().rounded_md().bg(rgb(if on { USER_BUBBLE } else { PANEL })).child(div().text_color(rgb(if i <= self.flow.stage { ACCENT } else { MUTED })).child(SharedString::from(mark))).child(*s);
            d = d.child(if enabled {
                row.cursor_pointer().on_click(cx.listener(move |this, _, _, cx| {
                    this.flow.stage = i;
                    cx.notify();
                })).into_any_element()
            } else {
                row.opacity(0.5).into_any_element()
            });
        }
        if let Some(chat) = self.flow.chat.clone() {
            d = d.child(div().id("stage-transcript").mt_2().px_2().text_sm().text_color(rgb(ACCENT)).cursor_pointer().child("Pipeline transcript").on_click(cx.listener(move |this, _, _, cx| {
                this.view = View::Chat;
                this.open_chat(chat.clone());
                cx.notify();
            })));
        }
        d.child(div().mt_auto().text_sm().text_color(rgb(MUTED)).child(SharedString::from(format!("Open Design pipeline · {}", self.model.snapshot.projects.iter().find(|p| p.id == self.create_project()).map_or("", |p| p.name.as_str())))))
    }

    pub fn create_view(&self, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let f = &self.flow;
        let busy = !f.busy.is_empty();
        let mut col = div().flex().flex_col().gap_3();
        if busy {
            col = col.child(div().p_3().rounded_lg().bg(rgb(PANEL)).text_color(rgb(ACCENT)).child(SharedString::from(f.busy.clone())));
        }
        col = col.child(match f.stage {
            0 => self.brief_stage(cx),
            1 => {
                let mut p = page("A few questions", "");
                for (i, q) in f.questions.iter().enumerate() {
                    if let Some(a) = f.answers.get(i) {
                        p = p.child(labeled(q, a));
                    }
                }
                p.child(div().flex().justify_end().child(btn(cx, "q-next", "Pick a direction", ACCENT, !busy, |this, cx| {
                    let qa = this.flow_answers(cx);
                    this.flow_directions(qa);
                })))
            }
            2 => {
                let mut row = div().flex().flex_wrap().gap_2();
                for (i, d) in f.directions.iter().enumerate() {
                    row = row.child(div().w(px(270.)).flex().child(opt(cx, format!("dir-{i}"), &d.name, &format!("{}\nTone · {}", d.summary, d.tone), f.direction == i, true, move |this| this.flow.direction = i)));
                }
                page("Pick a direction", "").child(row).child(
                    div()
                        .flex()
                        .gap_2()
                        .child(btn(cx, "dir-more", "Three more", RAISED_OFF, !busy, |this, cx| {
                            let qa = this.flow_answers(cx);
                            this.flow_directions(qa);
                        }))
                        .child(div().flex_1())
                        .child(btn(cx, "dir-next", "Design system", ACCENT, !f.directions.is_empty(), |this, _| this.flow.stage = 3)),
                )
            }
            3 => {
                let q = f.ds_query.read(cx).content().to_string();
                let mut grid = div().id("ds-grid").flex().flex_wrap().gap_2().max_h(px(460.)).overflow_y_scroll();
                for d in create::filter_systems(&f.systems, &q) {
                    let id = d.id.clone();
                    let mut sw = div().flex().gap_1();
                    for c in d.swatches.iter().take(5) {
                        if let Ok(v) = u32::from_str_radix(c.trim_start_matches('#'), 16) {
                            sw = sw.child(div().size(px(16.)).rounded_sm().bg(rgb(v)));
                        }
                    }
                    grid = grid.child(
                        div()
                            .id(SharedString::from(format!("ds-{}", d.id)))
                            .w(px(210.))
                            .flex()
                            .flex_col()
                            .gap_1()
                            .p_3()
                            .rounded_lg()
                            .cursor_pointer()
                            .border_1()
                            .border_color(rgb(if f.system == d.id { ACCENT } else { RAISED_OFF }))
                            .bg(rgb(if f.system == d.id { USER_BUBBLE } else { PANEL }))
                            .child(sw)
                            .child(div().font_weight(FontWeight::BOLD).child(SharedString::from(d.title.clone())))
                            .child(small(create::system_note(d).chars().take(60).collect::<String>()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.flow.system = id.clone();
                                cx.notify();
                            })),
                    );
                }
                page("Design system", "Its DESIGN.md is given to the agent as the brand contract.").child(field(&f.ds_query)).child(grid).child(div().flex().justify_end().child(btn(cx, "ds-generate", "Generate", ACCENT, !busy && f.chat.is_some(), |this, _| this.flow_generate())))
            }
            _ => self.generated_stage(cx),
        });
        div().id("create-scroll").flex_1().min_w_0().h_full().overflow_y_scroll().p(px(28.)).child(col)
    }

    fn brief_stage(&self, cx: &mut Context<Self>) -> Div {
        let f = &self.flow;
        let busy = !f.busy.is_empty();
        let snap = &self.model.snapshot;
        let mut outs = div().flex().flex_wrap().gap_2();
        for (ty, label, note) in create::OUTPUTS {
            outs = outs.child(div().w(px(190.)).flex().child(opt(cx, format!("out-{ty}"), label, note, f.ty == ty, true, move |this| this.flow.ty = ty)));
        }
        let mut tpl = div().flex().flex_wrap().items_center().gap_1().child(small("Start from a template "));
        for (i, (label, ty, title, brief)) in create::TEMPLATES.iter().enumerate() {
            let (ty, title, brief) = (*ty, *title, *brief);
            tpl = tpl.child(chip(cx, format!("tpl-{i}"), label, false, move |this, cx| {
                this.flow.ty = create::OUTPUTS.iter().find(|o| o.0 == ty).map_or("Doc", |o| o.0);
                this.flow.title.update(cx, |i, cx| i.set_content(title, cx));
                this.flow.brief.update(cx, |i, cx| i.set_content(brief, cx));
            }));
        }
        let project = self.create_project();
        let pname = snap.projects.iter().find(|p| p.id == project).map_or("", |p| p.name.as_str()).to_string();
        let ready = !f.brief.read(cx).content().trim().is_empty();
        page("New artifact brief", "")
            .child(small("OUTPUT"))
            .child(outs)
            .child(tpl)
            .child(div().flex().child(labeled("Title", &f.title)))
            .child(small(format!("Project — artifacts always belong to one: {pname}")))
            .child(project_chips(cx, "cr-proj", snap, &project, |this, id| this.flow.project = Some(id)))
            .child(labeled("Brief", &f.brief))
            .child(small(format!("It follows {pname}’s instructions and memory, like a chat there.")))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(btn(cx, "brief-skip", "Skip questions", RAISED_OFF, ready && !busy, |this, cx| this.flow_start(false, cx)))
                    .child(btn(cx, "brief-ask", "Ask me questions", ACCENT, ready && !busy, |this, cx| this.flow_start(true, cx))),
            )
    }

    fn generated_stage(&self, cx: &mut Context<Self>) -> Div {
        let f = &self.flow;
        let busy = !f.busy.is_empty();
        let snap = &self.model.snapshot;
        let Some(art) = f.art.as_ref().and_then(|id| snap.artifacts.iter().find(|a| &a.id == id)) else {
            return page("Generate", if busy { "" } else { "Generation did not finish. Go back to Design system and try again." }).child(dim(if busy { f.busy.clone() } else { String::new() }));
        };
        let latest = artifacts::version_rows(art).into_iter().next();
        let body = match &f.art_view {
            None => dim("Loading…").into_any_element(),
            Some(v) => match self.art_source(&art.kind, v, &f.art_page) {
                Some(src) => div().flex().flex_col().gap_2().child(self.device_toggle(&art.kind, cx)).child(div().h(px(520.)).rounded_md().overflow_hidden().child(self.web_slot(&self.side.art, src))).into_any_element(),
                None => match library::body_for(&v.format, &v.content) {
                    Body::Markdown(t) if art.kind != "Deck" => md_blocks(&t).into_any_element(),
                    Body::Text(t) => mono(t).into_any_element(),
                    Body::Markdown(t) => mono(t).into_any_element(),
                    Body::Unsupported(why) => div().p_4().rounded_lg().bg(rgb(PANEL)).text_color(rgb(MUTED)).child(SharedString::from(why)).into_any_element(),
                },
            },
        };
        let tabs = {
            let mut t = div().flex().gap_1();
            for (k, l) in [("plan", "Plan"), ("crit", "Critique"), ("tweaks", "Tweaks"), ("ship", "Export")] {
                t = t.child(chip(cx, format!("gtab-{k}"), l, f.tab == k, move |this, _| this.flow.tab = k));
            }
            t
        };
        let side = match f.tab {
            "plan" => {
                let d = f.directions.get(f.direction);
                let versions = artifacts::version_rows(art).iter().map(|v| format!("v{} {}", v.v, v.note)).collect::<Vec<_>>().join(" · ");
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(SharedString::from(format!("Direction · {}", d.map_or("—", |d| d.name.as_str()))))
                    .child(small(d.map_or(String::new(), |d| d.summary.clone())))
                    .child(SharedString::from(format!("Design system · {}", if self.system_title().is_empty() { art.design_system.clone() } else { self.system_title() })))
                    .child(small(format!("Versions · {versions}")))
                    .child(btn(cx, "run-crit", "Run the critique", ACCENT, !busy, |this, _| this.run_critique()))
            }
            "crit" => {
                let mut c = div().flex().flex_col().gap_2();
                match &f.crit {
                    None => c = c.child(btn(cx, "run-crit2", "Run the critique", ACCENT, !busy, |this, _| this.run_critique())),
                    Some(cr) => {
                        c = c.child(SharedString::from(format!("Composite {}/10 · {}", cr.composite, if cr.passed { "passes".to_string() } else { format!("below {}", cr.threshold) })));
                        for r in &cr.roles {
                            let v = cr.score(&r.id);
                            c = c.child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(div().flex().justify_between().text_sm().child(SharedString::from(format!("{} · weight {}", r.name, r.weight))).child(SharedString::from(format!("{v}"))))
                                    .child(div().h(px(5.)).rounded_sm().bg(rgb(USER_BUBBLE)).child(div().h(px(5.)).rounded_sm().w(px(2. * (v as f32 * 10.).clamp(0., 100.))).bg(rgb(create::score_tone(v))))),
                            );
                        }
                        for i in &cr.issues {
                            c = c.child(small(format!("{} · {}\n→ {}", i.role, i.issue, i.fix)));
                        }
                        if !cr.issues.is_empty() {
                            let issues = cr.issues.clone();
                            c = c.child(btn(cx, "apply-fixes", "Apply fixes", ACCENT, !busy, move |this, _| this.flow_fix(issues.clone(), "Applying fixes as a new version…")));
                        }
                    }
                }
                c
            }
            "tweaks" => {
                let html = f.art_view.as_ref().is_some_and(|v| v.format == "html");
                let mut c = div().flex().flex_col().gap_2();
                let mut sugg = div().flex().flex_wrap().gap_1();
                for (i, t) in create::tweak_suggestions(html).into_iter().enumerate() {
                    sugg = sugg.child(chip(cx, format!("tw-{i}"), t, false, move |this, cx| this.flow.tweak.update(cx, |inp, cx| inp.set_content(t, cx))));
                }
                let text = f.tweak.read(cx).content().to_string();
                c = c.child(sugg).child(field(&f.tweak));
                c.child(btn(cx, "tweak-apply", "Apply as a new version", ACCENT, !busy && !text.trim().is_empty(), move |this, cx| {
                    if let Some(issues) = create::tweak_issues(&text) {
                        this.flow_fix(issues, "Applying the change as a new version…");
                        this.flow.tweak.update(cx, |i, cx| i.set_content("", cx));
                    }
                }))
            }
            _ => {
                let mut c = div().flex().flex_col().gap_1();
                for ef in &f.formats {
                    let (id, format, ext) = (art.id.clone(), ef.format.clone(), ef.ext.clone());
                    c = c.child(btn(cx, &format!("ship-{}", ef.format), &format!("{}  {}", ef.label, ef.ext), RAISED_OFF, true, move |this, _| this.export_artifact(id.clone(), format.clone(), 0, ext.clone())));
                }
                if f.formats.is_empty() {
                    c = c.child(small("No export formats for this artifact."));
                }
                c.child(small("Files go to your Downloads folder."))
            }
        };
        let lint = artifacts::lint_text(latest.as_ref().and_then(|v| v.lint));
        let aid = art.id.clone();
        let (rem_id, rem_dir, rem_sys) = (art.id.clone(), f.directions.get(f.direction).cloned(), self.system_title());
        page(&art.title, &format!("{} · v{} · {lint}", art.kind, art.version))
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(div().flex_1().min_w_0().p_3().rounded_lg().bg(rgb(PANEL)).child(body))
                    .child(div().w(px(320.)).flex().flex_col().gap_2().p_3().rounded_lg().bg(rgb(PANEL)).child(tabs).child(side)),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(btn(cx, "save-lib", "Save to the library", RAISED_OFF, true, move |this, _| {
                        this.view = View::Library;
                        this.select_artifact(aid.clone());
                    }))
                    .child(div().flex_1())
                    .child(btn(cx, "remember", "Remember direction and tone", RAISED_OFF, !busy, move |this, _| {
                        let (Some(chat), ws) = (this.flow.chat.clone(), this.ws_id()) else { return };
                        this.flow.busy = "Saving design decisions…".into();
                        this.call("POST", create::step_path(&ws, &chat, "remember"), Some(create::remember_body(&rem_id, rem_dir.as_ref(), &rem_sys)), "flow-remember", None);
                    }))
                    .child(btn(cx, "handoff", "Continue as code", ACCENT, !busy, |this, _| {
                        let (Some(chat), Some(art), ws) = (this.flow.chat.clone(), this.flow.art.clone(), this.ws_id()) else { return };
                        this.flow.busy = "Handing off to a project chat…".into();
                        this.call("POST", create::step_path(&ws, &chat, "handoff"), Some(create::critique_body(&art)), "flow-handoff", None);
                    })),
            )
    }

    fn run_critique(&mut self) {
        let (Some(chat), Some(art), ws) = (self.flow.chat.clone(), self.flow.art.clone(), self.ws_id()) else { return };
        self.flow.tab = "crit";
        self.flow.stage = self.flow.stage.max(5);
        self.flow.busy = "The design jury is reviewing…".into();
        self.call("POST", create::step_path(&ws, &chat, "critique"), Some(create::critique_body(&art)), "flow-critique", None);
    }

    // ---------- Approvals ----------

    pub fn approvals_view(&self, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let asks = approvals::pending(&self.model.snapshot, &self.model.permissions);
        let mut p = page("Approvals", "Requests from agents that are waiting for your answer, across all chats.");
        for a in &asks {
            let (c1, c2, c3) = (a.clone(), a.clone(), a.clone());
            let open = a.chat.clone();
            let mut card = panel()
                .child(div().child(SharedString::from(format!("{} asks to use {}", a.agent, a.title))))
                .child(small(format!("{} · {}", a.chat_title, a.project)));
            // Security: never let someone approve a request whose detail is cut off. Allow stays off until the full text is read in the chat.
            let total = a.detail.chars().count();
            let truncated = total > 300;
            if !a.detail.is_empty() {
                let shown: String = a.detail.chars().take(300).collect();
                let text = if truncated { format!("{shown}…(truncated, {} more characters — open the chat to read the full request)", total - 300) } else { shown };
                card = card.child(div().font_family("monospace").text_size(px(12.)).text_color(rgb(MUTED)).child(SharedString::from(text)));
            }
            let key = format!("{}-{}", a.chat, a.id);
            p = p.child(
                card.child(
                    div()
                        .flex()
                        .gap_2()
                        .child(btn(cx, &format!("ap-once-{key}"), "Allow once", ACCENT, !truncated, move |this, _| this.reply_ask(&c1.chat, &c1.id, "once")))
                        .child(btn(cx, &format!("ap-always-{key}"), "Always allow", RAISED_OFF, !truncated, move |this, _| this.reply_ask(&c2.chat, &c2.id, "always")))
                        .child(btn(cx, &format!("ap-reject-{key}"), "Reject", ui::DANGER, true, move |this, _| this.reply_ask(&c3.chat, &c3.id, "reject")))
                        .child(btn(cx, &format!("ap-open-{key}"), "Open chat", RAISED_OFF, true, move |this, _| {
                            this.view = View::Chat;
                            this.open_chat(open.clone());
                        })),
                ),
            );
        }
        if asks.is_empty() {
            p = p.child(dim("Nothing is waiting for approval."));
        }
        div().id("approvals-scroll").flex_1().min_w_0().h_full().overflow_y_scroll().p(px(28.)).child(p)
    }

    pub fn approvals_count(&self) -> usize {
        approvals::pending(&self.model.snapshot, &self.model.permissions).len()
    }

    // ---------- Project ----------

    pub fn project_view(&self, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let snap = &self.model.snapshot;
        let Some(proj) = self.flow.proj.as_ref().and_then(|id| snap.projects.iter().find(|p| &p.id == id)) else {
            return div().id("project-none").flex_1().p(px(28.)).child(dim("Pick a project in the sidebar."));
        };
        let mut path = vec![proj.name.clone()];
        let mut cur = proj.parent.clone();
        for _ in 0..16 {
            let Some(p) = cur.and_then(|id| snap.projects.iter().find(|x| x.id == id)) else { break };
            path.insert(0, p.name.clone());
            cur = p.parent.clone();
        }
        let ws = self.ws_id();
        let ids = instructions::within(&snap.projects, &proj.id);
        let mut tabs = div().flex().gap_1();
        for (k, l) in PROJECT_TABS {
            tabs = tabs.child(chip(cx, format!("ptab-{k}"), l, self.flow.ptab == k, move |this, _| this.flow.ptab = k));
        }
        let mut p = page(&proj.name, &path.join(" / ")).child(tabs);
        match self.flow.ptab {
            "chats" => {
                let mut chats: Vec<_> = snap.chats.iter().filter(|c| !c.archived && ids.contains(&c.project)).collect();
                chats.sort_by_key(|c| std::cmp::Reverse(c.updated_at));
                for c in &chats {
                    let id = c.id.clone();
                    let sub = if c.project == proj.id { String::new() } else { snap.projects.iter().find(|x| x.id == c.project).map_or(String::new(), |x| x.name.clone()) };
                    p = p.child(tile(format!("pchat-{}", c.id), if c.title.is_empty() { "New chat" } else { &c.title }, &sub).on_click(cx.listener(move |this, _, _, cx| {
                        this.view = View::Chat;
                        this.open_chat(id.clone());
                        cx.notify();
                    })));
                }
                if chats.is_empty() {
                    p = p.child(dim("No chats in this project yet."));
                }
            }
            "artifacts" => {
                let mut arts: Vec<_> = snap.artifacts.iter().filter(|a| ids.contains(&a.project)).collect();
                arts.sort_by_key(|a| std::cmp::Reverse(a.updated_at));
                for a in &arts {
                    let id = a.id.clone();
                    let sub = format!("{} v{} · {}", a.kind, a.version, snap.projects.iter().find(|x| x.id == a.project).map_or("", |x| x.name.as_str()));
                    p = p.child(tile(format!("part-{}", a.id), &a.title, &sub).on_click(cx.listener(move |this, _, _, cx| {
                        this.view = View::Library;
                        this.select_artifact(id.clone());
                        cx.notify();
                    })));
                }
                if arts.is_empty() {
                    p = p.child(dim("Artifacts made in this project, and in its sub-projects, show up here."));
                }
            }
            _ => {
                let meta = instructions::meta(&self.flow.raw, &proj.id);
                let pid = proj.id.clone();
                let (ws1, pid1) = (ws.clone(), pid.clone());
                p = p.child(ui::row("Inherit from parents", "Include the instructions above this project", ui::switch("p-inherit", meta.inherit, false).on_click(cx.listener(move |this, _, _, cx| {
                    this.call("PATCH", format!("/api/ws/{}/projects/{}", seg(&ws1), seg(&pid1)), Some(instructions::inherit_body(!meta.inherit)), "flow-pset", Some(("flow-psnap", format!("/api/ws/{}/snapshot", seg(&ws1)))));
                    cx.notify();
                }))));
                // Model: this project's own choice, else its parent's, else the workspace default.
                let source = instructions::model_source(&self.flow.raw, snap, &proj.id);
                let mut models = div().flex().flex_wrap().gap_1().child(chip(cx, "pm-inherit".into(), "Inherit", meta.model.is_empty(), {
                    let (ws, pid) = (ws.clone(), pid.clone());
                    move |this, _| this.call("PATCH", format!("/api/ws/{}/projects/{}", seg(&ws), seg(&pid)), Some(instructions::model_body("")), "flow-pset", Some(("flow-psnap", format!("/api/ws/{}/snapshot", seg(&ws)))))
                }));
                for (i, m) in self.models.options_for("opencode").into_iter().enumerate().take(40) {
                    let (ws, pid, id) = (ws.clone(), pid.clone(), m.id.clone());
                    let label = if m.name.is_empty() { m.id.clone() } else { m.name.clone() };
                    models = models.child(chip(cx, format!("pm-{i}"), &label, meta.model == m.id, move |this, _| {
                        this.call("PATCH", format!("/api/ws/{}/projects/{}", seg(&ws), seg(&pid)), Some(instructions::model_body(&id)), "flow-pset", Some(("flow-psnap", format!("/api/ws/{}/snapshot", seg(&ws)))))
                    }));
                }
                p = p.child(panel().child(div().font_weight(FontWeight::BOLD).child("Model")).child(small(format!("Used for new chats here: {source}"))).child(models));
                for l in &self.flow.layers {
                    p = p.child(panel().child(small(l.name.to_uppercase())).child(div().font_family("monospace").text_size(px(12.)).child(SharedString::from(if l.text.is_empty() { "Empty".to_string() } else { l.text.clone() }))));
                }
                let (ws2, pid2) = (ws.clone(), pid.clone());
                p = p
                    .child(small("This project's AGENTS.md — the narrowest layer. It wins when it disagrees with the ones above."))
                    .child(field(&self.flow.agents))
                    .child(btn(cx, "save-agents", "Save AGENTS.md", ACCENT, true, move |this, cx| {
                        let text = this.flow.agents.read(cx).content().to_string();
                        this.call("PUT", instructions::agents_path(&ws2, &pid2), Some(instructions::save_body(&text)), "flow-psaved", None);
                        this.model.toast = Some("Instructions saved".into());
                    }));
            }
        }
        div().id("project-scroll").flex_1().min_w_0().h_full().overflow_y_scroll().p(px(28.)).child(p)
    }

    // ---------- Library artifact actions ----------

    /// Export, share, versions and delete for the artifact shown in the Library.
    pub fn library_extras(&self, a: &workspaceai_client::types::Artifact, cx: &mut Context<Self>) -> Div {
        let ws = self.ws_id();
        let shown = self.flow.lib_ver.unwrap_or(a.version);
        let mut actions = div().flex().flex_wrap().gap_2();
        for ef in &self.flow.lib_formats {
            let (id, format, ext) = (a.id.clone(), ef.format.clone(), ef.ext.clone());
            let v = self.flow.lib_ver.unwrap_or(0);
            actions = actions.child(btn(cx, &format!("lib-ex-{}", ef.format), &format!("Export {}", ef.label), RAISED_OFF, true, move |this, _| this.export_artifact(id.clone(), format.clone(), v, ext.clone())));
        }
        let (ws_s, id_s) = (ws.clone(), a.id.clone());
        actions = actions.child(btn(cx, "lib-share", "Share", RAISED_OFF, true, move |this, _| this.call("POST", artifacts::share_path(&ws_s, &id_s), None, "flow-share", None)));
        let armed = self.confirm.is_armed(&format!("a:{}", a.id));
        let (ws_d, id_d) = (ws.clone(), a.id.clone());
        actions = actions.child(btn(cx, "lib-del", if armed { "Click again to delete" } else { "Delete" }, if armed { 0x7a2b2b } else { ui::DANGER }, true, move |this, _| {
            if this.confirm.press(&format!("a:{id_d}")) {
                this.mutate_path("DELETE", artifacts::item_path(&ws_d, &id_d), None);
                this.library_sel = None;
                this.library_view = None;
            }
        }));
        let mut versions = div().flex().flex_col().gap_1().child(small("VERSIONS"));
        for v in artifacts::version_rows(a) {
            let on = v.v == shown;
            let id = a.id.clone();
            let ws_v = ws.clone();
            let lint = match v.lint {
                None => String::new(),
                Some(0) => " · OD lint clean".into(),
                Some(n) => format!(" · OD lint {n}"),
            };
            let when = workspaceai_client::library::updated_text(v.at, &workspaceai_shared::schedule::local_tz(), chrono::Utc::now());
            let mut row = div().flex().gap_2().items_center().child(
                div()
                    .id(SharedString::from(format!("ver-{}", v.v)))
                    .flex_1()
                    .min_w_0()
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(rgb(if on { USER_BUBBLE } else { PANEL }))
                    .child(SharedString::from(format!("v{} · {} · {}{lint}", v.v, artifacts::format_label(&v.format), if v.note.is_empty() { "—" } else { &v.note })))
                    .child(small(when))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.flow.lib_ver = Some(v.v);
                        this.library_view = None;
                        if let Some(api) = this.api.clone() {
                            load_artifact(api, ws_v.clone(), id.clone(), Some(v.v), this.tx.clone());
                        }
                        cx.notify();
                    })),
            );
            if !v.is_latest {
                let (ws_r, id_r, vn) = (ws.clone(), a.id.clone(), v.v);
                row = row.child(btn(cx, &format!("ver-restore-{vn}"), "Restore", RAISED_OFF, true, move |this, _| {
                    this.call("POST", artifacts::restore_path(&ws_r, &id_r), Some(artifacts::restore_body(vn)), "flow-restored", None);
                    this.model.toast = Some(format!("Restored v{vn} as a new version"));
                }));
            }
            versions = versions.child(row);
        }
        let lint = artifacts::lint_lines(a, shown);
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(small(artifacts::lint_text(a.versions.iter().find(|x| x.v == shown).and_then(|x| x.od.as_ref()).map(|o| o.lint.len()))))
            .children((!lint.is_empty()).then(|| panel().children(lint.into_iter().map(|l| small(l)))))
            .child(actions)
            .child(versions)
    }
}

fn mono(t: String) -> Div {
    div().font_family("monospace").text_size(px(13.)).p_3().rounded_md().bg(rgb(BG)).child(SharedString::from(t))
}

fn md_blocks(t: &str) -> Div {
    crate::md_view::blocks(&markdown::parse(t))
}
