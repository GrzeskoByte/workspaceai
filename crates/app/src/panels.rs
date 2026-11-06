//! The artifact panel beside the chat (and its full-screen view) and the browser panel, after `apps/ui`'s
//! ArtifactPanel / ArtifactFullscreen / BrowserPanel. Pages show in the embedded web view (`webview.rs`); the
//! request and page logic is in `workspaceai_client::web`. A child of the crate root, like `flow.rs`.

use crate::onboarding::{btn, small};
use crate::text_input::TextInput;
use crate::webview::{Event, Source, Web};
use crate::{AppView, View, ACCENT, BG, MUTED, PANEL, RAISED_OFF, USER_BUBBLE};
use gpui::{canvas, div, prelude::*, px, rgb, AnyElement, Context, Div, Entity, FontWeight, SharedString};
use serde_json::Value;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};
use workspaceai_client::artifacts;
use workspaceai_client::library::{self, Body};
use workspaceai_client::markdown;
use workspaceai_client::types::{Artifact, ChatMessage};
use workspaceai_client::web;

pub struct Side {
    /// The Library's selected artifact is open beside the chat.
    pub art_open: bool,
    pub wide: bool,
    pub full: bool,
    /// Whether the window was put into native full screen for `full` (applied in render, which has the window).
    native_full: bool,
    pub mobile: bool,
    /// The browser panel is open at this address ("" before one is picked).
    pub browser: Option<String>,
    pub browser_wide: bool,
    pub address: Entity<TextInput>,
    dev_cmd: Entity<TextInput>,
    dev: Option<web::Dev>,
    dev_project: String,
    want_dev_url: bool,
    show_log: bool,
    last_poll: Instant,
    pending_address: Option<String>,
    pending_cmd: Option<String>,
    /// One web view for artifacts (panel, full screen, Library, New artifact), one for the browser panel, so the
    /// browser keeps its history while an artifact is shown.
    pub art: Rc<RefCell<Web>>,
    pub br: Rc<RefCell<Web>>,
}

impl Side {
    pub fn new(cx: &mut Context<AppView>) -> Side {
        let address = cx.new(|cx| TextInput::new(cx, "localhost:5173, a port, or any address"));
        let dev_cmd = cx.new(|cx| TextInput::new(cx, "Command that starts the dev server"));
        cx.observe(&dev_cmd, |_, _, cx| cx.notify()).detach();
        Side {
            art_open: false,
            wide: false,
            full: false,
            native_full: false,
            mobile: false,
            browser: None,
            browser_wide: false,
            address,
            dev_cmd,
            dev: None,
            dev_project: String::new(),
            want_dev_url: false,
            show_log: false,
            last_poll: Instant::now(),
            pending_address: None,
            pending_cmd: None,
            art: Rc::new(RefCell::new(Web::new())),
            br: Rc::new(RefCell::new(Web::new())),
        }
    }
}

fn icon_btn(cx: &mut Context<AppView>, id: &str, glyph: &str, enabled: bool, f: impl Fn(&mut AppView, &mut Context<AppView>) + 'static) -> AnyElement {
    btn(cx, id, glyph, RAISED_OFF, enabled, f)
}

fn tag(t: String) -> Div {
    div().px(px(6.)).py(px(2.)).rounded_sm().bg(rgb(USER_BUBBLE)).font_family("monospace").text_size(px(10.5)).text_color(rgb(ACCENT)).child(SharedString::from(t))
}

impl AppView {
    // ---------- web view plumbing ----------

    /// Every 16 ms: GTK work, hiding web views no slot wanted, and what the pages asked for.
    #[cfg_attr(not(feature = "webview"), allow(dead_code))]
    pub fn web_tick(&mut self, cx: &mut Context<Self>) {
        crate::webview::pump();
        let (art, br) = (self.side.art.borrow_mut().tick(), self.side.br.borrow_mut().tick());
        for (e, from_browser) in art.into_iter().map(|e| (e, false)).chain(br.into_iter().map(|e| (e, true))) {
            match e {
                Event::Leave if !from_browser => self.side.full = false,
                Event::Loaded(u) if from_browser && self.side.browser.is_some() && u.starts_with("http") => {
                    self.side.browser = Some(u.clone());
                    self.side.pending_address = Some(u);
                }
                Event::Popup(u) if u.starts_with("http://") || u.starts_with("https://") => cx.open_url(&u),
                _ => {}
            }
            cx.notify();
        }
    }

