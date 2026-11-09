//! Sessions are terminals: opening one fetches the supervisor's launch spec and runs it in a `TerminalView` in the
//! main pane. Views stay alive across switches (keyed by workspace and session) and end when the session is
//! deleted, restarted, or the app quits. A child of the crate root, like `flow.rs`.

use crate::terminal_view::{Status, TermViewEvent, TerminalView};
use crate::{patch_chat, spawn, AppView, Msg, View, ACCENT, MUTED, PANEL, USER_BUBBLE};
use gpui::{div, prelude::*, px, rgb, rgba, Context, Div, Entity, ExternalPaths, Focusable, FontWeight, PathPromptOptions, SharedString, Window};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use workspaceai_client::api::seg;
use workspaceai_client::chats::{self, Launch};
use workspaceai_client::files;
use workspaceai_client::projects;
use workspaceai_client::web;
use workspaceai_term::{Size, SpawnOptions};

type Key = (String, String);

pub struct Live {
    pub view: Entity<TerminalView>,
    pub status: Status,
}

#[derive(Default)]
pub struct Terms {
    pub live: HashMap<Key, Live>,
    starting: HashSet<Key>,
    failed: HashMap<Key, String>,
    last_open: HashMap<String, String>,
    /// Sessions whose title came from the terminal; a rename by the user takes them out.
    auto_title: HashSet<String>,
    title_due: Option<(Instant, Key, String)>,
    /// Full access was clicked on this session and waits for the warning to be confirmed.
    warn_full: Option<String>,
    /// Focus the open terminal on the next frame.
    focus: bool,
}

impl Terms {
    pub fn status_of(&self, ws: &str, chat: &str) -> Option<Status> {
        self.live.get(&(ws.to_string(), chat.to_string())).map(|l| l.status)
    }

    pub fn needs_you(&self, ws: &str) -> bool {
        self.live.iter().any(|((w, _), l)| w == ws && l.status == Status::NeedsYou)
    }

    /// Ends the session's process (the view's drop hangs the PTY up).
    pub fn end(&mut self, ws: &str, chat: &str) {
        self.live.remove(&(ws.to_string(), chat.to_string()));
        self.auto_title.remove(chat);
    }

    pub fn remember_user_title(&mut self, chat: &str) {
        self.auto_title.remove(chat);
    }
}

/// (colour, label) of the status dot.
pub fn dot(s: Status) -> (u32, String) {
    match s {
        Status::Working => (0x5fb36b, "working".into()),
        Status::NeedsYou => (ACCENT, "needs you".into()),
        Status::Idle => (MUTED, "idle".into()),
        Status::Exited(c) => (0xd96b6b, format!("exited {}", c.map_or("(signal)".to_string(), |c| c.to_string()))),
    }
}

pub fn dot_el(s: Status) -> Div {
    div().size(px(8.)).rounded_full().bg(rgb(dot(s).0))
}

impl AppView {
    fn key(&self, chat: &str) -> Key {
        (self.ws_id(), chat.to_string())
    }

    fn open_view(&self) -> Option<&Live> {
        let chat = self.model.open_chat_id.as_ref()?;
        self.terms.live.get(&self.key(chat))
    }

    pub fn open_chat(&mut self, id: String) {
        self.model.open_chat(id.clone());
        self.renaming = false;
        self.move_open = false;
        self.terms.warn_full = None;
        self.confirm.cancel();
        let ws = self.ws_id();
        self.terms.last_open.insert(ws.clone(), id.clone());
        self.terms.focus = true;
        self.launch(ws, id, None);
    }

    /// The session that was open the last time this workspace was shown, if its terminal still runs.
    pub fn restore_open(&mut self, ws: &str) {
        self.model.open_chat_id = self.terms.last_open.get(ws).filter(|c| self.terms.live.contains_key(&(ws.to_string(), (*c).clone()))).cloned();
    }

    /// Fetches the launch spec (after setting full access, when asked) unless a terminal already runs or is starting.
    fn launch(&mut self, ws: String, chat: String, full: Option<bool>) {
        let key = (ws.clone(), chat.clone());
        let Some(api) = self.api.clone() else { return };
        if self.terms.live.contains_key(&key) || !self.terms.starting.insert(key.clone()) {
            return;
        }
        self.terms.failed.remove(&key);
        let tx = self.tx.clone();
        spawn(move || {
            let r = (|| {
                if let Some(on) = full {
                    api.patch(&format!("/api/ws/{}/chats/{}", seg(&ws), seg(&chat)), &chats::full_access_body(on)).map_err(|e| e.to_string())?;
                }
                let v = api.get(&chats::launch_path(&ws, &chat)).map_err(|e| e.to_string())?;
                chats::parse_launch(&v).ok_or_else(|| "The supervisor sent no command to run.".to_string())
            })();
            let _ = tx.send(Msg::Launch(ws, chat, r));
        });
    }

