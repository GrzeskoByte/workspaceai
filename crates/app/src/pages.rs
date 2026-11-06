//! The Settings pages that need their own state: web search, workspaces and members, vaults, skills, artifacts,
//! Open Design, data, audit, appearance, the engine connection, other secrets, editing a provider, and the
//! workspace instructions editor. `AppView` lives in `main.rs`; this module is its child, so it can use its fields.

use crate::text_input::TextInput;
use crate::ui;
use crate::{AppView, Msg, ACCENT, BG, MUTED, PANEL, RAISED_OFF, USER_BUBBLE};
use gpui::{div, prelude::*, px, rgb, Context, Div, Entity, FontWeight, SharedString};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};
use workspaceai_client::api::Api;
use workspaceai_client::providers;
use workspaceai_client::settings;
use workspaceai_client::types::{AppSettings, AuditEvent, Project, Provider};
use workspaceai_client::workspace::{self as wsx, DesignSystem, OdState};

/// State of the pages in this module.
pub struct Extra {
    pub instr: Entity<TextInput>,
    web_url: Entity<TextInput>,
    web_check: String,
    web_preview: String,
    invite: Entity<TextInput>,
    vault_name: Entity<TextInput>,
    vault_path: Entity<TextInput>,
    skill_folder: Entity<TextInput>,
    audit_q: Entity<TextInput>,
    audit_all: Option<Vec<AuditEvent>>,
    archived: Vec<Project>,
    eng_url: Entity<TextInput>,
    eng_user: Entity<TextInput>,
    eng_pass: Entity<TextInput>,
    eng_ver: Entity<TextInput>,
    eng_log: String,
    od: Option<OdState>,
    od_systems: Vec<DesignSystem>,
    od_filter: Entity<TextInput>,
    od_log: String,
    od_polled: Option<Instant>,
    secret_name: Entity<TextInput>,
    edit: Option<(String, [Entity<TextInput>; 4])>,
    pub app: AppSettings,
}

impl Extra {
    pub fn new(cx: &mut Context<AppView>) -> Self {
        let make = |cx: &mut Context<AppView>, ph: &str, masked: bool| {
            let i = cx.new(|cx| if masked { TextInput::new_masked(cx, ph) } else { TextInput::new(cx, ph) });
            cx.observe(&i, |_, _, cx| cx.notify()).detach();
            i
        };
        let instr = {
            let i = cx.new(|cx| TextInput::new_multiline(cx, "Rules every project in this workspace should follow…"));
            cx.observe(&i, |_, _, cx| cx.notify()).detach();
            i
        };
        Extra {
            instr,
            web_url: make(cx, "https://searx.example.org", false),
            web_check: String::new(),
            web_preview: String::new(),
            invite: make(cx, "name@example.com", false),
            vault_name: make(cx, "Name (optional)", false),
            vault_path: make(cx, "~/Documents/My Vault", false),
            skill_folder: make(cx, "~/skills/my-skill", false),
            audit_q: make(cx, "Filter (e.g. policy, provider, export)", false),
            audit_all: None,
            archived: Vec::new(),
            eng_url: make(cx, "https://host:4096", false),
            eng_user: make(cx, "Username", false),
            eng_pass: make(cx, "Password (kept in the keychain)", true),
            eng_ver: make(cx, "e.g. 2.2.1", false),
            eng_log: String::new(),
            od: None,
            od_systems: Vec::new(),
            od_filter: make(cx, "Filter design systems…", false),
            od_log: String::new(),
            od_polled: None,
            secret_name: make(cx, "Secret name, e.g. GITHUB_TOKEN", false),
            edit: None,
            app: AppSettings::default(),
        }
    }
}

/// Runs a request on a thread. A reply with `kind` is handed to the window; a failure becomes a toast (and, for
/// `kind`, a reply with `ok: false`). `then` is read afterwards, and the snapshot is always refreshed.
fn call(api: Api, ws: String, method: &'static str, path: String, body: Option<serde_json::Value>, kind: &'static str, then: Option<(&'static str, String)>, tx: Sender<Msg>) {
    crate::spawn(move || {
        match api.request(method, &path, body.as_ref()) {
            Ok(v) if !kind.is_empty() => {
                let _ = tx.send(Msg::Fetched(kind, ws.clone(), v));
            }
            Ok(_) => {}
            Err(e) => {
                let _ = tx.send(Msg::Error(e.to_string()));
                if !kind.is_empty() {
                    let _ = tx.send(Msg::Fetched(kind, ws.clone(), serde_json::json!({ "ok": false, "error": e.to_string() })));
                }
            }
        }
        if let Some((k, p)) = then {
            if let Ok(v) = api.get(&p) {
                let _ = tx.send(Msg::Fetched(k, ws.clone(), v));
            }
        }
        crate::load_snapshot(api, ws, tx);
    });
}

pub(crate) fn chips<T: Clone + 'static>(cx: &mut Context<AppView>, id: &str, opts: Vec<(String, T)>, active: impl Fn(&T) -> bool, locked: bool, pick: impl Fn(&mut AppView, T) + Clone + 'static) -> Div {
    let mut d = div().flex().flex_wrap().gap_2().justify_end();
    for (i, (label, v)) in opts.into_iter().enumerate() {
        let b = ui::button(format!("{id}-{i}"), label, if active(&v) { ACCENT } else { RAISED_OFF });
        d = d.child(if locked {
            b.opacity(0.5).into_any_element()
        } else {
            let pick = pick.clone();
            b.on_click(cx.listener(move |this, _, _, cx| {
                pick(this, v.clone());
                cx.notify();
            }))
            .into_any_element()
        });
    }
    d
}

fn field(input: &Entity<TextInput>, w: f32) -> Div {
    div().w(px(w)).px_2().py_1().rounded_md().bg(rgb(BG)).child(input.clone())
}

fn pre(id: &'static str, text: &str, max_h: f32) -> impl IntoElement {
    div().id(id).max_h(px(max_h)).overflow_y_scroll().p_3().rounded_md().bg(rgb(BG)).font_family("monospace").text_size(px(12.)).child(SharedString::from(text.to_string()))
}

