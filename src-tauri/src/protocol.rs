//! `wsnap` custom URI scheme: serves frozen frames to overlays and snapshot images to the strip.
//! URLs: frame/<captureId>/<overlayLabel> (raw RGBA) and snap/<sessionId>/<letter>/<revision> (PNG).
//! Built on the JS side with `convertFileSrc(path, "wsnap")`.

use crate::state::AppState;
use tauri::http::{Request, Response, StatusCode, header};
use tauri::{AppHandle, Manager, UriSchemeContext, UriSchemeResponder, Wry};

pub fn handle(ctx: UriSchemeContext<'_, Wry>, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let app = ctx.app_handle().clone();
    // Off the main thread: a 4K frame is ~33 MB to copy.
    std::thread::spawn(move || responder.respond(route(&app, &request)));
}

fn route(app: &AppHandle, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let path = urlencoding::decode(request.uri().path()).map(|p| p.into_owned()).unwrap_or_default();
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    match parts.as_slice() {
        ["frame", id, label] => match id.parse::<u64>().ok().and_then(|id| crate::overlay::frame(app, id, label)) {
            Some(frame) => ok("application/octet-stream", frame.rgba.clone(), "no-store"),
            None => not_found(),
        },
        ["snap", session_id, letter, _revision] => {
            let state = app.state::<AppState>();
            let path = {
                let session = state.session.lock().unwrap();
                session
                    .as_ref()
                    .filter(|s| s.id == *session_id)
                    .and_then(|s| letter.chars().next().and_then(|c| s.get(c)))
                    .map(|snap| snap.path.clone())
            };
            match path.and_then(|p| std::fs::read(p).ok()) {
                // The revision segment changes when the image changes, so caching is safe.
                Some(bytes) => ok("image/png", bytes, "max-age=3600"),
                None => not_found(),
            }
        }
        _ => not_found(),
    }
}

fn ok(content_type: &str, body: Vec<u8>, cache: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, cache)
        // Pages are served from another origin (tauri.localhost or the dev server).
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(body)
        .unwrap()
}

fn not_found() -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(Vec::new())
        .unwrap()
}
