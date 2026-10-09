//! Sofia's movable voice pill inside a transparent GPUI layer.

use crate::speech_flow::SpeechFlow;
use std::collections::BTreeMap;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use sofia_protocol::{
    AUDIO_SPECTRUM_BANDS, AudioSource, ClientRequest, ServerEvent, StateSnapshot, TurnState,
};

use crate::ipc_client::{IpcClient, UiUpdate};

const BASE_EDGE_GAP_REM: f32 = 0.75;

pub struct PillView {
    documents: crate::content_windows::WindowManager,
    ipc: IpcClient,
    receiver: Receiver<UiUpdate>,
    snapshot: StateSnapshot,
    bands: [f32; AUDIO_SPECTRUM_BANDS],
    smooth_bands: [f32; AUDIO_SPECTRUM_BANDS],
    phase: f32,
    error: bool,
    expanded: bool,
    assistant_text: String,
    speech: SpeechFlow,
    tools: BTreeMap<String, String>,
    last_tool: Option<(String, Instant)>,
    fullscreen: bool,
    viewport: (f32, f32),
    pill_size: (f32, f32),
    panel_size: (f32, f32),
    position: Option<(f32, f32)>,
    drag_offset: Option<(f32, f32)>,
    snap_x: Option<f32>,
    velocity_x: f32,
    dragged: bool,
    last_input_regions: Option<Vec<Bounds<Pixels>>>,
}

