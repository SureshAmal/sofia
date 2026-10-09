use gpui_kit::*;
use sofia_trace::TraceView;
fn main() {
    application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            sofia_ui_layer::theme::init(cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::centered(size(px(1200.), px(760.)), cx)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Sofia Trace".into()),
                        ..Default::default()
                    }),
                    app_id: Some("sofia-trace".into()),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    sofia_ui_layer::theme::observe(window, cx);
                    cx.new(|cx| TraceView::new(window, cx))
                },
            )
            .expect("open Sofia Trace");
            cx.activate(true);
        });
}