fn card(title: &str) -> Div {
    div().flex().flex_col().gap_2().p_3().rounded_lg().bg(rgb(PANEL)).child(div().font_weight(FontWeight::BOLD).child(SharedString::from(title.to_string())))
}

pub(crate) fn heading(text: &str) -> Div {
    div().text_size(px(18.)).font_weight(FontWeight::BOLD).mt_3().child(SharedString::from(text.to_string()))
}

fn when_text(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default()
}

impl AppView {
    fn ws_api(&self) -> Option<(Api, String)> {
        Some((self.api.clone()?, self.model.active_ws.clone()?))
    }

    pub(crate) fn call(&mut self, method: &'static str, path: String, body: Option<serde_json::Value>, kind: &'static str, then: Option<(&'static str, String)>) {
        if let Some((api, ws)) = self.ws_api() {
            call(api, ws, method, path, body, kind, then, self.tx.clone());
        }
    }

    pub(crate) fn get_into(&mut self, kind: &'static str, path: String) {
        if let Some((api, ws)) = self.ws_api() {
            let tx = self.tx.clone();
            crate::spawn(move || match api.get(&path) {
                Ok(v) => {
                    let _ = tx.send(Msg::Fetched(kind, ws, v));
                }
                Err(e) if kind == "od" => {
                    let _ = tx.send(Msg::Error(e.to_string()));
                }
                Err(_) => {}
            });
        }
    }

    /// Fills the page's inputs and fetches what it shows. Called when a page is opened.
    pub fn open_page(&mut self, id: &str, cx: &mut Context<Self>) {
        let w = self.model.snapshot.workspace.clone();
        match id {
            "general" => {
                self.extra.instr.update(cx, |i, cx| i.set_content(&w.instructions, cx));
            }
            "web" => {
                self.extra.web_url.update(cx, |i, cx| i.set_content(&w.web.searxng_url, cx));
                self.extra.web_check.clear();
            }
            "engine" => {
                self.extra.eng_url.update(cx, |i, cx| i.set_content(&w.engine.url, cx));
                self.extra.eng_user.update(cx, |i, cx| i.set_content(&w.engine.username, cx));
                let v = if w.engine.version == "system" { "" } else { w.engine.version.as_str() };
                self.extra.eng_ver.update(cx, |i, cx| i.set_content(v, cx));
                self.extra.eng_log.clear();
            }
            "opendesign" => self.extra.od_log.clear(),
            "audit" => self.extra.audit_all = None,
            _ => {}
        }
        self.refresh_page(id);
    }

    /// Fetches what the page shows (no input changes), also after a snapshot arrives.
    pub fn refresh_page(&mut self, id: &str) {
        let Some(ws) = self.model.active_ws.clone() else { return };
        match id {
            "web" => self.get_into("web-preview", wsx::web_preview_path(&ws)),
            "data" => self.get_into("archived", wsx::archived_path(&ws)),
            "opendesign" => {
                self.get_into("od", wsx::od_path(&ws, ""));
                self.get_into("od-systems", wsx::design_systems_path(&ws));
            }
            "providers" => {}
            _ => {}
        }
    }

    pub fn fetched(&mut self, kind: &'static str, v: serde_json::Value) {
        if kind.starts_with("flow-") {
            return self.flow_fetched(kind, v);
        }
        if kind.starts_with("side-") {
            return self.side_fetched(kind, v);
        }
        let x = &mut self.extra;
        match kind {
            "web-preview" => x.web_preview = v["text"].as_str().unwrap_or("").to_string(),
            "web-check" => x.web_check = wsx::check_text(&v),
            "archived" => x.archived = serde_json::from_value(v["projects"].clone()).unwrap_or_default(),
            "audit" => x.audit_all = serde_json::from_value(v["events"].clone()).ok(),
            "od" => {
                x.od = serde_json::from_value(v).ok();
                x.od_polled = Some(Instant::now());
            }
            "od-systems" => x.od_systems = wsx::parse_design_systems(&v),
            "od-log" => x.od_log = wsx::log_text(&v, 120),
            "eng-log" => x.eng_log = wsx::log_text(&v, 120),
            _ => {}
        }
    }

    /// While Open Design is installing or starting, ask again about every 1.2 s.
    pub fn poll(&mut self) {
        if self.view == crate::View::Settings && self.settings_page == "opendesign" && self.extra.od.as_ref().is_some_and(|o| o.busy()) && self.extra.od_polled.is_none_or(|t| t.elapsed() > Duration::from_millis(1200)) {
            self.extra.od_polled = Some(Instant::now());
            if let Some(ws) = self.model.active_ws.clone() {
                self.get_into("od", wsx::od_path(&ws, ""));
            }
        }
    }

    pub fn start_edit_provider(&mut self, p: &Provider, cx: &mut Context<Self>) {
        let ph = ["Name", "Base URL, https://…", "Model ids, comma separated", "New API key (empty keeps the stored one)"];
        let inputs: [Entity<TextInput>; 4] = std::array::from_fn(|i| {
            let input = cx.new(|cx| if i == 3 { TextInput::new_masked(cx, ph[i]) } else { TextInput::new(cx, ph[i]) });
            cx.observe(&input, |_, _, cx| cx.notify()).detach();
            input
        });
        let start = [p.name.clone(), p.base_url.clone().unwrap_or_default(), wsx::models_csv(p)];
        for (i, v) in inputs.iter().zip(start) {
            i.update(cx, |i, cx| i.set_content(&v, cx));
        }
        self.extra.edit = Some((p.id.clone(), inputs));
    }

    /// The provider edit form, when open.
    pub fn provider_edit_card(&self, cx: &mut Context<Self>) -> Option<Div> {
        let (id, inputs) = self.extra.edit.clone()?;
        let v: Vec<String> = inputs.iter().map(|i| i.read(cx).content().to_string()).collect();
        let body = wsx::edit_provider_body(&v[0], &v[1], &v[2], &v[3]);
        let ws = self.model.active_ws.clone().unwrap_or_default();
        let mut c = card(&format!("Edit {id}"));
        for i in &inputs {
            c = c.child(div().px_2().py_1().rounded_md().bg(rgb(BG)).child(i.clone()));
        }
        if let Err(why) = &body {
            c = c.child(div().text_sm().text_color(rgb(MUTED)).child(*why));
        }
        let ok = body.is_ok();
        Some(c.child(
            div()
                .flex()
                .gap_2()
                .child(ui::button("pe-save", "Save", if ok { ACCENT } else { RAISED_OFF }).on_click(cx.listener(move |this, _, _, cx| {
                    if let Ok(b) = &body {
                        this.mutate_path("PATCH", providers::provider_path(&ws, &id), Some(b.clone()));
                        this.extra.edit = None;
                    }
                    cx.notify();
                })))
                .child(ui::button("pe-cancel", "Cancel", USER_BUBBLE).on_click(cx.listener(|this, _, _, cx| {
                    this.extra.edit = None;
                    cx.notify();
                }))),
        ))
    }

    /// Other secrets (not tied to a provider): name, the shared masked field for the value, list with Remove.
    pub fn secrets_card(&self, cx: &mut Context<Self>) -> Div {
        let ws = self.model.active_ws.clone().unwrap_or_default();
        let known: Vec<String> = self.model.snapshot.workspace.providers.iter().filter_map(|p| p.key_env.as_deref().and_then(providers::secret_name)).collect();
        let mut c = card("Other secrets").child(div().text_sm().text_color(rgb(MUTED)).child("Named values for connectors and tools, kept in the system keychain. Type a name, paste the value in the API key field above, then Save secret."));
        let ws_s = ws.clone();
        c = c.child(
            div().flex().gap_2().child(field(&self.extra.secret_name, 300.)).child(ui::button("sec-save", "Save secret", ACCENT).on_click(cx.listener(move |this, _, window, cx| {
                let name = this.extra.secret_name.read(cx).content().to_string();
                let value = this.secret_input.read(cx).content().to_string();
                if let (Some(n), Some(b), Some((api, _))) = (providers::secret_name(name.trim()), providers::secret_body(&value), this.ws_api()) {
                    crate::change_secret(api, ws_s.clone(), "PUT", providers::secret_path(&ws_s, &n), Some(b), this.tx.clone());
                    this.extra.secret_name.update(cx, |i, cx| i.clear(window, cx));
                    this.secret_input.update(cx, |i, cx| i.clear(window, cx));
                } else {
                    this.model.toast = Some("Give the secret a name and paste its value first.".into());
                }
                cx.notify();
            }))),
        );
        for n in self.secret_names.iter().filter(|n| !known.contains(n)) {
            let (n_rm, ws_rm) = (n.clone(), ws.clone());
            let armed = self.confirm.is_armed(&format!("sec:{n}"));
            c = c.child(ui::row(n, "Stored in the keychain", ui::button(format!("sec-rm-{n}"), if armed { "Click again to remove" } else { "Remove" }, if armed { 0x7a2b2b } else { ui::DANGER }).on_click(cx.listener(move |this, _, _, cx| {
                if this.confirm.press(&format!("sec:{n_rm}")) {
                    if let Some((api, _)) = this.ws_api() {
                        crate::change_secret(api, ws_rm.clone(), "DELETE", providers::secret_path(&ws_rm, &n_rm), None, this.tx.clone());
                    }
                }
                cx.notify();
            }))));
        }
        c
    }

    /// Workspace instructions: a multi-line editor with Save and Revert.
    pub fn instructions_card(&self, cx: &mut Context<Self>) -> Div {
        let saved = self.model.snapshot.workspace.instructions.clone();
        let text = self.extra.instr.read(cx).content().to_string();
        let dirty = text != saved;
        let revert = saved.clone();
        card("Workspace instructions")
            .child(div().text_sm().text_color(rgb(MUTED)).child("The broadest layer; every project inherits it."))
            .child(div().px_2().py_1().rounded_md().bg(rgb(BG)).text_size(px(14.)).child(self.extra.instr.clone()))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(ui::button("instr-save", "Save", if dirty { ACCENT } else { RAISED_OFF }).on_click(cx.listener(move |this, _, _, cx| {
                        if dirty {
                            let t = this.extra.instr.read(cx).content().to_string();
                            this.patch_ws(settings::patch("instructions", t.into()));
                        }
                        cx.notify();
                    })))
                    .child(ui::button("instr-revert", "Revert", USER_BUBBLE).on_click(cx.listener(move |this, _, _, cx| {
                        this.extra.instr.update(cx, |i, cx| i.set_content(&revert, cx));
                        cx.notify();
                    }))),
            )
    }