impl PillView {
    pub fn new(cx: &mut Context<Self>, fullscreen: bool) -> Self {
        let (ipc, receiver) = IpcClient::start();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                if this.update(cx, |view, cx| view.tick(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            documents: crate::content_windows::WindowManager::new(),
            ipc,
            receiver,
            snapshot: StateSnapshot::default(),
            bands: [0.0; AUDIO_SPECTRUM_BANDS],
            smooth_bands: [0.0; AUDIO_SPECTRUM_BANDS],
            phase: 0.0,
            error: false,
            expanded: false,
            assistant_text: String::new(),
            speech: SpeechFlow::default(),
            tools: BTreeMap::new(),
            last_tool: None,
            fullscreen,
            viewport: (0.0, 0.0),
            pill_size: (0.0, 0.0),
            panel_size: (0.0, 0.0),
            position: None,
            drag_offset: None,
            snap_x: None,
            velocity_x: 0.0,
            dragged: false,
            last_input_regions: None,
        }
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        let mut changed = self.documents.tick(cx);
        while let Ok(update) = self.receiver.try_recv() {
            changed = true;
            match update {
                UiUpdate::Snapshot(snapshot) => {
                    self.snapshot = snapshot;
                    self.error = false;
                }
                UiUpdate::Event(event) => self.apply_event(event),
                UiUpdate::Disconnected => {
                    self.snapshot.state = TurnState::Disconnected;
                    self.snapshot.microphone_active = false;
                    self.speech.clear();
                    self.tools.clear();
                    self.last_tool = None;
                }
            }
        }
        if let Some(target) = self.snap_x
            && let Some((x, _)) = self.position.as_mut()
        {
            // Snappy damping for edge magnetism
            self.velocity_x = (self.velocity_x + (target - *x) * 0.18) * 0.72;
            *x += self.velocity_x;
            if (target - *x).abs() < 0.25 && self.velocity_x.abs() < 0.25 {
                *x = target;
                self.snap_x = None;
                self.velocity_x = 0.0;
            }
            changed = true;
        }

        changed |= self.speech.tick(Instant::now());
        if self
            .last_tool
            .as_ref()
            .is_some_and(|(_, time)| time.elapsed() > Duration::from_millis(900))
        {
            self.last_tool = None;
            changed = true;
        }

        // Decay peak targets smoothly
        for band in &mut self.bands {
            if *band > 0.005 {
                *band *= 0.88;
                changed = true;
            } else if *band > 0.0 {
                *band = 0.0;
                changed = true;
            }
        }

        // Interpolate smooth_bands toward target bands for fluid visualization
        for (smooth, &target) in self.smooth_bands.iter_mut().zip(self.bands.iter()) {
            let diff = target - *smooth;
            if diff.abs() > 0.005 {
                *smooth += if diff > 0.0 {
                    diff * 0.45 // Quick attack
                } else {
                    diff * 0.22 // Smooth release
                };
                changed = true;
            } else if *smooth != target {
                *smooth = target;
                changed = true;
            }
        }

        if (matches!(
            self.snapshot.state,
            TurnState::Connecting
                | TurnState::Reconnecting
                | TurnState::Thinking
                | TurnState::ToolQueued
                | TurnState::ToolRunning
        ) || !self.tools.is_empty())
            && !cx.reduce_motion()
        {
            self.phase += 0.10;
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }

    fn apply_event(&mut self, event: ServerEvent) {
        match event {
            ServerEvent::ContentChanged { .. } => self.documents.refresh(),
            ServerEvent::TurnStateChanged { state } => {
                self.snapshot.state = state;
                if matches!(
                    state,
                    TurnState::Disconnected
                        | TurnState::Connecting
                        | TurnState::Reconnecting
                        | TurnState::Ready
                ) {
                    self.speech.clear();
                    self.tools.clear();
                    self.last_tool = None;
                }
                if state != TurnState::Error {
                    self.error = false;
                }
            }
            ServerEvent::AssistantTextDelta { turn_id, text } => {
                self.last_tool = None;
                self.speech
                    .update(turn_id.to_string(), &text, false, Instant::now());
            }
            ServerEvent::AssistantTextFinal {
                turn_id,
                text,
                interrupted,
            } => {
                if interrupted {
                    self.speech.clear();
                } else {
                    self.speech
                        .update(turn_id.to_string(), &text, true, Instant::now());
                }
            }
            ServerEvent::ToolCallRequested { call_id, name } => {
                self.speech.clear();
                self.last_tool = None;
                self.tools.insert(call_id, display_tool_name(&name));
            }
            ServerEvent::ToolCallFinished { call_id, name, .. } => {
                self.tools.remove(&call_id);
                self.last_tool = Some((display_tool_name(&name), Instant::now()));
            }
            ServerEvent::AudioSpectrum { source, bins } => {
                let active_source = match self.snapshot.state {
                    TurnState::Listening => Some(AudioSource::User),
                    TurnState::Speaking => Some(AudioSource::Assistant),
                    _ => None,
                };
                if active_source == Some(source) {
                    for (band, incoming) in self.bands.iter_mut().zip(bins) {
                        let incoming_level = incoming.clamp(0.0, 1.0);
                        *band = (*band * 0.25 + incoming_level * 0.75).max(*band * 0.8);
                    }
                }
            }
            ServerEvent::AudioStatusChanged {
                microphone_active,
                speaker_muted,
            } => {
                self.snapshot.microphone_active = microphone_active;
                self.snapshot.speaker_muted = speaker_muted;
            }
            ServerEvent::Error { message } => {
                self.error = true;
                self.speech.clear();
                self.tools.clear();
                self.last_tool = None;
                self.assistant_text = message;
            }
            _ => {}
        }
    }

    fn toggle_expanded(&mut self, cx: &mut Context<Self>) {
        if self.dragged {
            self.dragged = false;
            return;
        }
        self.expanded = !self.expanded;
        cx.notify();
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window) {
        if !self.fullscreen {
            return;
        }
        let (w, h) = if self.expanded {
            self.panel_size
        } else {
            self.pill_size
        };
        if let Some((x, y)) = self.position {
            let on_right = x + self.pill_size.0 / 2.0 >= self.viewport.0 / 2.0;
            let current_x = if self.expanded {
                if on_right {
                    (x + self.pill_size.0 - w).max(0.0)
                } else {
                    x
                }
            } else {
                x
            };
            let current_y = if self.expanded {
                (y + (self.pill_size.1 - h) / 2.0).clamp(0.0, (self.viewport.1 - h).max(0.0))
            } else {
                y
            };
            self.drag_offset = Some((
                f32::from(event.position.x) - current_x,
                f32::from(event.position.y) - current_y,
            ));
            self.dragged = false;
            self.snap_x = None;
            self.velocity_x = 0.0;
            window.set_input_region(None);
        }
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        self.documents.on_mouse_move(event, cx);
        let Some((offset_x, offset_y)) = self.drag_offset else {
            return;
        };
        if !event.dragging() {
            return;
        }
        let (w, h) = if self.expanded {
            self.panel_size
        } else {
            self.pill_size
        };
        let (old_x, old_y) = self.position.unwrap_or_default();
        let new_elem_x =
            (f32::from(event.position.x) - offset_x).clamp(0.0, (self.viewport.0 - w).max(0.0));
        let new_elem_y =
            (f32::from(event.position.y) - offset_y).clamp(0.0, (self.viewport.1 - h).max(0.0));

        let (x, y) = if self.expanded {
            let on_right = new_elem_x + w / 2.0 >= self.viewport.0 / 2.0;
            let pill_x = if on_right {
                (new_elem_x + w - self.pill_size.0)
                    .clamp(0.0, (self.viewport.0 - self.pill_size.0).max(0.0))
            } else {
                new_elem_x.clamp(0.0, (self.viewport.0 - self.pill_size.0).max(0.0))
            };
            let pill_y = (new_elem_y - (self.pill_size.1 - h) / 2.0)
                .clamp(0.0, (self.viewport.1 - self.pill_size.1).max(0.0));
            (pill_x, pill_y)
        } else {
            (new_elem_x, new_elem_y)
        };

        if (x - old_x).abs() + (y - old_y).abs() > 3.0 {
            self.dragged = true;
        }
        self.position = Some((x, y));
        cx.notify();
    }

    fn on_mouse_up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.documents.on_mouse_up(cx);
        if self.drag_offset.take().is_none() {
            return;
        }
        if let Some((x, _)) = self.position {
            let gap = f32::from(window.rem_size()) * BASE_EDGE_GAP_REM;
            self.snap_x = Some(if x + self.pill_size.0 / 2.0 < self.viewport.0 / 2.0 {
                gap
            } else {
                (self.viewport.0 - self.pill_size.0 - gap).max(0.0)
            });
        }
        cx.notify();
    }

    fn status_label(&self) -> &'static str {
        if self.error || self.snapshot.state == TurnState::Error {
            return "Sofia error";
        }
        match self.snapshot.state {
            TurnState::Disconnected => "Sofia offline",
            TurnState::Connecting | TurnState::Reconnecting => "Sofia connecting",
            TurnState::Ready => "Start listening",
            TurnState::Listening => "Stop listening",
            TurnState::Thinking => "Thinking…",
            TurnState::Speaking => "Sofia speaking",
            TurnState::ToolQueued | TurnState::ToolRunning => "Working…",
            TurnState::Error => "Sofia error",
        }
    }
}

