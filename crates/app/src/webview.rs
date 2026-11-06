//! The embedded web view: one wry `WebView` laid over a box in the GPUI window (the phase 0 spike's approach, see
//! `docs/superpowers/spike-webview.md`). The native web view always draws above GPUI, so it is placed each frame
//! over a `slot` and hidden whenever no slot asked for it (another view, or an overlay or dialog covering it).
//!
//! Without the `webview` cargo feature everything here is a no-op and `unavailable` says why.

// Without the feature nothing ticks the web view, so its event plumbing is unused.
#![cfg_attr(not(feature = "webview"), allow(dead_code))]

use gpui::{Bounds, Pixels, Window};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::mpsc::{channel, Receiver, Sender};

#[derive(Clone, PartialEq, Hash)]
pub enum Source {
    /// A host page built by `workspaceai_client::web::host_page` (the artifact sits in a sandboxed frame inside).
    Html(String),
    Url(String),
}

/// What the page told the app.
pub enum Event {
    /// Esc in a full-screen artifact.
    Leave,
    /// A page finished loading at this address (the browser panel's address bar follows it).
    Loaded(String),
    /// The page asked for a new window; it opens in the system browser instead.
    Popup(String),
    /// Creating the web view failed; `unavailable` has the reason.
    Failed,
}

fn key(src: &Source) -> u64 {
    let mut h = DefaultHasher::new();
    src.hash(&mut h);
    h.finish()
}

pub struct Web {
    #[cfg(feature = "webview")]
    view: Option<wry::WebView>,
    loaded: u64,
    is_url: bool,
    wanted: bool,
    shown: bool,
    bounds: Option<Bounds<Pixels>>,
    error: Option<String>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
}

impl Web {
    pub fn new() -> Web {
        let (tx, rx) = channel();
        Web {
            #[cfg(feature = "webview")]
            view: None,
            loaded: 0,
            is_url: false,
            wanted: false,
            shown: false,
            bounds: None,
            error: None,
            tx,
            rx,
        }
    }

    /// `Some(reason)` when no web view can be shown in this build or session.
    pub fn unavailable(&self) -> Option<String> {
        #[cfg(not(feature = "webview"))]
        return Some("This needs the web view, which this build leaves out. Build the app with the `webview` feature (on by default; Linux needs libwebkit2gtk-4.1-dev and libgtk-3-dev).".into());
        #[cfg(feature = "webview")]
        imp::init_error().or_else(|| self.error.clone())
    }

    /// Called at the start of every render: a slot painted in this frame sets it again.
    pub fn begin_frame(&mut self) {
        self.wanted = false;
    }

    /// Puts the web view over `bounds` (window coordinates, logical pixels) showing `src`; creates it the first time.
    pub fn place(&mut self, window: &Window, bounds: Bounds<Pixels>, src: &Source) {
        self.wanted = true;
        #[cfg(feature = "webview")]
        {
            if self.unavailable().is_some() {
                return;
            }
            let k = key(src);
            let Some(view) = &self.view else {
                match imp::build(window, bounds, src, self.tx.clone()) {
                    Ok(v) => {
                        self.view = Some(v);
                        (self.loaded, self.is_url, self.shown, self.bounds) = (k, matches!(src, Source::Url(_)), true, Some(bounds));
                    }
                    Err(e) => {
                        self.error = Some(e);
                        let _ = self.tx.send(Event::Failed);
                    }
                }
                return;
            };
            if self.bounds != Some(bounds) {
                let _ = view.set_bounds(imp::rect(bounds));
                self.bounds = Some(bounds);
            }
            if !self.shown {
                let _ = view.set_visible(true);
                self.shown = true;
            }
            if self.loaded != k {
                let _ = match src {
                    Source::Html(h) => view.load_html(h),
                    Source::Url(u) => view.load_url(u),
                };
                (self.loaded, self.is_url) = (k, matches!(src, Source::Url(_)));
            }
        }
        #[cfg(not(feature = "webview"))]
        let _ = (window, bounds, key(src));
    }

    /// Runs every 16 ms: hides the web view when no slot wanted it in the last frame, and returns the page's events.
    pub fn tick(&mut self) -> Vec<Event> {
        #[cfg(feature = "webview")]
        if !self.wanted && self.shown {
            if let Some(v) = &self.view {
                let _ = v.set_visible(false);
                // Keyboard focus goes back to the GPUI window, not to an invisible page.
                let _ = v.focus_parent();
            }
            self.shown = false;
        }
        let events: Vec<Event> = self.rx.try_iter().collect();
        for e in &events {
            // A link followed inside the page is the new address; placing the same URL must not reload it.
            if let (Event::Loaded(u), true) = (e, self.is_url) {
                self.loaded = key(&Source::Url(u.clone()));
            }
        }
        events
    }

