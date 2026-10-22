// pith: a desktop for pi, drawn natively with gpui
mod core;
mod dev;
mod state;
mod ui;

use gpui_kit::*;

fn main() {
    gpui_kit::application().with_assets(gpui_kit::assets::AllAssets).run(|cx| {
        gpui_kit::init(cx);
        ui::theme::apply(None, cx);
        ui::workspace::bind_keys(cx);
        state::app::Pith::init(cx);
        cx.on_action(|_: &ui::workspace::Quit, cx| cx.quit());
        cx.on_app_quit(|cx| {
            state::app::Pith::global(cx).update(cx, |a, cx| a.stop_all(cx));
            core::term::close_all();
            async {}
        }).detach();

        let mac = cfg!(target_os = "macos");
        let bounds = Bounds::centered(None, size(px(1320.), px(860.)), cx);
        let opts = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions { title: Some("pith".into()), appears_transparent: true, traffic_light_position: Some(point(px(14.), px(15.))) }),
            window_min_size: Some(size(px(720.), px(480.))),
            app_id: Some("pith".into()),
            window_decorations: if mac { None } else { Some(WindowDecorations::Client) },
            ..Default::default()
        };
        let (win, _) = gpui_kit::open_window(opts, cx, |window, cx| {
            ui::theme::apply(Some(window), cx);
            cx.new(|cx| ui::workspace::Workspace::new(window, cx))
        }).expect("pith couldn't open its window");
        dev::start(win, cx);
        cx.activate(true);
    });
}
