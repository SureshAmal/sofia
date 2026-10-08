use cpal::traits::{DeviceTrait, HostTrait};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, IndexPath,
    button::Button,
    input::{Input, InputState, Textarea, TextareaState},
    select::{SearchableVec, Select, SelectEvent, SelectState},
    setting::{SettingField, SettingGroup, SettingItem, SettingPage, Settings as SettingsPanel},
};
use gpui_kit::*;
use sofia_config::Settings;
use sofia_protocol::{ClientRequest, ServerEvent};
use sofia_ui_layer::ipc_client::{IpcClient, UiUpdate};
use std::sync::mpsc::Receiver;
use std::time::Duration;
type Picker = Entity<SelectState<SearchableVec<SharedString>>>;
pub struct SettingsView {
    settings: Settings,
    mcp: Entity<crate::mcp::McpPanel>,
    fields: Vec<Entity<InputState>>,
    prompt: Entity<TextareaState>,
    output: Picker,
    output_ids: Vec<Option<String>>,
    theme: Picker,
    voice: Picker,
    api_key: Entity<InputState>,
    api_model: Entity<InputState>,
    status: String,
    connected: bool,
    ipc: IpcClient,
    updates: Receiver<UiUpdate>,
}
const LABELS: [&str; 6] = [
    "Project ID",
    "Location",
    "Live model",
    "Client ID",
    "Client secret",
    "Refresh token",
];
impl SettingsView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let loaded = sofia_config::load();
        let status = loaded.as_ref().err().cloned().unwrap_or_else(|| {
            "Settings apply when you save. Credentials left blank use .env.".into()
        });
        let settings = loaded.unwrap_or_default();
        let vertex = &settings.vertex;
        let values = [
            &vertex.project_id,
            &vertex.location,
            &vertex.model,
            &vertex.client_id,
            &vertex.client_secret,
            &vertex.refresh_token,
        ];
        let fields: Vec<_> = values
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(value.clone())
                        .masked(index >= 4)
                })
            })
            .collect();
        let mut voices = vec![SharedString::from("Provider default")];
        voices.extend(
            livesofia::voice::available_voices()
                .into_iter()
                .map(|voice| SharedString::from(voice.name)),
        );
        let selected = settings
            .gemini
            .voice_name
            .as_ref()
            .and_then(|name| voices.iter().position(|value| value.as_ref() == name))
            .unwrap_or(0);
        let voice = picker(voices, selected, window, cx);
        let api_key = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(settings.generative.api_key.clone())
                .masked(true)
        });
        let api_model = cx
            .new(|cx| InputState::new(window, cx).default_value(settings.generative.model.clone()));
        let prompt = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(14)
                .default_value(settings.assistant.system_prompt.clone())
        });
        let mut output_ids = vec![None];
        let mut output_names = vec![SharedString::from("Follow system default")];
        if let Ok(devices) = cpal::default_host().output_devices() {
            for device in devices {
                if let (Ok(id), Ok(description)) = (device.id(), device.description()) {
                    if id.1 == "null" {
                        continue;
                    }
                    output_ids.push(Some(id.to_string()));
                    output_names.push(description.name().to_string().into());
                }
            }
        }
        let selected = output_ids
            .iter()
            .position(|id| id == &settings.audio.output_device_id);
        let output = picker(output_names, selected.unwrap_or(0), window, cx);
        let mut themes: Vec<SharedString> = vec!["dark".into(), "light".into(), "system".into()];
        themes.extend(
            gpui_kit::component::ThemeRegistry::global(cx)
                .themes()
                .keys()
                .cloned(),
        );
        themes.sort();
        themes.dedup();
        let selected = themes
            .iter()
            .position(|name| name.as_ref() == settings.appearance.theme)
            .unwrap_or(0);
        let theme = picker(themes, selected, window, cx);
        cx.subscribe_in(&theme, window, |view, _, event, window, cx| {
            if let SelectEvent::Confirm(Some(name)) = event {
                view.settings.appearance.theme = name.to_string();
                super::apply_theme(name, Some(window), cx);
                view.status = "Theme preview. Save & apply to keep this preference.".into();
                cx.notify();
            }
        })
        .detach();
        let (ipc, updates) = IpcClient::start();
        cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                if view
                    .update(cx, |view, cx| {
                        while let Ok(update) = view.updates.try_recv() {
                            match update {
                                UiUpdate::Snapshot(_) => view.connected = true,
                                UiUpdate::Disconnected => view.connected = false,
                                UiUpdate::Event(ServerEvent::Error { message }) => {
                                    view.status = message
                                }
                                _ => {}
                            }
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let mcp = cx.new(|cx| crate::mcp::McpPanel::new(&settings.mcp_servers, window, cx));
        Self {
            mcp,
            settings,
            fields,
            prompt,
            output,
            output_ids,
            theme,
            voice,
            api_key,
            api_model,
            status,
            connected: false,
            ipc,
            updates,
        }
    }
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.mcp_servers = match self.mcp.read(cx).collect(cx) {
            Ok(configs) => configs,
            Err(error) => {
                self.status = error;
                cx.notify();
                return;
            }
        };
        let values: Vec<String> = self
            .fields
            .iter()
            .map(|field| field.read(cx).value().to_string())
            .collect();
        let vertex = &mut self.settings.vertex;
        vertex.project_id = values[0].trim().into();
        vertex.location = values[1].trim().into();
        vertex.model = values[2].trim().into();
        vertex.client_id = values[3].trim().into();
        vertex.client_secret = values[4].clone();
        vertex.refresh_token = values[5].clone();
        self.settings.gemini.voice_name = self
            .voice
            .read(cx)
            .selected_value()
            .filter(|value| value.as_ref() != "Provider default")
            .map(|value| value.to_string());
        self.settings.generative.api_key = self.api_key.read(cx).value().to_string();
        self.settings.generative.model = self.api_model.read(cx).value().trim().to_string();
        self.settings.assistant.system_prompt = self.prompt.read(cx).value().to_string();
        self.settings.audio.output_device_id = self
            .output
            .read(cx)
            .selected_index(cx)
            .and_then(|index| self.output_ids.get(index.row).cloned())
            .flatten();
        if let Some(name) = self.theme.read(cx).selected_value() {
            self.settings.appearance.theme = name.to_string();
        }
        self.status = match sofia_config::save(&self.settings) {
            Ok(()) => {
                super::apply_theme(&self.settings.appearance.theme, Some(window), cx);
                if self.connected {
                    self.ipc.send(ClientRequest::ReloadSettings);
                    "Saved. Sofia is reconnecting to apply your settings.".into()
                } else {
                    "Saved. Sofia will use these settings when it starts.".into()
                }
            }
            Err(error) => error,
        };
        cx.notify();
    }
}
fn picker(
    values: Vec<SharedString>,
    selected: usize,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Picker {
    cx.new(|cx| {
        SelectState::new(
            SearchableVec::new(values),
            Some(IndexPath::new(selected)),
            window,
            cx,
        )
        .searchable(true)
    })
}
fn field(label: &'static str, state: &Entity<InputState>) -> SettingItem {
    let state = state.clone();
    SettingItem::new(
        label,
        SettingField::element(
            move |_: &gpui_kit::component::setting::RenderOptions, _: &mut Window, _: &mut App| {
                Input::new(&state).w_full()
            },
        ),
    )
    .layout(Axis::Vertical)
}
impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let prompt = self.prompt.clone();
        let dirty_prompt = prompt.clone();
        let reset_prompt = prompt.clone();
        let output = self.output.clone();
        let theme = self.theme.clone();
        let voice = self.voice.clone();
        let provider_get = cx.entity().downgrade();
        let provider_set = provider_get.clone();
        let use_vertex = self
            .settings
            .connection
            .use_vertex_ai
            .unwrap_or(self.api_key.read(cx).value().trim().is_empty());
        let credentials = if use_vertex {
            SettingGroup::new()
                .title("Vertex AI credentials")
                .items((0..6).map(|index| field(LABELS[index], &self.fields[index])))
        } else {
            SettingGroup::new()
                .title("Gemini API credentials")
                .item(field("API key", &self.api_key))
                .item(field("Live model", &self.api_model))
        };
        let get_view = cx.entity().downgrade();
        let set_view = get_view.clone();
        let mcp = self.mcp.clone();
        let pages = SettingsPanel::new("sofia-settings")
            .page(SettingPage::new("Assistant").icon(IconName::Bot).description("How Sofia speaks and uses tools")
                .group(SettingGroup::new().title("System prompt").item(
                    SettingItem::new("Instructions", SettingField::element(move |_: &gpui_kit::component::setting::RenderOptions, _: &mut Window, _: &mut App| {
                        Textarea::new(&prompt).w_full().h_80()
                    }).on_reset(
                        move |cx| dirty_prompt.read(cx).value().as_ref() != sofia_config::DEFAULT_SYSTEM_PROMPT,
                        move |window, cx| reset_prompt.update(cx, |state, cx| state.set_value(sofia_config::DEFAULT_SYSTEM_PROMPT, window, cx)),
                    )).layout(Axis::Vertical).description("Applied to the next live session. Restore defaults with the reset control."))))
            .page(SettingPage::new("Connection").icon(IconName::Network).description("Choose Google Cloud Vertex AI or the Gemini API")
                .group(SettingGroup::new().title("Provider").item(
                    SettingItem::new("Use Vertex AI", SettingField::switch(
                        move |cx| provider_get.upgrade().is_some_and(|view| {
                            let view = view.read(cx);
                            view.settings.connection.use_vertex_ai.unwrap_or(view.api_key.read(cx).value().trim().is_empty())
                        }),
                        move |enabled, cx| { if let Some(view) = provider_set.upgrade() { view.update(cx, |view, cx| { view.settings.connection.use_vertex_ai = Some(enabled); cx.notify(); }); } },
                    )).description("On: Vertex OAuth credentials. Off: Gemini API key. Save & apply reconnects Sofia.")))
                .group(credentials))
            .page(SettingPage::new("Voice & audio").icon(IconName::Mic)
                .group(SettingGroup::new().title("Audio")
                    .item(SettingItem::new("Gemini voice", SettingField::element(move |_: &gpui_kit::component::setting::RenderOptions, _: &mut Window, _: &mut App| {
                        Select::new(&voice).w_full()
                    })).layout(Axis::Vertical).description("Choose a prebuilt voice or use the provider default."))
                    .item(SettingItem::new("Speaker output", SettingField::element(move |_: &gpui_kit::component::setting::RenderOptions, _: &mut Window, _: &mut App| {
                        Select::new(&output).w_full()
                    })).layout(Axis::Vertical).description("System default follows desktop routing, including Bluetooth. Linux uses PipeWire."))
                    .item(SettingItem::new("Start listening when connected", SettingField::switch(
                        move |cx| get_view.upgrade().is_some_and(|view| view.read(cx).settings.audio.auto_listen),
                        move |value, cx| { if let Some(view) = set_view.upgrade() { view.update(cx, |view, cx| { view.settings.audio.auto_listen = value; cx.notify(); }); } },
                    ).default_value(true)).description("Right-click the pill to pause or resume. Left-click expands or collapses."))))
            .page(SettingPage::new("MCP").icon(IconName::Network).description("Connect local and remote tools to Gemini")
                .group(SettingGroup::new().title("Servers and tools").item(SettingItem::new("MCP connections", SettingField::element(move |_: &gpui_kit::component::setting::RenderOptions, _: &mut Window, _: &mut App| mcp.clone())).layout(Axis::Vertical).description("Test to select tools. Save & apply reconnects Gemini with enabled servers."))))
            .page(SettingPage::new("Appearance").icon(IconName::Palette)
                .group(SettingGroup::new().title("Theme").item(
                    SettingItem::new("Color theme", SettingField::element(move |_: &gpui_kit::component::setting::RenderOptions, _: &mut Window, _: &mut App| {
                        Select::new(&theme).w_full()
                    })).layout(Axis::Vertical).description("GPUI Kit loads and watches the Sofia themes folder. Reopen settings to select a newly added theme."))));
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .p_4()
                    .child(div().text_xl().child("Sofia Settings"))
                    .child(if self.connected {
                        "Sofia connected"
                    } else {
                        "Sofia offline"
                    }),
            )
            .child(div().flex_1().min_h_0().child(pages))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .p_4()
                    .child(div().flex_1().text_sm().child(self.status.clone()))
                    .child(
                        Button::new("save-settings")
                            .label("Save & apply")
                            .on_click(cx.listener(|view, _, window, cx| view.save(window, cx))),
                    ),
            )
    }
}