    pub fn accent_row(&self, cx: &mut Context<Self>) -> Div {
        let cur = self.model.snapshot.workspace.accent.clone();
        let opts = wsx::ACCENTS.iter().map(|(id, n)| (n.to_string(), id.to_string())).collect();
        let chips = chips(cx, "accent", opts, move |id| *id == cur, false, |this, id| this.patch_ws(settings::patch("accent", id.into())));
        ui::row("Accent", "Top strip, rail and buttons in the web app", chips)
    }

    /// Pages from this module; any other id gets the placeholder.
    pub fn extra_page(&self, id: &str, page: Div, locked: bool, cx: &mut Context<Self>) -> Div {
        let w = &self.model.snapshot.workspace;
        let ws = self.model.active_ws.clone().unwrap_or_default();
        let is_org = w.kind == "org";
        let switch = |cx: &mut Context<Self>, key: String, on: bool, locked: bool, f: fn(&mut AppView, bool)| {
            let sw = ui::switch(format!("sw-{key}"), on, locked);
            if locked {
                sw
            } else {
                sw.on_click(cx.listener(move |this, _, _, cx| {
                    f(this, !on);
                    cx.notify();
                }))
            }
        };
        match id {
            "web" => {
                let web = &w.web;
                let modes = wsx::WEB_MODES.iter().map(|(m, l)| (l.to_string(), m.to_string())).collect();
                let cur = web.mode.clone();
                let mut p = page.child(ui::row(
                    "Search",
                    "opencode websearch, an MCP search server, or none",
                    chips(cx, "web-mode", modes, move |m| *m == cur, locked, |this, m| {
                        this.patch_ws(wsx::nested("web", "mode", m.clone().into()));
                        this.model.toast = Some(wsx::web_toast(&m));
                    }),
                ));
                if web.mode == "mcp" {
                    let cur = web.provider.clone();
                    let opts = wsx::WEB_PROVIDERS.iter().map(|x| (x.to_string(), x.to_string())).collect();
                    p = p.child(ui::row("Provider", "Runs as a local MCP server; key kept in the keychain", chips(cx, "web-prov", opts, move |x| *x == cur, locked, |this, x| this.patch_ws(wsx::nested("web", "provider", x.into())))));
                }
                if web.mode == "searxng" {
                    p = p.child(ui::row(
                        "SearXNG URL",
                        "Your instance",
                        div().flex().gap_2().child(field(&self.extra.web_url, 260.)).child(ui::button("web-url-save", "Save", ACCENT).on_click(cx.listener(move |this, _, _, cx| {
                            if !locked {
                                let u = this.extra.web_url.read(cx).content().trim().to_string();
                                this.patch_ws(wsx::nested("web", "searxngUrl", u.into()));
                            }
                            cx.notify();
                        }))),
                    ));
                }
                p = p
                    .child(ui::row("Read pages by URL", "opencode webfetch", switch(cx, "web-fetch".into(), web.fetch, locked, |t, on| t.patch_ws(wsx::nested("web", "fetch", on.into())))))
                    .child(ui::row("Ask before each search", "Every search needs your approval", switch(cx, "web-ask".into(), web.ask, false, |t, on| t.patch_ws(wsx::nested("web", "ask", on.into())))))
                    .child(ui::row("Show sources under answers", "Links to every page the agent read", switch(cx, "web-src".into(), web.show_sources, false, |t, on| t.patch_ws(wsx::nested("web", "showSources", on.into())))));
                let off = web.mode == "off";
                let body = wsx::web_test_body(&web.mode, &web.provider, &web.searxng_url);
                p = p.child(ui::row(
                    "Try a search",
                    if off { "Search is off" } else if self.extra.web_check.is_empty() { "Checks the configured backend" } else { &self.extra.web_check },
                    ui::button("web-check", "Check", if off { RAISED_OFF } else { ACCENT }).on_click(cx.listener(move |this, _, _, cx| {
                        if !off {
                            this.extra.web_check = "Checking…".into();
                            this.call("POST", wsx::WEB_TEST_PATH.into(), Some(body.clone()), "web-check", None);
                        }
                        cx.notify();
                    })),
                ));
                p.child(card("Written to this workspace's opencode").child(pre("web-preview", &self.extra.web_preview, 240.)))
            }
            "members" => {
                if w.kind == "personal" {
                    return page.child(ui::row("Members", "A personal workspace has one member", div().text_color(rgb(MUTED)).child("Only you")))
                            .child(div().text_color(rgb(MUTED)).child("To collaborate, create a team workspace or join an organization with SSO. That flow is not in the native app yet; use the web app for it."));
                }
                let mut p = page;
                if is_org {
                    if let Some(o) = &w.org {
                        p = p.child(ui::row("Identity provider", &o.issuer, div().font_family("monospace").text_sm().child(SharedString::from(format!("{} · {}", if o.idp.is_empty() { "OIDC" } else { &o.idp }, o.subject)))));
                    }
                    p = p.child(ui::row("Require SSO", "Set by your organization", ui::switch("sw-sso", true, true)));
                }
                for (i, m) in w.members.iter().enumerate() {
                    let roles = wsx::ROLES.iter().map(|r| (r.to_string(), r.to_string())).collect();
                    let cur = m.role.clone();
                    let members = w.members.clone();
                    p = p.child(ui::row(
                        &m.name,
                        m.email.as_deref().unwrap_or("member"),
                        chips(cx, &format!("role-{i}"), roles, move |r| *r == cur, i == 0 || locked, move |this, r| {
                            if let Some(b) = wsx::role_body(&members, i, &r) {
                                this.patch_ws(b);
                            }
                        }),
                    ));
                }
                let members = w.members.clone();
                p.child(ui::row(
                    "Invite",
                    if is_org { "By email; they sign in with SSO" } else { "By email; they get their own copy of the workspace settings" },
                    div().flex().gap_2().child(field(&self.extra.invite, 240.)).child(ui::button("invite", "Invite", ACCENT).on_click(cx.listener(move |this, _, window, cx| {
                        let t = this.extra.invite.read(cx).content().to_string();
                        if let Some(b) = wsx::invite_body(&members, &t) {
                            this.patch_ws(b);
                            this.extra.invite.update(cx, |i, cx| i.clear(window, cx));
                        }
                        cx.notify();
                    }))),
                ))
            }
            "vaults" => {
                let mut p = page;
                for v in &w.vaults {
                    let armed = self.confirm.is_armed(&format!("vault:{}", v.id));
                    let (vaults, id) = (w.vaults.clone(), v.id.clone());
                    p = p.child(ui::row(
                        &v.name,
                        &v.path,
                        ui::button(format!("vault-rm-{}", v.id), if armed { "Click again to remove" } else { "Remove" }, if armed { 0x7a2b2b } else { ui::DANGER }).on_click(cx.listener(move |this, _, _, cx| {
                            if this.confirm.press(&format!("vault:{id}")) {
                                this.patch_ws(wsx::remove_vault_body(&vaults, &id));
                            }
                            cx.notify();
                        })),
                    ));
                }
                if w.vaults.is_empty() {
                    p = p.child(div().text_color(rgb(MUTED)).child("No vaults yet. Add your vault's folder, then attach folders of it to projects."));
                }
                let vaults = w.vaults.clone();
                p.child(
                    card("Add a vault")
                        .child(div().px_2().py_1().rounded_md().bg(rgb(BG)).child(self.extra.vault_name.clone()))
                        .child(div().px_2().py_1().rounded_md().bg(rgb(BG)).child(self.extra.vault_path.clone()))
                        .child(ui::button("vault-add", "Add vault", ACCENT).on_click(cx.listener(move |this, _, window, cx| {
                            let (n, pth) = (this.extra.vault_name.read(cx).content().to_string(), this.extra.vault_path.read(cx).content().to_string());
                            if let Some(b) = wsx::add_vault_body(&vaults, &n, &pth) {
                                this.patch_ws(b);
                                this.extra.vault_name.update(cx, |i, cx| i.clear(window, cx));
                                this.extra.vault_path.update(cx, |i, cx| i.clear(window, cx));
                            }
                            cx.notify();
                        }))),
                )
                .child(div().p_3().rounded_lg().border_1().border_color(rgb(USER_BUBBLE)).text_sm().child("Agents can read every Markdown note in these vaults, and edit only the folders attached to their project. Hidden folders such as .obsidian are off limits."))
                .child(div().text_sm().text_color(rgb(MUTED)).child("The harness enforces this on its own vault tools. Agents with shell or file tools of their own can go around it, so it is not a security boundary against them. Do not attach notes those agents must never see."))
            }
            "skills" => {
                let mut p = page;
                for s in &w.skills {
                    let (skills, sid) = (w.skills.clone(), s.id.clone());
                    let sw = ui::switch(format!("sw-skill-{}", s.id), s.on, false).on_click(cx.listener(move |this, _, _, cx| {
                        this.patch_ws(wsx::toggle_skill_body(&skills, &sid));
                        cx.notify();
                    }));
                    p = p.child(ui::row(&s.name, &s.note, sw));
                }
                let skills = w.skills.clone();
                p.child(ui::row(
                    "Add a skill folder",
                    "A folder with a SKILL.md; opencode and Open Design both read it",
                    div().flex().gap_2().child(field(&self.extra.skill_folder, 240.)).child(ui::button("skill-add", "Add", ACCENT).on_click(cx.listener(move |this, _, window, cx| {
                        let f = this.extra.skill_folder.read(cx).content().to_string();
                        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
                        if let Some(b) = wsx::add_skill_body(&skills, &f, now) {
                            this.patch_ws(b);
                            this.extra.skill_folder.update(cx, |i, cx| i.clear(window, cx));
                        }
                        cx.notify();
                    }))),
                ))
            }
            "artifacts" => {
                let blocked = is_org && self.model.snapshot.policy.as_ref().is_some_and(|p| p.block_external_sharing);
                let vis = w.visibility.clone();
                let od_on = w.open_design.as_ref().map_or(true, |o| o.engine != "builtin");
                page.child(ui::row(
                    "Default visibility",
                    "Who can see new artifacts",
                    chips(cx, "vis", wsx::VISIBILITY.iter().map(|v| (v.to_string(), v.to_string())).collect(), move |v| *v == vis, false, |this, v| this.patch_ws(settings::patch("visibility", v.into()))),
                ))
                .child(ui::row(
                    "Allow public links",
                    if blocked { "Blocked by org policy" } else { "Anyone with the link can view a static export" },
                    switch(cx, "public".into(), w.public_links && !blocked, blocked || locked, |t, on| t.patch_ws(settings::patch("publicLinks", on.into()))),
                ))
                .child(ui::row(
                    "Artifact engine",
                    if od_on { "Open Design saves, versions and lints every artifact" } else { "Built-in store: no lint, no PDF or PowerPoint export through Open Design" },
                    div().flex().items_center().gap_2().child(if od_on { "Open Design" } else { "Built-in" }).child(ui::button("art-manage", "Manage", USER_BUBBLE).on_click(cx.listener(|this, _, _, cx| {
                        this.settings_page = "opendesign";
                        this.open_page("opendesign", cx);
                        cx.notify();
                    }))),
                ))
                .child(ui::row("Design system", "Used for new artifacts. Choose it in Open Design.", div().text_color(rgb(MUTED)).child(SharedString::from(w.design_system.clone()))))
            }
            "opendesign" => self.open_design_page(page, cx),
            "data" => {
                let keep = w.keep_chats.clone();
                let mut p = page
                    .child(ui::row("Data folder", "", div().font_family("monospace").text_sm().child(SharedString::from(format!("~/.harness/ws/{}", w.id)))))
                    .child(ui::row(
                        "Keep chats",
                        if is_org { "Set by org policy" } else { "" },
                        chips(cx, "keep", wsx::KEEP_CHATS.iter().map(|v| (v.to_string(), v.to_string())).collect(), move |v| *v == keep, locked, |this, v| this.patch_ws(settings::patch("keepChats", v.into()))),
                    ))
                    .child(ui::row("Export this workspace", "Chats, artifacts, memory and config as .tar.gz — secrets stay in the keychain. Saved to your Downloads folder.", ui::button("export-ws", "Export", ACCENT).on_click(cx.listener(|this, _, _, cx| {
                        this.export_workspace();
                        cx.notify();
                    }))))
                    .child(heading("Archived projects"));
                for pr in &self.extra.archived {
                    let (id_r, id_d, ws_r, ws_d) = (pr.id.clone(), pr.id.clone(), ws.clone(), ws.clone());
                    let armed = self.confirm.is_armed(&format!("arch:{}", pr.id));
                    p = p.child(ui::row(
                        &pr.name,
                        "",
                        div()
                            .flex()
                            .gap_2()
                            .child(ui::button(format!("arch-restore-{}", pr.id), "Restore", ACCENT).on_click(cx.listener(move |this, _, _, cx| {
                                this.call("POST", workspaceai_client::projects::restore_path(&ws_r, &id_r), None, "", Some(("archived", wsx::archived_path(&ws_r))));
                                cx.notify();
                            })))
                            .child(ui::button(format!("arch-del-{}", pr.id), if armed { "Click again to delete" } else { "Delete" }, if armed { 0x7a2b2b } else { ui::DANGER }).on_click(cx.listener(move |this, _, _, cx| {
                                if this.confirm.press(&format!("arch:{id_d}")) {
                                    this.call("DELETE", workspaceai_client::projects::item_path(&ws_d, &id_d), None, "", Some(("archived", wsx::archived_path(&ws_d))));
                                }
                                cx.notify();
                            }))),
                    ));
                }
                if self.extra.archived.is_empty() {
                    p = p.child(div().text_color(rgb(MUTED)).child("Nothing archived."));
                }
                p
            }
            "audit" => {
                let all: &[AuditEvent] = self.extra.audit_all.as_deref().unwrap_or(&self.model.snapshot.audit);
                let q = self.extra.audit_q.read(cx).content().to_string();
                let shown = wsx::filter_audit(all, &q);
                let ws_l = ws.clone();
                let mut list = div().flex().flex_col().rounded_lg().bg(rgb(PANEL)).py_1();
                for (i, e) in shown.iter().enumerate() {
                    list = list.child(
                        div()
                            .id(SharedString::from(format!("audit-{i}")))
                            .flex()
                            .gap_3()
                            .px_3()
                            .py(px(4.))
                            .text_sm()
                            .child(div().w(px(130.)).text_color(rgb(MUTED)).child(SharedString::from(when_text(e.at))))
                            .child(div().w(px(150.)).font_weight(FontWeight::BOLD).child(SharedString::from(e.action.clone())))
                            .child(div().flex_1().min_w_0().child(SharedString::from(e.detail.clone())))
                            .child(div().text_color(rgb(MUTED)).child(SharedString::from(e.actor.clone()))),
                    );
                }
                if shown.is_empty() {
                    list = list.child(div().px_3().py_1().text_color(rgb(MUTED)).child("Nothing yet. Actions you take appear here."));
                }
                page.child(div().flex().gap_2().child(div().flex_1().px_2().py_1().rounded_md().bg(rgb(PANEL)).child(self.extra.audit_q.clone())).child(ui::button("audit-all", "Load all", USER_BUBBLE).on_click(cx.listener(move |this, _, _, cx| {
                    this.get_into("audit", wsx::audit_path(&ws_l, 1000));
                    cx.notify();
                })))).child(list)
            }
            "appearance" => {
                let s = &self.extra.app;
                let theme = s.theme.clone();
                let accent = s.accent.clone();
                let mut p = page.child(div().text_sm().text_color(rgb(MUTED)).child("The native window has one look for now. These choices are saved to the app settings and used by the web app."));
                p = p.child(ui::row(
                    "Theme",
                    "",
                    chips(cx, "theme", vec![("Claude".into(), "claude".to_string()), ("opencode".into(), "opencode".to_string())], move |t| *t == theme, false, |this, t| this.mutate_computer("PATCH", wsx::APP_SETTINGS_PATH.into(), Some(wsx::theme_body(&t)))),
                ));
                let sw_follow = ui::switch("sw-follow", s.follow_system, false).on_click(cx.listener({
                    let on = s.follow_system;
                    move |this, _, _, cx| {
                        this.mutate_computer("PATCH", wsx::APP_SETTINGS_PATH.into(), Some(settings::patch("followSystem", (!on).into())));
                        cx.notify();
                    }
                }));
                let sw_motion = ui::switch("sw-motion", s.reduce_motion, false).on_click(cx.listener({
                    let on = s.reduce_motion;
                    move |this, _, _, cx| {
                        this.mutate_computer("PATCH", wsx::APP_SETTINGS_PATH.into(), Some(settings::patch("reduceMotion", (!on).into())));
                        cx.notify();
                    }
                }));
                p.child(ui::row("Follow system light and dark", "Claude when light, opencode when dark", sw_follow)).child(ui::row(
                    "Accent",
                    "Default accent; each workspace can set its own in General",
                    chips(cx, "app-accent", wsx::ACCENTS.iter().map(|(i, n)| (n.to_string(), i.to_string())).collect(), move |a| *a == accent, false, |this, a| this.mutate_computer("PATCH", wsx::APP_SETTINGS_PATH.into(), Some(settings::patch("accent", a.into())))),
                ))
                .child(ui::row("Reduce motion", "Also follows your system setting", sw_motion))
            }
            _ => page.child(div().p_4().rounded_lg().bg(rgb(PANEL)).text_color(rgb(MUTED)).child("This page is not in the native app yet. Use the web app for it for now.")),
        }
    }

