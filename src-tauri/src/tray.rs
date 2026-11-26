//! Tray icon. Closing the window hides it here, so this PC keeps serving its stats to your
//! other machines; "Quit" really exits.

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

struct AutostartItem(CheckMenuItem<Wry>);

static TOLD_ABOUT_TRAY: AtomicBool = AtomicBool::new(false);

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        // The dashboard pauses redrawing while hidden; tell it to resume.
        let _ = w.emit("fleet://visible", true);
    }
}

/// Hide instead of closing; mention it once per run so the window doesn't seem to vanish.
pub fn hide_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
        let _ = w.emit("fleet://visible", false);
    }
    if !TOLD_ABOUT_TRAY.swap(true, Ordering::SeqCst) {
        let _ = crate::system::notify(
            app.clone(),
            "Pane is still running".into(),
            "It keeps collecting in the tray so your other PCs can see this one. Right-click the tray icon to quit.".into(),
        );
    }
}

pub fn sync_autostart(app: &AppHandle) {
    if let Some(item) = app.try_state::<AutostartItem>() {
        let _ = item.0.set_checked(app.autolaunch().is_enabled().unwrap_or(false));
    }
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Pane", true, None::<&str>)?;
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        "Start with Windows",
        true,
        app.autolaunch().is_enabled().unwrap_or(false),
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit Pane", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&show, &PredefinedMenuItem::separator(app)?, &autostart, &PredefinedMenuItem::separator(app)?, &quit],
    )?;
    app.manage(AutostartItem(autostart));

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Pane")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main(app),
            "autostart" => {
                let al = app.autolaunch();
                let on = al.is_enabled().unwrap_or(false);
                let _ = if on { al.disable() } else { al.enable() };
                sync_autostart(app);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}