    pub fn launched(&mut self, ws: String, chat: String, r: Result<Launch, String>, cx: &mut Context<Self>) {
        let key = (ws.clone(), chat.clone());
        self.terms.starting.remove(&key);
        let l = match r {
            Ok(l) => l,
            Err(e) => {
                self.terms.failed.insert(key, e);
                return;
            }
        };
        let opts = SpawnOptions { argv: l.argv, env: l.env.into_iter().collect(), cwd: l.cwd.map(PathBuf::from), size: Size::new(80, 24) };
        let view = cx.new(|cx| TerminalView::new(opts, cx));
        let k = key.clone();
        cx.subscribe(&view, move |this, _, e: &TermViewEvent, cx| {
            this.term_event(&k, e, cx);
            cx.notify();
        })
        .detach();
        self.terms.live.insert(key, Live { view, status: Status::Idle });
        self.terms.focus = true;
        self.model.needs.snapshot = true; // `launched` flipped on the server
    }

    fn term_event(&mut self, key: &Key, e: &TermViewEvent, cx: &mut Context<Self>) {
        match e {
            TermViewEvent::Status(s) => {
                if let Some(l) = self.terms.live.get_mut(key) {
                    l.status = *s;
                }
            }
            TermViewEvent::Title(Some(t)) => {
                let t: String = t.trim().chars().take(120).collect();
                let current = self.model.snapshot.chats.iter().find(|c| c.id == key.1).map(|c| c.title.clone());
                let auto = current.as_deref().is_some_and(|c| c.is_empty() || self.terms.auto_title.contains(&key.1));
                if auto && !t.is_empty() && current.as_deref() != Some(&t) {
                    self.terms.title_due = Some((Instant::now() + Duration::from_millis(1500), key.clone(), t));
                }
            }
            TermViewEvent::Title(None) => {}
            // Local addresses open in the browser panel; anything else only if it is http(s).
            TermViewEvent::OpenLink(url) => {
                if let Some(local) = web::local_url_in(url) {
                    self.view = View::Chat;
                    self.open_browser(Some(local));
                } else if url.starts_with("https://") || url.starts_with("http://") {
                    cx.open_url(url);
                }
            }
        }
    }

    /// From the 50 ms pump: the debounced title write, and what terminals asked of the clipboard.
    pub fn term_pump(&mut self, cx: &mut Context<Self>) {
        if self.terms.title_due.as_ref().is_some_and(|(at, _, _)| *at <= Instant::now()) {
            if let (Some((_, (ws, chat), title)), Some(api)) = (self.terms.title_due.take(), self.api.clone()) {
                if let (Some(body), true) = (chats::rename_body(&title), self.model.active_ws.as_deref() == Some(&ws)) {
                    if let Some(c) = self.model.snapshot.chats.iter_mut().find(|c| c.id == chat) {
                        c.title = title;
                    }
                    self.terms.auto_title.insert(chat.clone());
                    patch_chat(api, ws, chat, body, self.tx.clone());
                }
            }
        }
        let mut notices = Vec::new();
        for l in self.terms.live.values() {
            if let Some(n) = l.view.update(cx, |v, _| v.take_clipboard_notice()) {
                notices.push(n);
            }
        }
        for (bytes, applied) in notices {
            self.model.toast = Some(if applied { format!("Terminal copied {bytes} chars to the clipboard") } else { "Terminal tried to set the clipboard (blocked)".into() });
            cx.notify();
        }
    }

