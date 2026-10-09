//! Shared GPUI Kit theme setup for Sofia desktop clients.
use gpui_kit::component::{Theme, ThemeMode, ThemeRegistry};
use gpui_kit::*;

pub fn apply(name: &str, window: Option<&mut Window>, cx: &mut App) {
    match name {
        "light" => Theme::change(ThemeMode::Light, window, cx),
        "dark" => Theme::change(ThemeMode::Dark, window, cx),
        "system" => Theme::sync_system_appearance(window, cx),
        name => {
            if let Some(config) = ThemeRegistry::global(cx).themes().get(name).cloned() {
                Theme::update(cx, |theme| theme.apply_config(&config));
                cx.refresh_windows();
            } else {
                Theme::sync_system_appearance(window, cx);
            }
        }
    }
    if let Ok(settings) = sofia_config::load() {
        apply_appearance(&settings.appearance, cx);
    }
}

pub fn apply_appearance(appearance: &sofia_config::AppearanceSettings, cx: &mut App) {
    Theme::update(cx, |theme| {
        theme.font_family = appearance
            .font_family
            .as_deref()
            .unwrap_or(".SystemUIFont")
            .into();
        let radius = appearance.radius.unwrap_or(6) as f32;
        theme.radius = px(radius);
        theme.radius_lg = px(radius + 2.);
    });
}

pub fn apply_saved(window: Option<&mut Window>, cx: &mut App) {
    let name = sofia_config::load()
        .map(|settings| settings.appearance.theme)
        .unwrap_or_else(|_| "system".into());
    apply(&name, window, cx);
}

pub fn init(cx: &mut App) {
    apply_saved(None, cx);
    if let Ok(path) = sofia_config::settings_path() {
        cx.spawn(async move |cx| {
            let mut modified = std::fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .ok();
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;
                let next = std::fs::metadata(&path)
                    .and_then(|meta| meta.modified())
                    .ok();
                if next != modified {
                    modified = next;
                    cx.update(|cx| apply_saved(None, cx));
                }
            }
        })
        .detach();
    }
    if let Ok(path) = sofia_config::settings_path() {
        let directory = path.parent().unwrap().join("themes");
        if std::fs::create_dir_all(&directory).is_ok() {
            let _ = ThemeRegistry::watch_dir(directory, cx, |cx| {
                for handle in cx.windows() {
                    let _ = handle.update(cx, |_, window, cx| apply_saved(Some(window), cx));
                }
            });
        }
    }
}

pub fn observe(window: &mut Window, cx: &mut App) {
    apply_saved(Some(window), cx);
    window
        .observe_window_appearance(|window, cx| {
            if sofia_config::load()
                .map(|settings| settings.appearance.theme == "system")
                .unwrap_or(true)
            {
                Theme::sync_system_appearance(Some(window), cx);
                if let Ok(settings) = sofia_config::load() {
                    apply_appearance(&settings.appearance, cx);
                }
            }
        })
        .detach();
}
