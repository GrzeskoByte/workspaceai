// THROWAWAY phase 0 spike: can a wry web view live inside a GPUI window? Prints RESULT lines and exits.
use gpui::{div, prelude::*, px, rgb, size, App, Bounds, Context, Window, WindowBounds, WindowOptions};
use raw_window_handle::{HasWindowHandle, RawWindowHandle, WindowHandle, XlibWindowHandle};
use std::time::Duration;

/// GPUI hands out an XCB handle; wry on Linux wants Xlib. Same X11 window id, different label.
struct XlibShim(u64);
impl HasWindowHandle for XlibShim {
    fn window_handle(&self) -> Result<WindowHandle<'_>, raw_window_handle::HandleError> {
        // SAFETY: the id names the live GPUI window for the duration of build_as_child.
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Xlib(XlibWindowHandle::new(self.0))) })
    }
}
use wry::{dpi::{LogicalPosition, LogicalSize}, Rect, WebView, WebViewBuilder};

const HTML: &str = r#"<!doctype html><body style="background:#fff"><h1>spike</h1>
<script>
document.title = 'js ok';
window.ipc.postMessage('RESULT js: ok');
window.addEventListener('message', (e) => window.ipc.postMessage('RESULT ' + e.data));
</script>
<iframe sandbox="allow-scripts" srcdoc="<script>
let r; try { window.parent.document.title; r = 'sandbox: LEAK (parent readable)'; } catch (e) { r = 'sandbox: blocked (' + e.name + ')'; }
parent.postMessage(r, '*');
</script>"></iframe></body>"#;

struct Spike {
    _web: Option<WebView>,
}

impl Render for Spike {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(0x202020)).text_color(rgb(0xffffff)).child(div().h(px(40.)).child("GPUI title bar area"))
    }
}

fn main() {
    gtk::init().expect("gtk init");
    gpui_platform::application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(900.), px(600.)), cx);
        cx.open_window(WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() }, |window, cx| {
            let id = match window.window_handle().map(|h| h.as_raw()) {
                Ok(RawWindowHandle::Xcb(h)) => Some(h.window.get() as u64),
                Ok(RawWindowHandle::Xlib(h)) => Some(h.window as u64),
                other => {
                    println!("RESULT handle: unexpected {other:?}");
                    None
                }
            };
            let shim = XlibShim(id.unwrap_or(0));
            let web = WebViewBuilder::new()
                .with_html(HTML)
                .with_bounds(Rect { position: LogicalPosition::new(0, 40).into(), size: LogicalSize::new(900, 560).into() })
                .with_ipc_handler(|req| println!("{}", req.body()))
                .build_as_child(&shim);
            match &web {
                Ok(_) => println!("RESULT embed: created"),
                Err(e) => println!("RESULT embed: FAILED {e}"),
            }
            cx.new(|_| Spike { _web: web.ok() })
        })
        .unwrap();
        cx.activate(true);
        // Pump GTK so the web view runs; stays open until the window is closed.
        cx.on_window_closed(|cx, _window_id| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.spawn(async move |cx| loop {
            cx.background_executor().timer(Duration::from_millis(16)).await;
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
        })
        .detach();
    });
}