    /// Start of render: slots painted in this frame claim their web view again; full screen follows `full`.
    pub fn web_begin_frame(&mut self, window: &mut gpui::Window) {
        self.side.art.borrow_mut().begin_frame();
        self.side.br.borrow_mut().begin_frame();
        let full = self.side.full && self.library_sel.is_some();
        if full != self.side.native_full {
            self.side.native_full = full;
            if window.is_fullscreen() != full {
                window.toggle_fullscreen();
            }
            if full {
                self.side.art.borrow().focus();
            }
        }
    }

    /// An overlay or dialog drawn above the content: the native web view would cover it, so it stays hidden.
    fn web_covered(&self) -> bool {
        self.palette_open || self.onb.nw_kind.is_some()
    }

    /// The box the web view is laid over. Clipped to the scroll area it sits in; hidden when fully scrolled away.
    pub fn web_slot(&self, which: &Rc<RefCell<Web>>, src: Source) -> AnyElement {
        if let Some(why) = which.borrow().unavailable() {
            return div().size_full().p_4().rounded_lg().bg(rgb(PANEL)).text_color(rgb(MUTED)).child(SharedString::from(why)).into_any_element();
        }
        if self.web_covered() {
            return div().size_full().bg(rgb(BG)).into_any_element();
        }
        let web = which.clone();
        canvas(
            move |bounds, window, _| {
                let b = bounds.intersect(&window.content_mask().bounds);
                if b.size.width > px(1.) && b.size.height > px(1.) {
                    web.borrow_mut().place(window, b, &src);
                }
            },
            |_, _, _, _| {},
        )
        .size_full()
        .into_any_element()
    }

    /// The artifact as a host page for the web view, or `None` when it is drawn natively.
    pub fn art_source(&self, kind: &str, v: &library::View, page: &str) -> Option<Source> {
        let doc = web::web_doc(&v.format, kind, &v.content, page)?;
        Some(Source::Html(web::host_page(&doc, kind == "Deck", self.side.mobile && web::has_device_toggle(kind))))
    }

    pub fn device_toggle(&self, kind: &str, cx: &mut Context<Self>) -> Div {
        let mut d = div().flex().gap_1();
        if web::has_device_toggle(kind) {
            let m = self.side.mobile;
            d = d
                .child(btn(cx, "dev-desktop", "Desktop", if m { RAISED_OFF } else { ACCENT }, true, |this, _| this.side.mobile = false))
                .child(btn(cx, "dev-mobile", "Mobile", if m { ACCENT } else { RAISED_OFF }, true, |this, _| this.side.mobile = true));
        }
        d
    }

    // ---------- artifact panel ----------

    /// Opens an artifact beside the chat (from a card under a reply).
    pub fn open_artifact_panel(&mut self, id: String) {
        self.select_artifact(id);
        self.side.art_open = true;
        self.side.full = false;
    }

    fn close_artifact_panel(&mut self) {
        self.side.art_open = false;
        self.side.wide = false;
        self.side.full = false;
    }

    /// What the selected artifact shows, plus its versions/export/share/delete, for the panel and the Library.
    pub fn artifact_body(&self, a: &Artifact, cx: &mut Context<Self>) -> AnyElement {
        let extras = self.library_extras(a, cx);
        match &self.library_view {
            None => div().text_color(rgb(MUTED)).child(SharedString::from(format!("Opening {}…", a.title))).into_any_element(),
            Some(Err(e)) => div().id("art-scroll").flex_1().min_h_0().overflow_y_scroll().flex().flex_col().gap_3().child(div().text_color(rgb(0xff6b6b)).child(SharedString::from(e.clone()))).child(extras).into_any_element(),
            Some(Ok(v)) => match self.art_source(&a.kind, v, &self.library_page) {
                Some(src) => {
                    let view = if self.side.full { div().size_full().p_4().text_color(rgb(MUTED)).child("Shown full screen · Esc to leave").into_any_element() } else { self.web_slot(&self.side.art, src) };
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h_0()
                        .gap_2()
                        .child(div().flex().gap_2().items_center().child(self.device_toggle(&a.kind, cx)).child(div().flex_1()).child(btn(cx, "art-full-2", "⛶ Full screen", RAISED_OFF, true, |this, _| this.side.full = true)))
                        .child(div().flex_1().min_h(px(240.)).rounded_lg().overflow_hidden().border_1().border_color(rgb(USER_BUBBLE)).child(view))
                        .child(div().id("art-details").max_h(px(260.)).overflow_y_scroll().child(extras))
                        .into_any_element()
                }
                None => div().id("art-scroll").flex_1().min_h_0().overflow_y_scroll().flex().flex_col().gap_3().child(native_body(&a.kind, v)).child(extras).into_any_element(),
            },
        }
    }

