# Phase 0 spike: wry web view inside a GPUI window

2026-11-05 · throwaway code in `crates/spike` (not a workspace member, not shipped)

**Pins:** `gpui` + `gpui_platform` git rev `654efae441bc8eb117d0042bb28cd7cf3570bdc9` (features `x11`, `wayland`), `wry 0.57`, `gtk 0.18`, `raw-window-handle 0.6`. Rust 1.98.

## Result

| Check | X11 (this machine, `DISPLAY=:0`) | Wayland |
|---|---|---|
| GPUI window opens | pass | not tested (no Wayland session here) |
| Attempt A: `wry` child web view built from GPUI's window handle | **failed as is** in an earlier run of the spike (the committed code always uses the shim, so this is not reproducible from it): "the window handle kind is not supported" (GPUI gives an XCB handle, wry on Linux accepts only Xlib) | expected to fail: wry child embedding on Linux is X11 only |
| Attempt A′: same, with the XCB window id re-labelled as an Xlib handle (`XlibShim`) | **pass**: web view created | n/a |
| JS runs in the web view, IPC reaches Rust | pass (`RESULT js: ok`) | n/a |
| Sandbox rule: `<iframe sandbox="allow-scripts">` cannot read the parent | pass for a `srcdoc` iframe inside a `with_html` page (`SecurityError`). Weak evidence: a sandboxed iframe without `allow-same-origin` always gets an opaque origin, so this is guaranteed. **Not tested:** a page served from `http://127.0.0.1:<port>` as plan step 5 asks, which is how Open Design pages will load | n/a |
| Web view visible and correctly placed | **not verified**: no screenshot was taken; needs a human look | not tested |
| Web view resizes with the window; focus moves between GPUI inputs and the page | **not implemented** in the spike (fixed 900x560 bounds, no resize handling, no GPUI text input), so these are open work, not just unchecked | not tested |
| Attempt B: web view in its own native window | not run | not run |

## What the spike shows

- Embedding works on X11 with a small shim (`crates/spike/src/main.rs`, `XlibShim`). GTK must be initialised and its event loop pumped from a GPUI timer (16 ms) for the web view to run.
- Needs the system libraries `libwebkit2gtk-4.1` and `libgtk-3` at runtime. Linking GPUI here also needed a `libxkbcommon-x11.so` dev symlink (`libxkbcommon-x11-dev` is not installed); the spike built with `RUSTFLAGS="-L native=<dir with symlink>"`.
- Wayland: wry cannot embed a child web view into a non-GTK Wayland surface. The expected route is attempt B (a separate native window per web view), or forcing X11 (`GDK_BACKEND=x11` plus XWayland) for the whole app.

## Recommendation

Go for X11 with the shim. **Open items for the owner or a later task before phase 4 depends on this:**
1. Someone looks at the window once (placement, resize, focus) because the run could not show it.
2. Run attempt B and the Wayland session on a Wayland machine; if embedding is not possible there, decide between "separate window" and "XWayland only" for Open Design and the browser panel.
