//! Sofia's movable voice pill inside a transparent GPUI layer.

use std::sync::mpsc::Receiver;
use std::time::Duration;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use sofia_protocol::{
    AUDIO_SPECTRUM_BANDS, AudioSource, ClientRequest, ServerEvent, StateSnapshot, TurnState,
};

use crate::ipc_client::{IpcClient, UiUpdate};

const PILL_WIDTH_REM: f32 = 3.0; // w-12
const PILL_HEIGHT_REM: f32 = 12.0; // h-48
const PANEL_WIDTH_REM: f32 = 36.0; // w-144
const PANEL_HEIGHT_REM: f32 = 16.0; // h-64
const EDGE_GAP_REM: f32 = 1.0; // spacing-4

pub struct PillView {
    ipc: IpcClient,
    receiver: Receiver<UiUpdate>,
    snapshot: StateSnapshot,
    bands: [f32; AUDIO_SPECTRUM_BANDS],
    phase: f32,
    error: bool,
    expanded: bool,
    assistant_text: String,
    fullscreen: bool,
    viewport: (f32, f32),
    pill_size: (f32, f32),
    position: Option<(f32, f32)>,
    drag_offset: Option<(f32, f32)>,
    snap_x: Option<f32>,
    velocity_x: f32,
    dragged: bool,
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
            ipc,
            receiver,
            snapshot: StateSnapshot::default(),
            bands: [0.0; AUDIO_SPECTRUM_BANDS],
            phase: 0.0,
            error: false,
            expanded: false,
            assistant_text: String::new(),
            fullscreen,
            viewport: (0.0, 0.0),
            pill_size: (0.0, 0.0),
            position: None,
            drag_offset: None,
            snap_x: None,
            velocity_x: 0.0,
            dragged: false,
        }
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
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
                }
            }
        }
        if let Some(target) = self.snap_x {
            if let Some((x, y)) = self.position.as_mut() {
                self.velocity_x = (self.velocity_x + (target - *x) * 0.12) * 0.76;
                *x += self.velocity_x;
                if (target - *x).abs() < 0.35 && self.velocity_x.abs() < 0.35 {
                    *x = target;
                    self.snap_x = None;
                    self.velocity_x = 0.0;
                }
                let _ = y;
                changed = true;
            }
        }
        for band in &mut self.bands {
            if *band > 0.001 {
                *band *= 0.92;
                changed = true;
            }
        }
        if matches!(
            self.snapshot.state,
            TurnState::Connecting
                | TurnState::Reconnecting
                | TurnState::Thinking
                | TurnState::ToolQueued
                | TurnState::ToolRunning
        ) && !cx.reduce_motion()
        {
            self.phase += 0.08;
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }

    fn apply_event(&mut self, event: ServerEvent) {
        match event {
            ServerEvent::TurnStateChanged { state } => {
                self.snapshot.state = state;
                if state == TurnState::Listening {
                    self.assistant_text.clear();
                }
                if state != TurnState::Error {
                    self.error = false;
                }
            }
            ServerEvent::AssistantTextDelta { text, .. } => self.assistant_text.push_str(&text),
            ServerEvent::AssistantTextFinal { text, .. } => self.assistant_text = text,
            ServerEvent::AudioSpectrum { source, bins } => {
                let active_source = match self.snapshot.state {
                    TurnState::Listening => Some(AudioSource::User),
                    TurnState::Speaking => Some(AudioSource::Assistant),
                    _ => None,
                };
                if active_source == Some(source) {
                    for (band, incoming) in self.bands.iter_mut().zip(bins) {
                        *band = (*band * 0.35 + incoming.clamp(0.0, 1.0) * 0.65).max(*band * 0.75);
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
            ServerEvent::Error { .. } => self.error = true,
            _ => {}
        }
    }

    fn toggle_listening(&mut self) {
        if self.dragged {
            self.dragged = false;
            return;
        }
        match self.snapshot.state {
            TurnState::Listening => self.ipc.send(ClientRequest::StopListening),
            TurnState::Ready => self.ipc.send(ClientRequest::StartListening),
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
        if !self.fullscreen || self.expanded {
            return;
        }
        if let Some((x, y)) = self.position {
            self.drag_offset = Some((
                f32::from(event.position.x) - x,
                f32::from(event.position.y) - y,
            ));
            self.dragged = false;
            self.snap_x = None;
            self.velocity_x = 0.0;
            window.set_input_region(None);
        }
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some((offset_x, offset_y)) = self.drag_offset else {
            return;
        };
        if !event.dragging() {
            return;
        }
        let (old_x, old_y) = self.position.unwrap_or_default();
        let x = (f32::from(event.position.x) - offset_x)
            .clamp(0.0, (self.viewport.0 - self.pill_size.0).max(0.0));
        let y = (f32::from(event.position.y) - offset_y)
            .clamp(0.0, (self.viewport.1 - self.pill_size.1).max(0.0));
        if (x - old_x).abs() + (y - old_y).abs() > 3.0 {
            self.dragged = true;
        }
        self.position = Some((x, y));
        cx.notify();
    }

    fn on_mouse_up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.drag_offset.take().is_none() {
            return;
        }
        if let Some((x, _)) = self.position {
            let gap = f32::from(window.rem_size()) * EDGE_GAP_REM;
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
            TurnState::Thinking => "Sofia thinking",
            TurnState::Speaking => "Sofia speaking",
            TurnState::ToolQueued | TurnState::ToolRunning => "Sofia using a tool",
            TurnState::Error => "Sofia error",
        }
    }
}

/// Each line uses one FFT band; width follows its frequency energy.
struct PillWaveform {
    bands: [f32; AUDIO_SPECTRUM_BANDS],
    phase: f32,
    state: TurnState,
    color: Hsla,
    reduced_motion: bool,
}

impl PillWaveform {
    fn bar_fraction(&self, index: usize) -> f32 {
        let center = 1.0
            - ((index as f32 - (AUDIO_SPECTRUM_BANDS as f32 - 1.0) / 2.0).abs()
                / (AUDIO_SPECTRUM_BANDS as f32 / 2.0));
        let base = 0.45 + center * 0.28;
        let active = if matches!(
            self.state,
            TurnState::Connecting
                | TurnState::Reconnecting
                | TurnState::Thinking
                | TurnState::ToolQueued
                | TurnState::ToolRunning
        ) && !self.reduced_motion
        {
            (self.phase + index as f32 * 0.55).sin().abs() * 0.18
        } else {
            self.bands[index] * 0.4
        };
        (base + active).clamp(0.35, 1.0)
    }

    fn render(self, horizontal: bool) -> AnyElement {
        if horizontal {
            h_flex()
                .h_12()
                .flex_1()
                .items_center()
                .justify_between()
                .children((0..AUDIO_SPECTRUM_BANDS).map(|index| {
                    div()
                        .w_1()
                        .h(relative(self.bar_fraction(index)))
                        .rounded_full()
                        .bg(self.color)
                }))
                .into_any_element()
        } else {
            v_flex()
                .w_full()
                .flex_1()
                .items_center()
                .justify_between()
                .children((0..AUDIO_SPECTRUM_BANDS).map(|index| {
                    div()
                        .w(relative(self.bar_fraction(index)))
                        .h_1()
                        .rounded_full()
                        .bg(self.color)
                }))
                .into_any_element()
        }
    }
}

impl Render for PillView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        let view = cx.entity().downgrade();
        let marker = Button::new("sofia-microphone")
            .ghost()
            .compact()
            .accessibility_label(label)
            .tooltip(label)
            .child(div().size_5().rounded_full().border_2().border_color(color))
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                if let Some(view) = view.upgrade() {
                    view.update(cx, |view, _| view.toggle_listening());
                }
            });
        let expanded = self.expanded;
        let waveform = PillWaveform {
            bands: self.bands,
            phase: self.phase,
            state: self.snapshot.state,
            color,
            reduced_motion: cx.reduce_motion(),
        }
        .render(expanded);
        let content = if expanded {
            let text = if self.assistant_text.is_empty() {
                label.to_string()
            } else {
                self.assistant_text.clone()
            };
            v_flex()
                .size_full()
                .gap_6()
                .p_6()
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap_4()
                        .child(marker)
                        .child(waveform),
                )
                .child(div().text_lg().text_color(color).child(text))
                .into_any_element()
        } else {
            v_flex()
                .size_full()
                .items_center()
                .gap_2()
                .px_2()
                .py_2()
                .child(marker)
                .child(waveform)
                .into_any_element()
        };
        let content = div().size_full().child(content).with_spring(
            "sofia-pill-content-morph",
            SpringAnimation::new(SpringConfig::new(240.0, 28.0, 1.0))
                .to(if expanded { 1.0 } else { 0.0 })
                .from(0.0)
                .with_epsilon(0.001),
            move |this, value| {
                let opacity = if expanded {
                    ((value - 0.55) / 0.45).clamp(0.0, 1.0)
                } else {
                    ((0.45 - value) / 0.45).clamp(0.0, 1.0)
                };
                this.opacity(opacity)
            },
        );
        let view = cx.entity().downgrade();
        let click_view = cx.entity().downgrade();
        let pill = div()
            .id("sofia-pill")
            .relative()
            .overflow_hidden()
            .rounded_full()
            .border_2()
            .border_color(color)
            .bg(theme.background)
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                if let Some(view) = view.upgrade() {
                    view.update(cx, |view, _| view.on_mouse_down(event, window));
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
            self.pill_size = (PILL_WIDTH_REM * rem, PILL_HEIGHT_REM * rem);
            let panel_size = (PANEL_WIDTH_REM * rem, PANEL_HEIGHT_REM * rem);
            let surface = window.bounds().size;
            let ready = surface.width > px(1.) && surface.height > px(1.);
            self.viewport = (f32::from(surface.width), f32::from(surface.height));
            if ready && self.position.is_none() {
                self.position = Some((
                    (self.viewport.0 - self.pill_size.0 - EDGE_GAP_REM * rem).max(0.0),
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
                (self.viewport.0 - panel_size.0 - EDGE_GAP_REM * rem).max(0.0)
            } else {
                EDGE_GAP_REM * rem
            };
            let panel_y = (y + (self.pill_size.1 - panel_size.1) / 2.0)
                .clamp(0.0, (self.viewport.1 - panel_size.1).max(0.0));
            let (input_x, input_y, input_size) = if expanded {
                (panel_x, panel_y, panel_size)
            } else {
                (x, y, self.pill_size)
            };
            if !ready {
                window.set_input_region(Some(&[]));
            } else if self.drag_offset.is_none() {
                window.set_input_region(Some(&[bounds(
                    point(px(input_x), px(input_y)),
                    size(px(input_size.0), px(input_size.1)),
                )]));
            }
            let compact_size = self.pill_size;
            let morph = if expanded { 1.0 } else { 0.0 };
            let pill = pill.opacity(if ready { 1.0 } else { 0.0 }).with_spring(
                "sofia-pill-morph",
                SpringAnimation::new(SpringConfig::new(240.0, 28.0, 1.0))
                    .to(morph)
                    .from(0.0)
                    .with_epsilon(0.001),
                move |this, value| {
                    let value = value.clamp(0.0, 1.0);
                    let mix = |start: f32, end: f32| start + (end - start) * value;
                    this.absolute()
                        .left(px(mix(x, panel_x)))
                        .top(px(mix(y, panel_y)))
                        .w(px(mix(compact_size.0, panel_size.0)))
                        .h(px(mix(compact_size.1, panel_size.1)))
                        .rounded(px(mix(compact_size.0 / 2.0, rem * 1.75)))
                },
            );
            let view = cx.entity().downgrade();
            let view_up = cx.entity().downgrade();
            div()
                .id("sofia-layer")
                .size_full()
                .relative()
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
                .child(pill)
                .into_any_element()
        } else {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(pill.w_12().h_48())
                .into_any_element()
        }
    }
}