/// Dynamic FFT audio visualizer supporting compact minimal and expanded full layouts.
struct PillWaveform {
    bands: [f32; AUDIO_SPECTRUM_BANDS],
    phase: f32,
    state: TurnState,
    color: Hsla,
    accent: Hsla,
    bar_size: Pixels,
    reduced_motion: bool,
}

impl PillWaveform {
    fn levels(&self, count: usize) -> Vec<f32> {
        let animated = matches!(
            self.state,
            TurnState::Connecting
                | TurnState::Reconnecting
                | TurnState::Thinking
                | TurnState::ToolQueued
                | TurnState::ToolRunning
        ) && !self.reduced_motion;

        (0..count)
            .map(|index| {
                if animated {
                    return (self.phase + index as f32 * 0.58).sin().abs() * 0.72;
                }
                let start = index * AUDIO_SPECTRUM_BANDS / count;
                let end = ((index + 1) * AUDIO_SPECTRUM_BANDS / count)
                    .max(start + 1)
                    .min(AUDIO_SPECTRUM_BANDS);
                self.bands[start..end]
                    .iter()
                    .copied()
                    .fold(0.0_f32, f32::max)
                    .powf(0.72)
            })
            .collect()
    }

    fn render(self, horizontal: bool) -> AnyElement {
        let levels = self.levels(if horizontal { 14 } else { 13 });
        let color = self.color;
        let accent = self.accent;
        let strongest = levels
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(index, _)| index);
        let peak = levels.iter().copied().fold(0.0_f32, f32::max);
        let bar_size = self.bar_size;