    /// Start of render: give the keyboard to the open terminal when a click or a new session asked for it.
    pub fn term_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.terms.focus || self.palette_open || self.view != View::Chat {
            return;
        }
        if let Some(l) = self.open_view() {
            let h = l.view.focus_handle(cx);
            window.focus(&h, cx);
            self.terms.focus = false;
        }
    }

    pub fn ask_focus(&mut self) {
        self.terms.focus = true;
    }

    /// Pastes the paths, shell-quoted, into the open terminal.
    pub fn attach_paths(&mut self, paths: Vec<String>, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        let Some(view) = self.open_view().map(|l| l.view.clone()) else { return };
        let text = files::quote_paths(&paths);
        view.update(cx, |v, cx| v.paste_text(&text, cx));
        self.terms.focus = true;
        cx.notify();
    }

    pub fn pick_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some("Attach".into()) });
        cx.spawn_in(window, async move |this, cx| {
            let result = picked.await;
            this.update(cx, |this, cx| match result {
                Ok(Ok(Some(paths))) => this.attach_paths(paths.iter().map(|p| p.to_string_lossy().into_owned()).collect(), cx),
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

    /// Relaunches the open session, setting full access first when `full` is given. The old process is hung up.
    fn relaunch(&mut self, full: Option<bool>) {
        let (ws, Some(id)) = (self.ws_id(), self.model.open_chat_id.clone()) else { return };
        self.terms.end(&ws, &id);
        if let (Some(on), Some(c)) = (full, self.model.snapshot.chats.iter_mut().find(|c| c.id == id)) {
            c.full_access = on;
        }
        self.terms.warn_full = None;
        self.launch(ws, id, full);
        self.terms.focus = true;
    }

    /// Ends a session's terminal and forgets it (archive, delete).
    pub fn close_chat(&mut self) {
        if let Some(id) = self.model.open_chat_id.take() {
            let ws = self.ws_id();
            self.terms.end(&ws, &id);
            self.terms.last_open.remove(&ws);
        }
        self.confirm.cancel();
    }

    /// The main pane: header, session actions, and the terminal.
    pub fn session_view(&self, cx: &mut Context<Self>) -> Div {
        let m = &self.model;
        let chat = self.open_chat_info().cloned();
        let key = chat.as_ref().map(|c| self.key(&c.id));
        let live = key.as_ref().and_then(|k| self.terms.live.get(k));
        let status = live.map(|l| l.status);
        let title = chat.as_ref().map(|c| if c.title.is_empty() { "New session".to_string() } else { c.title.clone() }).unwrap_or_else(|| "WorkspaceAI".to_string());

        let mut right = div().flex().items_center().gap_3();
        if let Some(s) = status {
            right = right.child(div().flex().items_center().gap_2().child(dot_el(s)).child(div().text_sm().text_color(rgb(MUTED)).child(SharedString::from(dot(s).1))));
        }
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(rgb(PANEL))
            .child(div().font_weight(FontWeight::BOLD).child(SharedString::from(title)))
            .child(right);

        let actions = chat.clone().map(|c| {
            let id = c.id.clone();
            let armed = self.confirm.is_armed(&id);
            let button = |key: &'static str, label: String, bg: u32| div().id(key).px_2().py_1().rounded_md().cursor_pointer().bg(rgb(bg)).text_sm().child(SharedString::from(label));
            let mut row = div()
                .flex()
                .flex_wrap()
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
                .child(button("act-attach", "Attach".into(), PANEL).on_click(cx.listener(|this, _, window, cx| this.pick_files(window, cx))))
                .child(button("act-full", if c.full_access { "Full access: on".into() } else { "Full access: off".into() }, if c.full_access { 0x7a4a1b } else { PANEL }).on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_full_access();
                    cx.notify();
                })));
            if matches!(status, Some(Status::Exited(_))) {
                row = row.child(button("act-restart", "Restart".into(), ACCENT).on_click(cx.listener(|this, _, _, cx| {
                    this.relaunch(None);
                    cx.notify();
                })));
            }
            row
        });
        let move_panel = self.move_open.then(|| chat.clone()).flatten().map(|c| {
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
        // Security: full access is never switched on without this warning being confirmed.
        let warning = (self.terms.warn_full.as_deref() == chat.as_ref().map(|c| c.id.as_str()) && chat.is_some()).then(|| {
            div()
                .mx_4()
                .mt_2()
                .p_3()
                .rounded_lg()
                .border_1()
                .border_color(rgb(0xd9a441))
                .bg(rgb(PANEL))
                .flex()
                .flex_col()
                .gap_2()
                .child(div().font_weight(FontWeight::BOLD).child("Turn on full access?"))
                .child(div().text_sm().child(chats::FULL_ACCESS_WARNING))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(crate::ui::button("full-confirm", "Turn on and restart", 0x7a4a1b).on_click(cx.listener(|this, _, _, cx| {
                            this.relaunch(Some(true));
                            cx.notify();
                        })))
                        .child(crate::ui::button("full-cancel", "Cancel", USER_BUBBLE).on_click(cx.listener(|this, _, _, cx| {
                            this.terms.warn_full = None;
                            cx.notify();
                        }))),
                )
        });
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

        let body = match (live, &chat, key.as_ref().and_then(|k| self.terms.failed.get(k))) {
            (Some(l), _, _) => div().flex_1().min_h_0().child(l.view.clone()),
            (None, Some(_), Some(e)) => div().flex_1().p_4().text_color(rgb(MUTED)).child(SharedString::from(format!("Could not start this session: {e}"))),
            (None, Some(_), None) => div().flex_1().p_4().text_color(rgb(MUTED)).child("Starting the agent…"),
            (None, None, _) => div().flex_1().p_4().text_color(rgb(MUTED)).child("Pick a session on the left, or start a new one."),
        };
        let body = div()
            .id("terminal-pane")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .drag_over::<ExternalPaths>(|s, _, _, _| s.bg(rgba(0xd9775722)))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.attach_paths(paths.paths().iter().map(|p| p.to_string_lossy().into_owned()).collect(), cx);
            }))
            .child(body);

        div().flex().flex_col().flex_1().min_w_0().h_full().child(header).children(actions).children(warning).children(move_panel).children(rename_row).children(toast).child(body)
    }

    pub fn toggle_full_access(&mut self) {
        let Some(c) = self.open_chat_info().cloned() else { return };
        if c.full_access {
            self.relaunch(Some(false)); // turning it off needs no warning
        } else {
            self.terms.warn_full = Some(c.id);
        }
    }
}
