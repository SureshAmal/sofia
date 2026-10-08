use gpui_kit::*;
use sofia_setting::SettingsView;
fn main() {
    application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            sofia_ui_layer::theme::init(cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::centered(size(px(960.), px(760.)), cx)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Sofia Settings".into()),
                        ..Default::default()
                    }),
                    app_id: Some("sofia-setting".into()),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    sofia_ui_layer::theme::observe(window, cx);
                    cx.new(|cx| SettingsView::new(window, cx))
                },
            )
            .expect("open Sofia settings");
            cx.activate(true);
        });
}