        canvas(
            |_, _, _| {},
            move |frame, _, window, _| {
                let count = levels.len() as f32;
                let axis = if horizontal {
                    f32::from(frame.size.width)
                } else {
                    f32::from(frame.size.height)
                };
                let step = axis / count;
                let thickness = if horizontal {
                    (bar_size * 1.5).min(px(step * 0.58))
                } else {
                    bar_size.min(px(step * 0.58))
                };
                let radius = thickness / 2.0;

                for (index, level) in levels.into_iter().enumerate() {
                    let primary = horizontal && strongest == Some(index) && level > 0.025;
                    // Blend the live speech envelope with each frequency band so
                    // quieter bands stay visible without one oversized peak.
                    let level = if horizontal {
                        (level * 0.65 + peak * 0.35).powf(0.65) * 0.80
                    } else {
                        level
                    }
                    .clamp(0.0, 1.0);
                    let bar_bounds = if horizontal {
                        let height = bar_size.max(frame.size.height * level);
                        bounds(
                            point(
                                frame.origin.x + px(step * (index as f32 + 0.5)) - thickness / 2.0,
                                frame.origin.y + (frame.size.height - height) / 2.0,
                            ),
                            size(thickness, height),
                        )
                    } else {
                        let width = bar_size.max(frame.size.width * level);
                        bounds(
                            point(
                                frame.origin.x + (frame.size.width - width) / 2.0,
                                frame.origin.y + px(step * (index as f32 + 0.5)) - thickness / 2.0,
                            ),
                            size(width, thickness),
                        )
                    };
                    window.paint_quad(
                        fill(
                            bar_bounds,
                            if primary {
                                accent
                            } else {
                                color.opacity(0.55 + level * 0.35)
                            },
                        )
                        .corner_radii(radius),
                    );
                }
            },
        )
        .size_full()
        .into_any_element()
    }
}

