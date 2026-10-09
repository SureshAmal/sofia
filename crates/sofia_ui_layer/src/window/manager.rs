//! Window manager orchestrating panels, animations, and IPC sync.
use gpui_kit::component::ActiveTheme;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use sofia_content::{Document, Store};
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use super::document_view::{Command, DocumentView};
use super::layout::panel_bounds;

pub(crate) enum Update {
    Documents(Vec<Document>),
    Error(String, String),
}

pub(crate) struct Panel {
    pub(crate) view: Entity<DocumentView>,
    pub(crate) closing: Option<Instant>,
    pub(crate) generation: u64,
    pub(crate) slot: usize,
    pub(crate) anchor: Option<(f32, f32)>,
    pub(crate) _subscription: Option<Subscription>,
}

static NEXT_PANEL_GENERATION: AtomicU64 = AtomicU64::new(1);

pub struct WindowManager {
    panels: Vec<Panel>,
    commands: mpsc::Sender<Command>,
    updates: mpsc::Receiver<Update>,
    pending: Vec<Update>,
}

impl WindowManager {
    pub fn new() -> Self {
        let (commands, requests) = mpsc::channel();
        let (sender, updates) = mpsc::channel();
        std::thread::spawn(move || {
            let store = match Store::default_path().and_then(Store::open) {
                Ok(store) => store,
                Err(error) => {
                    let _ = sender.send(Update::Error(String::new(), error));
                    return;
                }
            };
            let mut previous = Vec::new();
            loop {
                match requests.recv_timeout(Duration::from_millis(250)) {
                    Ok(Command::Save(doc)) => {
                        let id = doc.id.clone();
                        let revision = doc.revision;
                        if let Err(error) = store.update(doc, revision) {
                            let _ = sender.send(Update::Error(id, error));
                        }
                    }
                    Ok(Command::Close(id)) => {
                        if let Err(error) = store.set_open(&id, false) {
                            let _ = sender.send(Update::Error(id, error));
                        }
                    }
                    Ok(Command::Refresh) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
                match store.open_documents() {
                    Ok(docs) if docs != previous => {
                        previous = docs.clone();
                        if sender.send(Update::Documents(docs)).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        if sender.send(Update::Error(String::new(), error)).is_err() {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        });
        Self {
            panels: vec![],
            commands,
            updates,
            pending: vec![],
        }
    }

    pub fn refresh(&self) {
        let _ = self.commands.send(Command::Refresh);
    }

    pub fn tick(&mut self, _cx: &mut App) -> bool {
        let mut changed = false;
        while let Ok(update) = self.updates.try_recv() {
            self.pending.push(update);
            changed = true;
        }
        if self.panels.iter().any(|panel| panel.closing.is_some()) {
            changed = true;
            self.panels.retain(|panel| {
                !panel
                    .closing
                    .is_some_and(|time| time.elapsed() > Duration::from_millis(700))
            });
        }
        changed
    }

    pub fn sync(&mut self, window: &mut Window, cx: &mut Context<crate::pill::PillView>) {
        for update in std::mem::take(&mut self.pending) {
            match update {
                Update::Documents(docs) => {
                    let ids: HashSet<_> = docs.iter().map(|doc| doc.id.clone()).collect();
                    for panel in &mut self.panels {
                        if !ids.contains(&panel.view.read(cx).doc.id) && panel.closing.is_none() {
                            panel.closing = Some(Instant::now());
                        }
                    }
                    for doc in docs {
                        if let Some(panel) = self
                            .panels
                            .iter_mut()
                            .find(|panel| panel.view.read(cx).doc.id == doc.id)
                        {
                            panel.closing = None;
                            if panel.view.read(cx).doc.revision < doc.revision {
                                panel
                                    .view
                                    .update(cx, |view, cx| view.apply(doc, window, cx));
                            }
                        } else {
                            let commands = self.commands.clone();
                            let view = cx.new(|cx| DocumentView::new(doc, commands, window, cx));
                            let subscription = cx.observe(&view, |_this, _view, cx| cx.notify());
                            let used_slots: HashSet<usize> =
                                self.panels.iter().map(|p| p.slot).collect();
                            let slot = (0..).find(|s| !used_slots.contains(s)).unwrap_or(0);
                            self.panels.push(Panel {
                                view,
                                closing: None,
                                generation: NEXT_PANEL_GENERATION.fetch_add(1, Ordering::Relaxed),
                                slot,
                                anchor: None,
                                _subscription: Some(subscription),
                            });
                        }
                    }
                }
                Update::Error(id, error) => {
                    if let Some(panel) = self
                        .panels
                        .iter()
                        .find(|panel| panel.view.read(cx).doc.id == id)
                    {
                        panel.view.update(cx, |view, cx| {
                            view.status = error;
                            cx.notify();
                        });
                    } else {
                        eprintln!("Sofia content: {error}");
                    }
                }
            }
        }
    }

    pub fn is_dragging(&self, cx: &App) -> bool {
        self.panels.iter().any(|panel| panel.view.read(cx).dragging)
    }

    pub fn on_drag_end(&self, cx: &mut App) {
        for panel in &self.panels {
            if panel.view.read(cx).dragging {
                panel.view.update(cx, |view, cx| {
                    view.on_drag_end(cx);
                });
            }
        }
    }

    pub fn regions(
        &mut self,
        pill: (f32, f32),
        pill_size: (f32, f32),
        viewport: (f32, f32),
        rem: f32,
        cx: &App,
    ) -> Vec<Bounds<Pixels>> {
        self.panels
            .iter_mut()
            .map(|panel| {
                let anchor = *panel.anchor.get_or_insert(pill);
                let view = panel.view.read(cx);
                panel_bounds(
                    &view.doc,
                    view.drag_offset,
                    panel.slot,
                    anchor,
                    pill_size,
                    viewport,
                    rem,
                )
            })
            .collect()
    }

    pub fn bring_to_top(&mut self, id: &str, cx: &App) {
        if let Some(pos) = self
            .panels
            .iter()
            .position(|panel| panel.view.read(cx).doc.id == id)
            && pos + 1 < self.panels.len()
        {
            let panel = self.panels.remove(pos);
            self.panels.push(panel);
        }
    }

    pub fn render(
        &mut self,
        pill: (f32, f32),
        pill_size: (f32, f32),
        viewport: (f32, f32),
        rem: f32,
        parent: Entity<crate::pill::PillView>,
        cx: &App,
    ) -> Vec<AnyElement> {
        self.panels
            .iter_mut()
            .map(|panel| {
                let anchor = *panel.anchor.get_or_insert(pill);
                let view = panel.view.read(cx);
                let target = panel_bounds(
                    &view.doc,
                    view.drag_offset,
                    panel.slot,
                    anchor,
                    pill_size,
                    viewport,
                    rem,
                );
                let closing = panel.closing.is_some();
                let dragging = view.dragging;
                let dragged_once = view.dragged_once;
                let radius = f32::from(cx.theme().radius_lg);
                let doc_id = view.doc.id.clone();
                let id = SharedString::from(format!(
                    "content-window-{}-{}",
                    view.doc.id, panel.generation
                ));
                let parent_focus = parent.clone();
                let focus_id = doc_id.clone();
                let content = div().size_full().child(
                    panel
                        .view
                        .clone()
                        .cached(StyleRefinement::default().size_full()),
                );
                let content = if dragging || dragged_once {
                    content.into_any_element()
                } else {
                    content
                        .with_spring(
                            SharedString::from(format!("{id}-content")),
                            SpringAnimation::new(SpringConfig::new(250., 30., 1.))
                                .to(if closing { 0. } else { 1. })
                                .from(0.),
                            |this, value| this.opacity(((value - 0.2) / 0.8).clamp(0., 1.)),
                        )
                        .into_any_element()
                };
                let element = div()
                    .id(id.clone())
                    .absolute()
                    .occlude()
                    .overflow_hidden()
                    .rounded(cx.theme().radius_lg)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background.opacity(0.95))
                    .when(!dragging, |this| this.shadow_lg())
                    .on_hover({
                        let view_hover = panel.view.clone();
                        move |hovered, _, cx| {
                            view_hover.update(cx, |view, cx| {
                                if !view.dragging && view.hovered != *hovered {
                                    view.hovered = *hovered;
                                    cx.notify();
                                }
                            });
                        }
                    })
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let id = focus_id.clone();
                        parent_focus.update(cx, |pill, cx| {
                            pill.documents.bring_to_top(&id, cx);
                            cx.notify();
                        });
                    })
                    .child(content);
                if dragging || dragged_once {
                    element
                        .left(target.origin.x)
                        .top(target.origin.y)
                        .w(target.size.width)
                        .h(target.size.height)
                        .into_any_element()
                } else {
                    element
                        .with_spring(
                            SharedString::from(format!("{id}-morph")),
                            SpringAnimation::new(SpringConfig::new(280., 28., 1.))
                                .to(if closing { 0. } else { 1. })
                                .from(0.),
                            move |this, value| {
                                let t = value.clamp(0., 1.);
                                let mix = |from: f32, to: f32| from + (to - from) * t;
                                this.left(px(mix(pill.0, f32::from(target.origin.x))))
                                    .top(px(mix(pill.1, f32::from(target.origin.y))))
                                    .w(px(mix(pill_size.0, f32::from(target.size.width))))
                                    .h(px(mix(pill_size.1, f32::from(target.size.height))))
                                    .rounded(px(mix(pill_size.0 / 2., radius)))
                            },
                        )
                        .into_any_element()
                }
            })
            .collect()
    }
}

impl Default for WindowManager {
    fn default() -> Self {
        Self::new()
    }
}