    /// Extra rows of the Engine page: connection mode, logs, version pin, sync.
    pub fn engine_more(&self, page: Div, locked: bool, cx: &mut Context<Self>) -> Div {
        let w = &self.model.snapshot.workspace;
        let ws = self.model.active_ws.clone().unwrap_or_default();
        let locked = locked || w.kind == "org";
        let mode = w.engine.mode.clone();
        let ws_m = ws.clone();
        let ws_c = ws.clone();
        let ws_l = ws.clone();
        let ws_v = ws.clone();
        let current = if w.engine.version.is_empty() { "system".to_string() } else { w.engine.version.clone() };
        let running = self.model.snapshot.engine.version.clone();
        let sync = w.engine.sync;
        page.child(ui::row(
            "opencode",
            "Start a server for this workspace, or use one you run",
            chips(cx, "eng-mode", vec![("Managed by WorkspaceAI".into(), "managed".to_string()), ("Connect to a server".into(), "remote".to_string())], move |m| *m == mode || (mode.is_empty() && m == "managed"), locked, move |this, m| {
                if m == "managed" {
                    this.call("POST", wsx::engine_path(&ws_m, "mode"), Some(wsx::managed_body()), "", None);
                } else {
                    this.model.toast = Some("Fill in the server below, then press Connect.".into());
                }
            }),
        ))
        .child(
            card("Server you run")
                .child(div().flex().gap_2().child(field(&self.extra.eng_url, 300.)).child(field(&self.extra.eng_user, 140.)))
                .child(field(&self.extra.eng_pass, 300.))
                .child(div().text_sm().text_color(rgb(MUTED)).child("Connecting requires a successful test. Remote servers must use HTTPS or an SSH tunnel to localhost; the password is kept in the keychain."))
                .child(ui::button("eng-connect", if w.engine.mode == "remote" { "Reconnect" } else { "Test & connect" }, if locked { RAISED_OFF } else { ACCENT }).on_click(cx.listener(move |this, _, window, cx| {
                    let (u, n, p) = (this.extra.eng_url.read(cx).content().to_string(), this.extra.eng_user.read(cx).content().to_string(), this.extra.eng_pass.read(cx).content().to_string());
                    match (locked, wsx::remote_body(&u, &n, &p)) {
                        (false, Some(b)) => {
                            this.call("POST", wsx::engine_path(&ws_c, "mode"), Some(b), "", None);
                            this.extra.eng_pass.update(cx, |i, cx| i.clear(window, cx));
                        }
                        (false, None) => this.model.toast = Some("The server URL must start with http:// or https://.".into()),
                        _ => {}
                    }
                    cx.notify();
                }))),
        )
        .child(ui::row(
            "Logs",
            "The last 120 lines of opencode's log",
            ui::button("eng-logs", "Show logs", USER_BUBBLE).on_click(cx.listener(move |this, _, _, cx| {
                this.get_into("eng-log", wsx::engine_path(&ws_l, "logs"));
                cx.notify();
            })),
        ))
        .children((!self.extra.eng_log.is_empty()).then(|| pre("eng-log", &self.extra.eng_log, 280.)))
        .child(ui::row(
            "opencode version",
            &wsx::version_note(&current, &running, locked),
            div()
                .flex()
                .gap_2()
                .child(field(&self.extra.eng_ver, 110.))
                .child(ui::button("eng-pin", "Pin", if locked { RAISED_OFF } else { ACCENT }).on_click(cx.listener(move |this, _, _, cx| {
                    let v = this.extra.eng_ver.read(cx).content().to_string();
                    if let (false, Some(b)) = (locked, wsx::version_body(&v)) {
                        this.model.toast = Some("Installing opencode… this can take a minute.".into());
                        this.call("POST", wsx::engine_path(&ws_v, "version"), Some(b), "", None);
                    }
                    cx.notify();
                })))
                .child(ui::button("eng-system", "Use system", USER_BUBBLE).on_click(cx.listener({
                    let ws = ws.clone();
                    move |this, _, _, cx| {
                        if !locked {
                            this.call("POST", wsx::engine_path(&ws, "version"), wsx::version_body("system"), "", None);
                        }
                        cx.notify();
                    }
                }))),
        ))
        .child(ui::row("Sync sessions both ways", "TUI sessions appear here; chats here appear in the TUI", {
            let sw = ui::switch("sw-sync", sync, locked);
            if locked {
                sw
            } else {
                sw.on_click(cx.listener(move |this, _, _, cx| {
                    this.patch_ws(wsx::nested("engine", "sync", (!sync).into()));
                    cx.notify();
                }))
            }
        }))
    }

