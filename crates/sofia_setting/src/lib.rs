mod mcp;
// Sofia settings desktop application.
mod view;
pub use sofia_ui_layer::theme::apply as apply_theme;
pub use view::SettingsView;

#[cfg(test)]
mod tests {
    use gpui_kit::{
        AssetSource,
        assets::{Assets, IconName},
    };
    #[test]
    fn bundled_settings_icons_are_available() {
        for icon in [
            IconName::Bot,
            IconName::Network,
            IconName::Mic,
            IconName::Palette,
            IconName::Search,
            IconName::Undo2,
            IconName::ChevronDown,
        ] {
            assert!(Assets.load(&icon.path()).unwrap().is_some());
        }
    }
}