    pub fn script(&self, _js: &str) {
        #[cfg(feature = "webview")]
        if let Some(v) = &self.view {
            let _ = v.evaluate_script(_js);
        }
    }

    pub fn reload(&self) {
        #[cfg(feature = "webview")]
        if let Some(v) = &self.view {
            let _ = v.reload();
        }
    }

    /// Hands the keyboard back to the GPUI window (called on every click in it; clicks in the page never get there).
    pub fn release_focus(&self) {
        #[cfg(feature = "webview")]
        if let Some(v) = self.view.as_ref().filter(|_| self.shown) {
            let _ = v.focus_parent();
        }
    }

    /// Gives the page the keyboard (a deck needs its arrow keys in full screen).
    pub fn focus(&self) {
        #[cfg(feature = "webview")]
        if let Some(v) = &self.view {
            let _ = v.focus();
        }
    }
}

/// Once, before the GPUI application starts: GTK must be initialised on the main thread for WebKitGTK.
pub fn init() {
    #[cfg(feature = "webview")]
    imp::init();
}

/// Runs pending GTK work; the web view does nothing without it.
pub fn pump() {
    #[cfg(all(feature = "webview", target_os = "linux"))]
    if imp::init_error().is_none() {
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
    }
}

#[cfg(feature = "webview")]
mod imp {
    use super::{Event, Source};
    use gpui::{Bounds, Pixels, Window};
    use std::sync::mpsc::Sender;
    use std::sync::OnceLock;
    use wry::dpi::{LogicalPosition, LogicalSize};
    use wry::{NewWindowResponse, PageLoadEvent, Rect, WebView, WebViewBuilder};

    static INIT: OnceLock<Option<String>> = OnceLock::new();

    pub fn init() {
        #[cfg(target_os = "linux")]
        let r = gtk::init().err().map(|e| format!("The web view could not start GTK: {e}"));
        #[cfg(not(target_os = "linux"))]
        let r = None;
        let _ = INIT.set(r);
    }

    pub fn init_error() -> Option<String> {
        INIT.get().cloned().unwrap_or_else(|| Some("The web view was not initialised.".into()))
    }

    pub fn rect(b: Bounds<Pixels>) -> Rect {
        let (x, y, w, h): (f32, f32, f32, f32) = (b.origin.x.into(), b.origin.y.into(), b.size.width.into(), b.size.height.into());
        Rect { position: LogicalPosition::new(x as f64, y as f64).into(), size: LogicalSize::new(w.max(1.) as f64, h.max(1.) as f64).into() }
    }

    /// GPUI hands out an XCB handle; wry on Linux wants Xlib. Same X11 window id, different label (from the spike).
    #[cfg(target_os = "linux")]
    struct XlibShim(u64);
    #[cfg(target_os = "linux")]
    impl raw_window_handle::HasWindowHandle for XlibShim {
        fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
            // SAFETY: the id names the live GPUI window, which outlives the web view built as its child.
            Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(raw_window_handle::RawWindowHandle::Xlib(raw_window_handle::XlibWindowHandle::new(self.0))) })
        }
    }

    pub fn build(window: &Window, bounds: Bounds<Pixels>, src: &Source, tx: Sender<Event>) -> Result<WebView, String> {
        let (t_ipc, t_load, t_pop) = (tx.clone(), tx.clone(), tx);
        let b = WebViewBuilder::new()
            .with_bounds(rect(bounds))
            .with_ipc_handler(move |req| {
                if req.body() == "leave-fullscreen" {
                    let _ = t_ipc.send(Event::Leave);
                }
            })
            .with_on_page_load_handler(move |ev, url| {
                if matches!(ev, PageLoadEvent::Finished) {
                    let _ = t_load.send(Event::Loaded(url));
                }
            })
            .with_new_window_req_handler(move |url, _| {
                let _ = t_pop.send(Event::Popup(url));
                NewWindowResponse::Deny
            });
        let b = match src {
            Source::Html(h) => b.with_html(h),
            Source::Url(u) => b.with_url(u),
        };
        #[cfg(target_os = "linux")]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            let id = match HasWindowHandle::window_handle(window).map(|h| h.as_raw()) {
                Ok(RawWindowHandle::Xcb(h)) => h.window.get() as u64,
                Ok(RawWindowHandle::Xlib(h)) => h.window as u64,
                _ => return Err("The web view needs an X11 window, and this window is not one (a Wayland session). Start the app under XWayland (unset WAYLAND_DISPLAY) to use it.".into()),
            };
            b.build_as_child(&XlibShim(id)).map_err(|e| format!("The web view could not be created: {e}"))
        }
        #[cfg(not(target_os = "linux"))]
        b.build_as_child(window).map_err(|e| format!("The web view could not be created: {e}"))
    }
}