    fn open_design_page(&self, page: Div, cx: &mut Context<Self>) -> Div {
        let Some(st) = self.extra.od.clone() else {
            return page.child(div().text_color(rgb(MUTED)).child("Checking Open Design…"));
        };
        let w = &self.model.snapshot.workspace;
        let ws = self.model.active_ws.clone().unwrap_or_default();
        let o = st.settings.clone();
        let state = st.daemon.state.clone();
        let mut p = page;
        if o.engine == "open-design" && !st.install.installed {
            p = p.child(div().p_3().rounded_lg().border_1().border_color(rgb(ACCENT)).text_sm().child("Open Design is this workspace's artifact engine but is not installed yet, so agents cannot save artifacts. Install it below, or switch to the built-in store."));
        }
        let note = st.daemon.error.clone().unwrap_or_else(|| match (state.as_str(), &st.daemon.url) {
            ("running", Some(u)) => format!("{u} · pid {}", st.daemon.pid.unwrap_or(0)),
            ("stopped", _) => (if o.auto_start { "Starts with the workspace engine or the first artifact" } else { "Starts with the first artifact" }).into(),
            _ => String::new(),
        });
        p = p.child(ui::row("Status", &note, div().font_weight(FontWeight::BOLD).child(wsx::daemon_label(&state))));
        let (ws_i, ws_u) = (ws.clone(), ws.clone());
        p = p.child(if st.install.installed {
            ui::row(
                "Installed",
                &st.install.error.clone().unwrap_or_default(),
                div().flex().items_center().gap_2().child(SharedString::from(format!("v{}", st.install.version.clone().unwrap_or_default()))).child(
                    ui::button("od-update", if st.install.installing { format!("Updating… {}%", st.install.progress) } else { "Update".to_string() }, USER_BUBBLE).on_click(cx.listener(move |this, _, _, cx| {
                        this.call("POST", wsx::OD_UPDATE_PATH.into(), None, "", Some(("od", wsx::od_path(&ws_u, ""))));
                        cx.notify();
                    })),
                ),
            )
        } else {
            ui::row(
                "Not installed",
                &if st.install.installing { format!("Installing… {}%", st.install.progress) } else { st.install.error.clone().unwrap_or_else(|| "Downloads Open Design to this computer.".into()) },
                ui::button("od-install", "Install", ACCENT).on_click(cx.listener(move |this, _, _, cx| {
                    this.call("POST", wsx::OD_INSTALL_PATH.into(), None, "", Some(("od", wsx::od_path(&ws_i, ""))));
                    cx.notify();
                })),
            )
        });
        let mut daemon = div().flex().gap_2();
        for (label, verb, enabled) in [("Start", "start", st.install.installed && state != "running" && state != "starting"), ("Restart", "restart", st.install.installed && state != "starting"), ("Stop", "stop", state == "running")] {
            let ws_d = ws.clone();
            daemon = daemon.child(ui::button(format!("od-{verb}"), label, if enabled { ACCENT } else { RAISED_OFF }).on_click(cx.listener(move |this, _, _, cx| {
                if enabled {
                    this.call("POST", wsx::od_path(&ws_d, verb), None, "", Some(("od", wsx::od_path(&ws_d, ""))));
                }
                cx.notify();
            })));
        }
        let ws_l = ws.clone();
        daemon = daemon.child(ui::button("od-logs", "Logs", USER_BUBBLE).on_click(cx.listener(move |this, _, _, cx| {
            this.get_into("od-log", wsx::od_path(&ws_l, "logs"));
            cx.notify();
        })));
        p = p.child(ui::row("Daemon", "One per workspace, on 127.0.0.1 only", daemon));
        if !self.extra.od_log.is_empty() {
            p = p.child(pre("od-log", &self.extra.od_log, 260.));
        }
        let engine = o.engine.clone();
        let ws_e = ws.clone();
        p = p.child(ui::row(
            "Artifact engine",
            if o.engine == "open-design" { "Every artifact is saved, versioned and linted by Open Design; a save fails if it cannot be" } else { "WorkspaceAI store only: no lint, no PDF or PowerPoint export through Open Design" },
            chips(cx, "od-engine", vec![("Open Design".into(), "open-design".to_string()), ("Built-in store".into(), "builtin".to_string())], move |e| *e == engine, false, move |this, e| {
                this.patch_ws(wsx::nested("openDesign", "engine", e.into()));
                this.get_into("od", wsx::od_path(&ws_e, ""));
            }),
        ));
        for (key, label, note, on, field_name, off) in [
            ("auto", "Start with the workspace", "Otherwise it starts on the first artifact save", o.auto_start, "autoStart", false),
            ("lint", "Lint every version", "Open Design's artifact checks; findings show on the artifact", o.lint, "lint", o.engine == "builtin"),
            ("tools", "Open Design tools for agents", "Also give agents Open Design's own MCP tools. Artifacts still go through artifact_create. Restarts the engine.", o.agent_tools, "agentTools", false),
        ] {
            let ws_s = ws.clone();
            let sw = ui::switch(format!("sw-od-{key}"), on, off);
            let sw = if off {
                sw
            } else {
                sw.on_click(cx.listener(move |this, _, _, cx| {
                    this.patch_ws(wsx::nested("openDesign", field_name, (!on).into()));
                    this.get_into("od", wsx::od_path(&ws_s, ""));
                    cx.notify();
                }))
            };
            p = p.child(ui::row(label, note, sw));
        }
        let missing = st.missing();
        let ws_r = ws.clone();
        p = p.child(ui::row(
            "Artifacts",
            &format!("{} of {} in Open Design · {} lint finding{} · {} Open Design project{}", st.artifacts.in_open_design, st.artifacts.total, st.artifacts.lint_findings, if st.artifacts.lint_findings == 1 { "" } else { "s" }, st.projects, if st.projects == 1 { "" } else { "s" }),
            ui::button("od-resync", if missing > 0 { format!("Resync {missing}") } else { "All synced".to_string() }, if missing > 0 && st.install.installed { ACCENT } else { RAISED_OFF }).on_click(cx.listener(move |this, _, _, cx| {
                if missing > 0 {
                    this.call("POST", wsx::od_path(&ws_r, "resync"), None, "", Some(("od", wsx::od_path(&ws_r, ""))));
                }
                cx.notify();
            })),
        ));
        let q = self.extra.od_filter.read(cx).content().to_string();
        let mut ds = card("Design system for new artifacts").child(div().text_sm().text_color(rgb(MUTED)).child(SharedString::from(if state == "running" { format!("{} available", self.extra.od_systems.len()) } else { "Start Open Design to see its design systems".to_string() })));
        if self.extra.od_systems.len() > 8 {
            ds = ds.child(div().px_2().py_1().rounded_md().bg(rgb(BG)).child(self.extra.od_filter.clone()));
        }
        let chosen = w.design_system.clone();
        for d in wsx::visible_systems(&self.extra.od_systems, &chosen, &q) {
            let title = d.title.clone();
            ds = ds.child(
                div()
                    .id(SharedString::from(format!("ds-{}", d.id)))
                    .p_2()
                    .rounded_md()
                    .cursor_pointer()
                    .border_1()
                    .border_color(rgb(if d.title == chosen { ACCENT } else { PANEL }))
                    .hover(|s| s.bg(rgb(USER_BUBBLE)))
                    .child(div().font_weight(FontWeight::MEDIUM).child(SharedString::from(d.title.clone())))
                    .child(div().text_sm().text_color(rgb(MUTED)).child(SharedString::from(d.summary.clone())))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.patch_ws(settings::patch("designSystem", title.clone().into()));
                        cx.notify();
                    })),
            );
        }
        p.child(ds)
            .child(ui::row("Data folder", "Open Design's own data for this workspace", div().font_family("monospace").text_sm().child(SharedString::from(st.data_dir.clone()))))
            .child(ui::row("Artifact files", "Each project's artifacts live in its folder", div().font_family("monospace").text_sm().child("<project>/.harness/artifacts")))
    }
}

