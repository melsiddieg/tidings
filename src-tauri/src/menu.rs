//! Native menu bar for macOS. Items send their id to the UI as a "menu" event.
//! (Linux windows have no menu bar; the UI handles the same shortcuts itself.)

use tauri::menu::{AboutMetadata, Menu, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Runtime};

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn item<R: Runtime>(app: &AppHandle<R>, id: &str, text: &str, accel: Option<&str>) -> tauri::Result<tauri::menu::MenuItem<R>> {
    let mut builder = MenuItemBuilder::with_id(id, text);
    if let Some(accel) = accel {
        builder = builder.accelerator(accel);
    }
    builder.build(app)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let about = AboutMetadata {
        name: Some("Tidings".into()),
        version: Some(env!("CARGO_PKG_VERSION").into()),
        comments: Some("A private, secure feed reader. No telemetry.".into()),
        ..Default::default()
    };
    let app_menu = SubmenuBuilder::new(app, "Tidings")
        .about(Some(about))
        .separator()
        .item(&item(app, "settings", "Settings…", Some("CmdOrCtrl+,"))?)
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;
    let file = SubmenuBuilder::new(app, "File")
        .item(&item(app, "add-feed", "Add Feed…", Some("CmdOrCtrl+N"))?)
        .item(&item(app, "new-folder", "New Folder…", Some("CmdOrCtrl+Shift+N"))?)
        .separator()
        .item(&item(app, "import-opml", "Import OPML…", None)?)
        .item(&item(app, "export-opml", "Export OPML…", None)?)
        .separator()
        .item(&item(app, "refresh-all", "Refresh All Feeds", Some("CmdOrCtrl+R"))?)
        .item(&item(app, "mark-all-read", "Mark All as Read", None)?)
        .separator()
        .item(&item(app, "close-tab", "Close Tab", Some("CmdOrCtrl+W"))?)
        .build()?;
    // The Edit menu is required on macOS for copy and paste to work in text fields.
    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .separator()
        .item(&item(app, "find", "Find…", Some("CmdOrCtrl+F"))?)
        .build()?;
    let view = SubmenuBuilder::new(app, "View")
        .item(&item(app, "view-widescreen", "Widescreen Layout", Some("CmdOrCtrl+Alt+1"))?)
        .item(&item(app, "view-normal", "Normal Layout", Some("CmdOrCtrl+Alt+2"))?)
        .item(&item(app, "view-combined", "Combined Layout", Some("CmdOrCtrl+Alt+3"))?)
        .separator()
        .fullscreen()
        .build()?;
    let window = SubmenuBuilder::new(app, "Window").minimize().maximize().build()?;
    let help = SubmenuBuilder::new(app, "Help")
        .item(&item(app, "shortcuts", "Keyboard Shortcuts", Some("CmdOrCtrl+/"))?)
        .build()?;
    Menu::with_items(app, &[&app_menu, &file, &edit, &view, &window, &help])
}

pub fn forward<R: Runtime>(app: &AppHandle<R>, id: &str) {
    let _ = app.emit("menu", id);
}
