// What the OS around the window gets from pith: the menu bar on macos, notifications when a session finishes
// in the background, and the theme following matugen's file on linux
use crate::core::sys;
use crate::ui::workspace::{CloseTab, NewSession, OpenSettings, Palette, Quit, SwitchModel, TogglePanel, ToggleSidebar};
use gpui_kit::*;
use std::time::Duration;

actions!(pith, [About, Minimize, Zoom]);

pub fn init(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &Minimize, cx| { if let Some(w) = cx.active_window() { let _ = w.update(cx, |_, window, _| window.minimize_window()); } });
    cx.on_action(|_: &Zoom, cx| { if let Some(w) = cx.active_window() { let _ = w.update(cx, |_, window, _| window.zoom_window()); } });
    cx.on_action(|_: &About, cx| cx.open_url("https://github.com/himetau/pith"));
    // linux has no global menu bar here; gpui ignores this there
    cx.set_menus([
        Menu { name: "pith".into(), items: vec![
            MenuItem::action("About pith", About),
            MenuItem::separator(),
            MenuItem::action("Settings…", OpenSettings),
            MenuItem::separator(),
            MenuItem::action("Quit pith", Quit),
        ], disabled: false },
        Menu { name: "File".into(), items: vec![
            MenuItem::action("New Session", NewSession),
            MenuItem::action("Close Session", CloseTab),
        ], disabled: false },
        Menu { name: "Edit".into(), items: vec![
            MenuItem::os_action("Undo", gpui_kit::component::input::Undo, OsAction::Undo),
            MenuItem::os_action("Redo", gpui_kit::component::input::Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", gpui_kit::component::input::Cut, OsAction::Cut),
            MenuItem::os_action("Copy", gpui_kit::component::input::Copy, OsAction::Copy),
            MenuItem::os_action("Paste", gpui_kit::component::input::Paste, OsAction::Paste),
            MenuItem::os_action("Select All", gpui_kit::component::input::SelectAll, OsAction::SelectAll),
        ], disabled: false },
        Menu { name: "View".into(), items: vec![
            MenuItem::action("Command Palette", Palette),
            MenuItem::action("Switch Model", SwitchModel),
            MenuItem::separator(),
            MenuItem::action("Toggle Sidebar", ToggleSidebar),
            MenuItem::action("Toggle Panel", TogglePanel),
        ], disabled: false },
        Menu { name: "Window".into(), items: vec![
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Zoom", Zoom),
        ], disabled: false },
    ]);
    watch_theme(cx);
}

// matugen rewrites colors.css when the wallpaper changes; the accent follows without a restart
fn watch_theme(cx: &mut App) {
    let file = sys::theme_file();
    cx.spawn(async move |cx| {
        let mtime = |f: &std::path::Path| std::fs::metadata(f).and_then(|m| m.modified()).ok();
        let mut last = mtime(&file);
        loop {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            let now = mtime(&file);
            if now != last {
                last = now;
                let _ = cx.update(|cx| crate::ui::theme::apply(None, cx));
            }
        }
    }).detach();
}

// a session finished while you were looking elsewhere
pub fn notify(title: &str, body: &str, cx: &App) {
    // gpui only reports an active window while pith has focus
    if cx.active_window().is_some() { return; }
    if sys::load_settings()["notify"].as_bool() == Some(false) { return; }
    let (title, body) = (title.to_string(), body.to_string());
    std::thread::spawn(move || {
        #[cfg(target_os = "macos")]
        {
            let script = format!("display notification {:?} with title {:?}", body, title);
            let _ = std::process::Command::new("osascript").args(["-e", &script]).status();
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = std::process::Command::new("notify-send").args(["-a", "pith", "-i", "pith", &title, &body]).status();
        }
    });
}