impl Render for PillView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.documents.sync(window, cx);
        let theme = cx.theme();
        let color = if self.error || self.snapshot.state == TurnState::Error {
            theme.red
        } else {
            match self.snapshot.state {
                TurnState::Listening => theme.green,
                TurnState::Speaking
                | TurnState::Thinking
                | TurnState::ToolQueued
                | TurnState::ToolRunning => theme.blue,
                TurnState::Connecting | TurnState::Reconnecting | TurnState::Disconnected => {
                    theme.foreground.opacity(0.6)
                }
                _ => theme.foreground,
            }
        };
        let label = self.status_label();
        let expanded = self.expanded;
        let bar_size = px(f32::from(window.rem_size()) * 0.125);
        let waveform = PillWaveform {
            bands: self.smooth_bands,
            phase: self.phase,
            state: self.snapshot.state,
            color,
            accent: theme.blue,
            bar_size,
            reduced_motion: cx.reduce_motion(),
        }
        .render(expanded);

        let content = if expanded {
            let tool = self
                .tools
                .values()
                .next()
                .cloned()
                .or_else(|| self.last_tool.as_ref().map(|(name, _)| name.clone()));
            let text = if self.error {
                div()
                    .text_base()
                    .text_color(color)
                    .child(self.assistant_text.clone())
                    .into_any_element()
            } else if let Some(tool) = tool {
                div()
                    .text_base()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(color)
                    .opacity(if cx.reduce_motion() {
                        1.
                    } else {
                        0.75 + 0.25 * (self.phase * 0.65).sin().abs()
                    })
                    .child(tool)
                    .into_any_element()
            } else if self.speech.visible() {
                self.speech.render(theme.blue, theme.foreground)
            } else {
                div()
                    .text_base()
                    .text_color(color)
                    .child(label)
                    .into_any_element()
            };
            v_flex()
                .size_full()
                .gap_2()
                .p_2p5()
                .child(
                    h_flex()
                        .w_full()
                        .h_8()
                        .items_center()
                        .child(div().h_8().flex_1().child(waveform)),
                )
                .child(div().flex_1().min_h_0().overflow_hidden().child(text))
                .into_any_element()
        } else {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .px_1()
                .py_2()
                .child(waveform)
                .into_any_element()
        };

        let content = div().size_full().child(content).with_spring(
            "sofia-pill-content-morph",
            SpringAnimation::new(SpringConfig::new(320.0, 30.0, 1.0))
                .to(if expanded { 1.0 } else { 0.0 })
                .with_epsilon(0.001),
            move |this, value| {
                let opacity = if expanded {
                    ((value - 0.40) / 0.60).clamp(0.0, 1.0)
                } else {
                    ((0.60 - value) / 0.60).clamp(0.0, 1.0)
                };
                this.opacity(opacity)
            },
        );

        let view = cx.entity().downgrade();
        let click_view = cx.entity().downgrade();
        let listen_view = cx.entity().downgrade();

        // 20% transparent background (80% opacity)
        let pill_bg = theme.background.opacity(0.80);

        let pill = div()
            .id("sofia-pill")
            .relative()
            .overflow_hidden()
            .rounded_full()
            .border_1()
            .border_color(color.opacity(0.85))
            .bg(pill_bg)
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                if let Some(view) = view.upgrade() {
                    view.update(cx, |view, _| view.on_mouse_down(event, window));
                }
            })
            .on_mouse_down(MouseButton::Right, move |_, _, cx| {
                if let Some(view) = listen_view.upgrade() {
                    view.update(cx, |view, _| {
                        let command = if view.snapshot.microphone_active {
                            ClientRequest::StopListening
                        } else if view.snapshot.state == TurnState::Ready {
                            ClientRequest::StartListening
                        } else {
                            return;
                        };
                        view.ipc.send(command);
                    });
                }
            })
            .on_click(move |_, _, cx| {
                if let Some(view) = click_view.upgrade() {
                    view.update(cx, |view, cx| view.toggle_expanded(cx));
                }
            })
            .child(content);

        if self.fullscreen {
            let rem = f32::from(window.rem_size());
            let surface = window.bounds().size;
            let ready = surface.width > Pixels::default() && surface.height > Pixels::default();
            let sw = f32::from(surface.width);
            let sh = f32::from(surface.height);
            self.viewport = (sw, sh);

            self.pill_size = (rem * 1.5, rem * 7.5);
            let panel_size = (rem * 12.0, rem * 6.0);
            self.panel_size = panel_size;

            let edge_gap = BASE_EDGE_GAP_REM * rem;

            if ready && self.position.is_none() {
                self.position = Some((
                    (self.viewport.0 - self.pill_size.0 - edge_gap).max(0.0),
                    ((self.viewport.1 - self.pill_size.1) / 2.0).max(0.0),
                ));
            }
            let (mut x, mut y) = self.position.unwrap_or_default();
            x = x.clamp(0.0, (self.viewport.0 - self.pill_size.0).max(0.0));
            y = y.clamp(0.0, (self.viewport.1 - self.pill_size.1).max(0.0));
            if ready {
                self.position = Some((x, y));
            }
            let on_right = x + self.pill_size.0 / 2.0 >= self.viewport.0 / 2.0;
            let panel_x = if on_right {
                (x + self.pill_size.0 - panel_size.0).max(0.0)
            } else {
                x
            };
            let panel_y = (y + (self.pill_size.1 - panel_size.1) / 2.0)
                .clamp(0.0, (self.viewport.1 - panel_size.1).max(0.0));
            let (input_x, input_y, input_size) = if expanded {
                (panel_x, panel_y, panel_size)
            } else {
                (x, y, self.pill_size)
            };
            if !ready {
                if self
                    .last_input_regions
                    .as_ref()
                    .is_none_or(|r| !r.is_empty())
                {
                    self.last_input_regions = Some(Vec::new());
                    window.set_input_region(Some(&[]));
                }
            } else if self.drag_offset.is_none() && !self.documents.is_dragging(cx) {
                let mut regions =
                    self.documents
                        .regions((input_x, input_y), input_size, self.viewport, rem, cx);
                regions.push(bounds(
                    point(px(input_x), px(input_y)),
                    size(px(input_size.0), px(input_size.1)),
                ));
                let changed = self.last_input_regions.as_ref() != Some(&regions);
                if changed {
                    self.last_input_regions = Some(regions.clone());
                    window.set_input_region(Some(&regions));
                }
            }

            let compact_size = self.pill_size;
            let morph = if expanded { 1.0 } else { 0.0 };

            // Snappy and responsive spring for smooth pill expanding & collapsing
            let pill = pill.opacity(if ready { 1.0 } else { 0.0 }).with_spring(
                "sofia-pill-morph",
                SpringAnimation::new(SpringConfig::new(320.0, 30.0, 1.0))
                    .to(morph)
                    .with_epsilon(0.001),
                move |this, value| {
                    let value = value.clamp(0.0, 1.0);
                    let mix = |start: f32, end: f32| start + (end - start) * value;
                    this.absolute()
                        .left(px(mix(x, panel_x)))
                        .top(px(mix(y, panel_y)))
                        .w(px(mix(compact_size.0, panel_size.0)))
                        .h(px(mix(compact_size.1, panel_size.1)))
                        .rounded(px(mix(compact_size.0 / 2.0, rem * 1.25)))
                },
            );

            let documents =
                self.documents
                    .render((input_x, input_y), input_size, self.viewport, rem, cx);
            let view = cx.entity().downgrade();
            let view_up = cx.entity().downgrade();
            div()
                .id("sofia-layer")
                .size_full()
                .relative()
                .child(gpui_kit::base::TextSelectionLayer)
                .on_mouse_move(move |event, _, cx| {
                    if let Some(view) = view.upgrade() {
                        view.update(cx, |view, cx| view.on_mouse_move(event, cx));
                    }
                })
                .on_mouse_up(MouseButton::Left, move |_, window, cx| {
                    if let Some(view) = view_up.upgrade() {
                        view.update(cx, |view, cx| view.on_mouse_up(window, cx));
                    }
                })
                .children(documents)
                .child(pill)
                .into_any_element()
        } else {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(pill.w(rems(1.5)).h(rems(7.5)))
                .into_any_element()
        }
    }
}

