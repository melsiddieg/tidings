// Tidings: a private, secure feed reader.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod brief;
mod commands;
mod db;
mod discover;
mod error;
mod feed_types;
mod feeds;
mod images;
mod media;
mod media_server;
mod menu;
mod net;
mod opml;
mod peertube;
mod protocol;
mod reader;
mod sanitize;
mod scheduler;
mod settings;
mod state;
mod updates;
mod video;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use tauri::webview::NewWindowResponse;
use tauri::{Manager, Url, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_window_state::StateFlags;

/// The web view may only show the app itself. Any attempt to navigate
/// somewhere else (for example a link that slipped through) is blocked.
fn is_app_url(url: &Url) -> bool {
    match url.scheme() {
        "tauri" => url.host_str() == Some("localhost"),
        "http" | "https" => {
            // Release builds on Linux and macOS use tauri://localhost.
            // The dev server is only allowed in debug builds.
            cfg!(debug_assertions) && matches!(url.host_str(), Some("localhost") | Some("127.0.0.1"))
        }
        "about" => url.as_str() == "about:blank",
        _ => false,
    }
}

fn build_main_window(app: &tauri::App) -> tauri::Result<()> {
    let builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .title("Tidings")
        .inner_size(1320.0, 840.0)
        .min_inner_size(880.0, 560.0)
        .center()
        .disable_drag_drop_handler()
        .on_navigation(is_app_url)
        .on_new_window(|_, _| NewWindowResponse::Deny);
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(16.0, 21.0));
    builder.build()?;
    Ok(())
}

/// WebKitGTK's DMA-BUF renderer shows a blank window with NVIDIA's proprietary
/// driver. Turn it off in that case, unless the user already chose a value.
/// `/proc/driver/nvidia` exists on Linux; `/dev/nvidiactl` on Linux and FreeBSD.
/// (OpenBSD has no NVIDIA driver.)
#[cfg(webkitgtk)]
fn webkitgtk_workarounds() {
    let nvidia = ["/proc/driver/nvidia/version", "/dev/nvidiactl"]
        .iter()
        .any(|p| std::path::Path::new(p).exists());
    if nvidia && std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // Safe: called first thing in main, before any other thread exists.
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
}

/// WebKitGTK cannot play media from the tidings:// scheme, so Linux and the BSDs
/// get audio from a token-protected server on 127.0.0.1 (see media_server.rs).
#[cfg(webkitgtk)]
fn start_media_server(app: &tauri::AppHandle, state: &Arc<state::AppState>) {
    let handle = app.clone();
    let handler: media_server::Handler = Arc::new(move |encoded, range| {
        let app = handle.clone();
        Box::pin(async move { protocol::media_response(&app, &encoded, range.as_deref()).await })
    });
    match tauri::async_runtime::block_on(media_server::start(handler)) {
        Ok(base) => {
            let _ = state.media_base.set(base);
        }
        Err(err) => eprintln!("tidings: podcast audio server could not start: {err}"),
    }
}

fn main() {
    #[cfg(webkitgtk)]
    webkitgtk_workarounds();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED)
                .build(),
        )
        .register_asynchronous_uri_scheme_protocol("tidings", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn(async move {
                responder.respond(protocol::handle(&app, request).await);
            });
        })
        .on_menu_event(|app, event| menu::forward(app, event.id().as_ref()))
        .setup(|app| {
            let state = Arc::new(state::AppState::init(app.handle())?);
            app.manage(state.clone());
            #[cfg(target_os = "macos")]
            app.set_menu(menu::build(app.handle())?)?;
            #[cfg(webkitgtk)]
            start_media_server(app.handle(), &state);
            build_main_window(app)?;
            scheduler::start(app.handle().clone(), state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_tree,
            commands::get_counts,
            commands::list_articles,
            commands::list_articles_full,
            commands::get_article,
            commands::fetch_full_text,
            commands::read_url,
            commands::set_read,
            commands::set_starred,
            commands::mark_all_read,
            commands::delete_articles,
            commands::discover_feeds,
            commands::add_feed,
            commands::update_feed,
            commands::delete_feed,
            commands::move_feed,
            commands::create_folder,
            commands::rename_folder,
            commands::delete_folder,
            commands::move_folder,
            commands::set_pinned,
            commands::set_folder_expanded,
            commands::set_folders_expanded,
            commands::delete_items,
            commands::delete_all_subscriptions,
            commands::refresh_feeds,
            commands::import_opml,
            commands::export_opml,
            commands::get_settings,
            commands::save_settings,
            commands::open_external,
            commands::video_player,
            commands::play_video,
            commands::audio_stream,
            commands::check_components,
            commands::ai_models,
            commands::make_brief,
            commands::briefs,
            commands::brief,
            commands::delete_brief,
            commands::check_components_online,
            commands::peertube_video,
            commands::search_peertube,
            commands::set_badge,
            commands::app_info,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tidings");
}