impl AppView {
    /// General: default project and default model, as chip lists.
    pub fn default_pickers(&self, cx: &mut Context<Self>) -> Div {
        let w = &self.model.snapshot.workspace;
        let cur_project = w.default_project.clone();
        let projects: Vec<(String, String)> = workspaceai_client::projects::tree(&self.model.snapshot.projects, false).into_iter().map(|n| (format!("{}{}", "  ".repeat(n.depth), n.name), n.id)).collect();
        let cur_model = w.default_model.clone();
        let listed: Vec<String> = self.models.options_for("opencode").into_iter().map(|o| o.id).collect();
        let models: Vec<(String, String)> = workspaceai_client::spaces::model_choices(&cur_model, &listed).into_iter().map(|m| (m.clone(), m)).collect();
        let note_model = if w.kind == "org" { "Only models your organization allows" } else { "Used when a project or chat does not pick one" };
        let scroll = |id: &'static str, d: Div| div().id(id).max_h(px(150.)).overflow_y_scroll().child(d);
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(ui::row("Default project", "Where quick chats go — no chat is left outside a project", scroll("pick-project", chips(cx, "defproj", projects, move |v| *v == cur_project, false, |this, v| this.patch_ws(settings::patch("defaultProject", v.into()))))))
            .child(ui::row(
                "Default model",
                note_model,
                scroll("pick-model", chips(cx, "defmodel", models, move |v| *v == cur_model, false, |this, v| {
                    this.model.toast = Some(workspaceai_client::spaces::model_toast(&v));
                    this.patch_ws(settings::patch("defaultModel", v.into()));
                })),
            ))
    }

    /// General: delete the workspace (or leave the organization), two-step.
    pub fn remove_row(&self, cx: &mut Context<Self>) -> Div {
        use workspaceai_client::spaces as sp;
        let kind = self.model.snapshot.workspace.kind.clone();
        let allowed = sp::can_delete(&kind, &self.model.workspaces);
        let armed = self.confirm.is_armed("remove-ws");
        let b = ui::button("remove-ws", sp::remove_button(&kind, armed), if armed { 0x7a2b2b } else { ui::DANGER });
        let b = if allowed {
            b.on_click(cx.listener(|this, _, _, cx| {
                if this.confirm.press("remove-ws") {
                    this.remove_workspace();
                }
                cx.notify();
            }))
        } else {
            b.opacity(0.5)
        };
        ui::row(sp::remove_label(&kind), sp::remove_note(&kind, allowed), b)
    }
}