fn display_tool_name(name: &str) -> String {
    let Some(namespaced) = name.strip_prefix("mcp_") else {
        return name.into();
    };
    let Some((_, name)) = namespaced.split_once("__") else {
        return namespaced
            .split_once('_')
            .map(|(_, tool)| tool.to_string())
            .unwrap_or_else(|| name.into());
    };
    let mut bytes = Vec::new();
    let mut index = 0;
    while index < name.len() {
        if name.as_bytes()[index] == b'_'
            && index + 3 <= name.len()
            && name.as_bytes()[index + 1..index + 3]
                .iter()
                .all(u8::is_ascii_hexdigit)
            && let Ok(byte) = u8::from_str_radix(&name[index + 1..index + 3], 16)
        {
            bytes.push(byte);
            index += 3;
        } else {
            bytes.push(name.as_bytes()[index]);
            index += 1;
        }
    }
    String::from_utf8(bytes).unwrap_or_else(|_| name.into())
}
#[cfg(test)]
mod motion_tests {
    use super::display_tool_name;
    #[test]
    fn tool_labels_hide_namespace_and_preserve_native_names() {
        assert_eq!(display_tool_name("mcp_search__web_5fsearch"), "web_search");
        assert_eq!(
            display_tool_name("mcp_sofia_sofia_list_documents"),
            "sofia_list_documents"
        );
        assert_eq!(display_tool_name("native_abc"), "native_abc");
        assert_eq!(display_tool_name("mcp_server__tool_€"), "tool_€");
    }
}