    fn artifact_panel(&self, a: &Artifact, cx: &mut Context<Self>) -> Div {
        let snap = &self.model.snapshot;
        let format = self.library_view.as_ref().and_then(|r| r.as_ref().ok()).map_or("markdown", |v| v.format.as_str());
        let shown = self.flow_lib_ver().unwrap_or(a.version);
        let project = snap.projects.iter().find(|p| p.id == a.project).map_or("—".to_string(), |p| p.name.clone());
        let from = a.chat.as_ref().and_then(|c| snap.chats.iter().find(|x| &x.id == c));
        let (pid, wide) = (a.project.clone(), self.side.wide);
        let mut links = div().flex().gap_2().text_size(px(11.)).child(div().id("art-proj").cursor_pointer().text_color(rgb(MUTED)).child(SharedString::from(format!("▤ {project}"))).on_click(cx.listener(move |this, _, _, cx| {
            this.open_project(pid.clone());
            cx.notify();
        })));
        if let Some(c) = from {
            let cid = c.id.clone();
            links = links.child(div().id("art-from").cursor_pointer().text_color(rgb(MUTED)).child(SharedString::from(format!("· from 💬 {}", if c.title.is_empty() { "New chat" } else { &c.title }))).on_click(cx.listener(move |this, _, _, cx| {
                this.view = View::Chat;
                this.open_chat(cid.clone());
                cx.notify();
            })));
        }
        let header = div()
            .flex()
            .items_center()
            .gap_2()
            .px(px(14.))
            .py(px(10.))
            .border_b_1()
            .border_color(rgb(USER_BUBBLE))
            .child(tag(a.kind.to_uppercase()))
            .child(div().font_family("monospace").text_size(px(10.5)).text_color(rgb(MUTED)).child(artifacts::format_label(format)))
            .child(div().flex().flex_col().flex_1().min_w_0().child(div().font_weight(FontWeight::BOLD).text_size(px(13.)).child(SharedString::from(a.title.clone()))).child(links))
            .child(div().px_2().rounded_md().bg(rgb(USER_BUBBLE)).font_family("monospace").text_size(px(11.)).child(SharedString::from(format!("v{shown} of {}", a.version))))
            .child(icon_btn(cx, "art-full", "⛶", true, |this, _| this.side.full = true))
            .child(icon_btn(cx, "art-wide", if wide { "⇤" } else { "⤢" }, true, |this, _| this.side.wide = !this.side.wide))
            .child(icon_btn(cx, "art-close", "✕", true, |this, _| this.close_artifact_panel()));
        div().flex().flex_col().h_full().min_w_0().bg(rgb(PANEL)).border_l_1().border_color(rgb(USER_BUBBLE)).child(header).child(div().flex().flex_col().flex_1().min_h_0().p_4().child(self.artifact_body(a, cx)))
    }

