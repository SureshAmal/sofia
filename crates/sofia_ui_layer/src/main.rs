use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use sofia_ui_layer::pill::PillView;

#[cfg(target_os = "linux")]
fn main() {
    // A layer surface requires the Wayland backend. Refuse X11 rather than
    // allowing the compositor to tile Sofia as an ordinary application window.
    gpui_kit::platform::linux(WindowingModes::WAYLAND)
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            sofia_ui_layer::theme::init(cx);
            open_pill(pill_window_options(cx), cx).expect("open Wayland Sofia layer");
        });
}

#[cfg(not(target_os = "linux"))]
fn main() {
    application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            sofia_ui_layer::theme::init(cx);
            open_pill(pill_window_options(cx), cx).expect("open Sofia pill");
        });
}

fn open_pill(options: WindowOptions, cx: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let fullscreen = matches!(options.kind, WindowKind::LayerShell(_));
    cx.open_window(options, |window, cx| {
        sofia_ui_layer::theme::observe(window, cx);
        window.set_rem_size(cx.theme().font_size);
        cx.new(|cx| PillView::new(cx, fullscreen))
    })?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn pill_window_options(cx: &App) -> WindowOptions {
    use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};

    let bounds = cx.primary_display().map_or_else(
        || bounds(point(px(0.), px(0.)), size(px(0.), px(0.))),
        |_| Bounds::maximized(None, cx),
    );
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        focus: false,
        is_resizable: false,
        kind: WindowKind::LayerShell(LayerShellOptions {
            namespace: "sofia-pill".into(),
            layer: Layer::Overlay,
            anchor: Anchor::TOP | Anchor::RIGHT | Anchor::BOTTOM | Anchor::LEFT,
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            ..Default::default()
        }),
        window_background: WindowBackgroundAppearance::Transparent,
        app_id: Some("sofia-pill".into()),
        ..Default::default()
    }
}

#[cfg(not(target_os = "linux"))]
fn pill_window_options(cx: &App) -> WindowOptions {
    use gpui_kit::component::ActiveTheme as _;
    let rem = f32::from(cx.theme().font_size);
    WindowOptions {
        window_bounds: Some(WindowBounds::centered(
            size(px(rem * 5.5), px(rem * 20.5)),
            cx,
        )),
        titlebar: None,
        focus: false,
        is_resizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        app_id: Some("sofia-pill".into()),
        ..Default::default()
    }
}