    /// The selected artifact filling the window, with a slim bar to leave (Esc or the button).
    pub fn artifact_fullscreen(&self, cx: &mut Context<Self>) -> Option<Div> {
        if !self.side.full {
            return None;
        }
        let a = self.library_sel.as_ref().and_then(|id| self.model.snapshot.artifacts.iter().find(|a| &a.id == id))?;
        let v = self.library_view.as_ref()?.as_ref().ok()?;
        let meta = format!("{} · {} · v{}", a.kind, artifacts::format_label(&v.format), self.flow_lib_ver().unwrap_or(a.version));
        let body = match self.art_source(&a.kind, v, &self.library_page) {
            Some(src) => self.web_slot(&self.side.art, src),
            None => div().id("full-doc").size_full().overflow_y_scroll().flex().justify_center().p(px(40.)).child(div().max_w(px(820.)).child(native_body(&a.kind, v))).into_any_element(),
        };
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .bg(rgb(BG))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .px_4()
                        .py_2()
                        .bg(rgb(PANEL))
                        .child(div().font_weight(FontWeight::BOLD).child(SharedString::from(a.title.clone())))
                        .child(div().font_family("monospace").text_size(px(11.)).text_color(rgb(MUTED)).child(SharedString::from(meta)))
                        .child(div().flex_1())
                        .child(small("Esc to leave"))
                        .child(btn(cx, "full-exit", "✕ Exit full screen", RAISED_OFF, true, |this, _| this.side.full = false)),
                )
                .child(div().flex_1().min_h_0().child(body)),
        )
    }

    // ---------- chat additions ----------

    /// Under a reply: a card for the artifact it made, and "Preview localhost:…" for a local address in it.
    pub fn message_extras(&self, msg: &ChatMessage, cx: &mut Context<Self>) -> Option<Div> {
        if msg.role == "user" {
            return None;
        }
        let art = msg.artifact.as_ref().and_then(|id| self.model.snapshot.artifacts.iter().find(|a| &a.id == id));
        let url = web::local_url_in(&msg.text).or_else(|| msg.tools.iter().find_map(|t| t.output.as_deref().and_then(web::local_url_in)));
        if art.is_none() && url.is_none() {
            return None;
        }
        let mut d = div().flex().flex_col().gap_2().mt_2();
        if let Some(a) = art {
            let id = a.id.clone();
            d = d.child(
                div()
                    .id(SharedString::from(format!("card-{}-{}", msg.id, a.id)))
                    .flex()
                    .items_center()
                    .gap_3()
                    .max_w(px(440.))
                    .p_3()
                    .rounded_lg()
                    .cursor_pointer()
                    .bg(rgb(PANEL))
                    .hover(|s| s.bg(rgb(USER_BUBBLE)))
                    .child(tag(a.kind.to_uppercase()))
                    .child(div().flex().flex_col().flex_1().child(div().font_weight(FontWeight::BOLD).text_size(px(13.)).child(SharedString::from(a.title.clone()))).child(small(format!("{} · v{}", a.kind, a.version))))
                    .child(div().text_color(rgb(ACCENT)).text_size(px(12.)).child("Open"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_artifact_panel(id.clone());
                        cx.notify();
                    })),
            );
        }
        if let Some(u) = url {
            let label = format!("▣ Preview {}", web::short_url(&u));
            d = d.child(div().flex().child(btn(cx, &format!("preview-{}", msg.id), &label, RAISED_OFF, true, move |this, _| this.open_browser(Some(u.clone())))));
        }
        Some(d)
    }

    /// The panel beside the chat: an open artifact covers the browser; closing it shows the browser again.
    pub fn side_panel(&self, cx: &mut Context<Self>) -> Option<(AnyElement, bool)> {
        if self.side.art_open {
            if let Some(a) = self.library_sel.as_ref().and_then(|id| self.model.snapshot.artifacts.iter().find(|a| &a.id == id)) {
                let wide = self.side.wide;
                let p = self.artifact_panel(a, cx);
                return Some((if wide { p.flex_1() } else { p.w(px(520.)).flex_none() }.into_any_element(), wide));
            }
        }
        self.side.browser.as_ref()?;
        let wide = self.side.browser_wide;
        let p = self.browser_panel(cx);
        Some((if wide { p.flex_1() } else { p.w(px(560.)).flex_none() }.into_any_element(), wide))
    }

    // ---------- browser panel ----------

    fn side_project(&self) -> String {
        let open = self.model.open_chat_id.as_ref().and_then(|c| self.model.snapshot.chats.iter().find(|x| &x.id == c)).map(|c| c.project.clone());
        open.or_else(|| self.model.snapshot.projects.iter().find(|p| !p.archived).map(|p| p.id.clone())).unwrap_or_default()
    }

    pub fn open_browser(&mut self, url: Option<String>) {
        self.side.art_open = false;
        self.side.full = false;
        let url = url.map(|u| web::to_url(&u)).unwrap_or_else(|| self.side.browser.clone().unwrap_or_default());
        self.side.pending_address = Some(url.clone());
        self.side.browser = Some(url);
    }

    pub fn toggle_browser(&mut self) {
        if self.side.browser.is_some() && !self.side.art_open {
            self.side.browser = None;
            self.side.browser_wide = false;
        } else {
            self.open_browser(None);
        }
    }

    pub fn browse(&mut self, raw: &str) {
        let u = web::to_url(raw);
        if !u.is_empty() {
            self.open_browser(Some(u));
        }
    }

    fn dev_refresh(&mut self) {
        let pid = self.side_project();
        if !pid.is_empty() {
            self.get_into("side-dev", web::dev_path(&self.ws_id(), &pid));
        }
    }

    pub fn side_fetched(&mut self, kind: &'static str, v: Value) {
        match kind {
            "side-dev" => {
                let Some(d) = web::parse_dev(&v) else { return };
                if self.side.want_dev_url && !d.url.is_empty() {
                    self.side.want_dev_url = false;
                    self.open_browser(Some(d.url.clone()));
                } else if self.side.want_dev_url && !d.running && d.exit.is_some() {
                    self.side.want_dev_url = false;
                }
                if self.side.dev.is_none() {
                    self.side.pending_cmd = Some(d.command.clone());
                }
                self.side.dev = Some(d);
            }
            "side-dev-changed" => {
                if v["ok"] == false {
                    self.side.want_dev_url = false;
                }
                self.dev_refresh();
            }
            _ => {}
        }
    }

    /// From the 50 ms pump: input updates that need the context, and polling the dev server while it runs.
    pub fn side_pump(&mut self, cx: &mut Context<Self>) {
        if let Some(u) = self.side.pending_address.take() {
            self.side.address.update(cx, |i, cx| i.set_content(&u, cx));
        }
        if let Some(c) = self.side.pending_cmd.take() {
            self.side.dev_cmd.update(cx, |i, cx| i.set_content(&c, cx));
        }
        if self.side.browser.is_none() {
            return;
        }
        let pid = self.side_project();
        if pid != self.side.dev_project {
            self.side.dev_project = pid;
            self.side.dev = None;
            self.side.want_dev_url = false;
            self.dev_refresh();
        }
        let polling = self.side.want_dev_url || self.side.dev.as_ref().is_some_and(|d| d.running);
        if polling && self.side.last_poll.elapsed() >= Duration::from_millis(1500) {
            self.side.last_poll = Instant::now();
            self.dev_refresh();
        }
    }

    fn browser_panel(&self, cx: &mut Context<Self>) -> Div {
        let url = self.side.browser.clone().unwrap_or_default();
        let has = !url.is_empty();
        let wide = self.side.browser_wide;
        let open_url = url.clone();
        let mut bar = div()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .py_2()
            .border_b_1()
            .border_color(rgb(USER_BUBBLE))
            .child(icon_btn(cx, "br-back", "‹", has, |this, _| this.side.br.borrow().script("history.back()")))
            .child(icon_btn(cx, "br-fwd", "›", has, |this, _| this.side.br.borrow().script("history.forward()")))
            .child(icon_btn(cx, "br-reload", "⟳", has, |this, _| this.side.br.borrow().reload()))
            .child(div().flex_1().min_w_0().px_2().py_1().rounded_md().bg(rgb(BG)).font_family("monospace").text_size(px(12.5)).child(self.side.address.clone()));
        if has {
            bar = bar.child(icon_btn(cx, "br-ext", "↗", true, move |_, cx| cx.open_url(&open_url)));
        }
        bar = bar
            .child(icon_btn(cx, "br-wide", if wide { "⇤" } else { "⤢" }, true, |this, _| this.side.browser_wide = !this.side.browser_wide))
            .child(icon_btn(cx, "br-close", "✕", true, |this, _| {
                this.side.browser = None;
                this.side.browser_wide = false;
            }));

        let dev = self.side.dev.as_ref();
        let running = dev.is_some_and(|d| d.running);
        let typed = self.side.dev_cmd.read(cx).content().to_string();
        let ws = self.ws_id();
        let pid = self.side_project();
        let dev_row = div()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .child(div().size(px(8.)).rounded_full().bg(rgb(if running { 0x7fd88f } else { 0x606060 })))
            .child(div().flex_1().min_w_0().px_2().py_1().rounded_md().bg(rgb(BG)).font_family("monospace").text_size(px(12.)).child(self.side.dev_cmd.clone()))
            .child(if running {
                let (ws, pid) = (ws.clone(), pid.clone());
                btn(cx, "dev-stop", "Stop", RAISED_OFF, true, move |this, _| this.call("POST", web::dev_path(&ws, &pid), Some(web::stop_body()), "side-dev-changed", None))
            } else {
                let (ws, pid, cmd) = (ws.clone(), pid.clone(), typed.clone());
                btn(cx, "dev-run", "Run", ACCENT, !typed.trim().is_empty() && !pid.is_empty(), move |this, _| {
                    if let Some(body) = web::start_body(&cmd) {
                        this.side.want_dev_url = true;
                        this.call("POST", web::dev_path(&ws, &pid), Some(body), "side-dev-changed", None);
                    }
                })
            })
            .when(dev.is_some_and(|d| !d.log.is_empty()), |d| d.child(btn(cx, "dev-log", "Log", if self.side.show_log { ACCENT } else { RAISED_OFF }, true, |this, _| this.side.show_log = !this.side.show_log)));
        let project = self.model.snapshot.projects.iter().find(|p| p.id == pid).map_or("Project".to_string(), |p| p.name.clone());
        let chips = dev.map(web::server_chips).unwrap_or_default();
        let servers = (!chips.is_empty()).then(|| {
            let mut row = div().flex().flex_wrap().items_center().gap_1().px_2().pb_1().child(small(project.clone()));
            for (i, (u, label)) in chips.into_iter().enumerate() {
                let on = url.starts_with(u.trim_end_matches('/'));
                row = row.child(btn(cx, &format!("srv-{i}"), &label, if on { ACCENT } else { RAISED_OFF }, true, move |this, _| this.open_browser(Some(u.clone()))));
            }
            row
        });
        let log = (self.side.show_log && dev.is_some()).then(|| {
            div().id("dev-log-text").mx_2().mb_1().p_2().max_h(px(160.)).overflow_y_scroll().rounded_md().bg(rgb(BG)).font_family("monospace").text_size(px(11.)).child(SharedString::from(dev.map(|d| d.log.join("\n")).unwrap_or_default()))
        });
        let view = if has {
            self.web_slot(&self.side.br, Source::Url(url))
        } else {
            let hint = match dev.filter(|d| !d.command.is_empty()) {
                Some(d) => format!("Run {} in {project} and its page opens here, or type an address above.", d.command),
                None => format!("Set the command that starts {project}’s dev server above, or type an address — a port like 5173 works."),
            };
            div().size_full().flex().flex_col().items_center().justify_center().gap_2().p_4().child(div().font_weight(FontWeight::BOLD).child("Preview what you’re building")).child(small(hint)).into_any_element()
        };
        div().flex().flex_col().h_full().min_w_0().bg(rgb(PANEL)).border_l_1().border_color(rgb(USER_BUBBLE)).child(bar).child(dev_row).children(servers).children(log).child(div().flex_1().min_h_0().child(view))
    }
}

/// Markdown and CSV/JSON/SVG text, drawn natively.
fn native_body(kind: &str, v: &library::View) -> AnyElement {
    match library::body_for(&v.format, &v.content) {
        Body::Markdown(t) if kind != "Deck" => crate::md_view::blocks(&markdown::parse(&t)).into_any_element(),
        Body::Markdown(t) | Body::Text(t) => div().font_family("monospace").text_size(px(13.)).p_3().rounded_md().bg(rgb(BG)).child(SharedString::from(t)).into_any_element(),
        Body::Unsupported(why) => div().p_4().rounded_lg().bg(rgb(PANEL)).text_color(rgb(MUTED)).child(SharedString::from(why)).into_any_element(),
    }
}
