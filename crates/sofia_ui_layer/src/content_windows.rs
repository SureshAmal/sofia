//! Native GPUI Kit presenters in the existing desktop layer.
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, Icon,
    button::Button,
    chart::{AreaChart, BarChart, LineChart, PieChart, RadarChart},
    checkbox::Checkbox,
    input::{Input, InputState},
    text::{TextView, TextViewState},
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use sofia_content::{ChartPoint, ChartType, Content, Document, Store, TodoItem};
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

pub(crate) enum Command {
    Save(Document),
    Close(String),
    Refresh,
}

enum Update {
    Documents(Vec<Document>),
    Error(String, String),
}

struct Panel {
    view: Entity<DocumentView>,
    closing: Option<Instant>,
    generation: u64,
    _subscription: Option<Subscription>,
}

static NEXT_PANEL_GENERATION: AtomicU64 = AtomicU64::new(1);

pub(crate) struct WindowManager {
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

    pub fn tick(&mut self) -> bool {
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
                            if panel.view.read(cx).doc.revision != doc.revision {
                                panel
                                    .view
                                    .update(cx, |view, cx| view.apply(doc, window, cx));
                            }
                        } else {
                            let commands = self.commands.clone();
                            let view = cx.new(|cx| DocumentView::new(doc, commands, window, cx));
                            let subscription = cx.observe(&view, |_this, _view, cx| cx.notify());
                            self.panels.push(Panel {
                                view,
                                closing: None,
                                generation: NEXT_PANEL_GENERATION.fetch_add(1, Ordering::Relaxed),
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

    pub fn regions(
        &self,
        pill: (f32, f32),
        pill_size: (f32, f32),
        viewport: (f32, f32),
        rem: f32,
        cx: &App,
    ) -> Vec<Bounds<Pixels>> {
        self.panels
            .iter()
            .enumerate()
            .map(|(index, panel)| {
                let view = panel.view.read(cx);
                panel_bounds(
                    &view.doc,
                    view.drag_offset,
                    view.minimized,
                    index,
                    pill,
                    pill_size,
                    viewport,
                    rem,
                )
            })
            .collect()
    }

    pub fn render(
        &self,
        pill: (f32, f32),
        pill_size: (f32, f32),
        viewport: (f32, f32),
        rem: f32,
        cx: &App,
    ) -> Vec<AnyElement> {
        self.panels
            .iter()
            .enumerate()
            .map(|(index, panel)| {
                let view = panel.view.read(cx);
                let target = panel_bounds(
                    &view.doc,
                    view.drag_offset,
                    view.minimized,
                    index,
                    pill,
                    pill_size,
                    viewport,
                    rem,
                );
                let closing = panel.closing.is_some();
                let id = SharedString::from(format!(
                    "content-window-{}-{}",
                    view.doc.id,
                    panel.generation
                ));
                let view_entity = panel.view.clone();
                let view_up = panel.view.clone();
                div()
                    .id(id.clone())
                    .absolute()
                    .overflow_hidden()
                    .rounded_xl()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background.opacity(0.95))
                    .shadow_lg()
                    .on_mouse_move(move |event, _, cx| {
                        view_entity.update(cx, |view, cx| view.on_mouse_move(event, cx));
                    })
                    .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                        view_up.update(cx, |view, cx| view.on_mouse_up(cx));
                    })
                    .child(
                        div().size_full().child(panel.view.clone()).with_spring(
                            SharedString::from(format!("{id}-content")),
                            SpringAnimation::new(SpringConfig::new(250., 30., 1.))
                                .to(if closing { 0. } else { 1. })
                                .from(0.),
                            |this, value| this.opacity(((value - 0.2) / 0.8).clamp(0., 1.)),
                        ),
                    )
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
                                .rounded(px(mix(pill_size.0 / 2., rem * 0.75)))
                        },
                    )
                    .into_any_element()
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Placement {
    #[default]
    Pill,
    Center,
    Left,
    Right,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Placement {
    pub fn parse(tags: &[String]) -> Self {
        for tag in tags {
            if let Some(pos) = tag.strip_prefix("pos:") {
                match pos.trim().to_ascii_lowercase().as_str() {
                    "center" => return Self::Center,
                    "left" => return Self::Left,
                    "right" => return Self::Right,
                    "bottom" => return Self::Bottom,
                    "top_left" | "topleft" | "top-left" => return Self::TopLeft,
                    "top_right" | "topright" | "top-right" => return Self::TopRight,
                    "bottom_left" | "bottomleft" | "bottom-left" => return Self::BottomLeft,
                    "bottom_right" | "bottomright" | "bottom-right" => return Self::BottomRight,
                    "pill" => return Self::Pill,
                    _ => {}
                }
            }
        }
        Self::Pill
    }

    pub fn label(&self) -> Option<&'static str> {
        match self {
            Self::Pill => None,
            Self::Center => Some("center"),
            Self::Left => Some("left"),
            Self::Right => Some("right"),
            Self::Bottom => Some("bottom"),
            Self::TopLeft => Some("top-left"),
            Self::TopRight => Some("top-right"),
            Self::BottomLeft => Some("bottom-left"),
            Self::BottomRight => Some("bottom-right"),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn panel_bounds(
    doc: &Document,
    drag_offset: (f32, f32),
    minimized: bool,
    index: usize,
    pill: (f32, f32),
    pill_size: (f32, f32),
    viewport: (f32, f32),
    rem: f32,
) -> Bounds<Pixels> {
    let width = (doc.width_rem * rem).min((viewport.0 - rem * 2.).max(rem));
    let normal_height = (doc.height_rem * rem).min((viewport.1 - rem * 2.).max(rem));
    let height = if minimized {
        (2.5 * rem).min(normal_height)
    } else {
        normal_height
    };

    let cascade = index as f32 * rem;
    let placement = Placement::parse(&doc.tags);

    let (base_x, base_y) = match placement {
        Placement::Pill => {
            let right = pill.0 + pill_size.0 / 2. >= viewport.0 / 2.;
            let x = if right {
                pill.0 - width - rem * 0.75 - cascade
            } else {
                pill.0 + pill_size.0 + rem * 0.75 + cascade
            };
            let y = pill.1 + (pill_size.1 - height) / 2. + cascade;
            (x, y)
        }
        Placement::Center => {
            let x = (viewport.0 - width) / 2. + cascade;
            let y = (viewport.1 - height) / 2. + cascade;
            (x, y)
        }
        Placement::Left => {
            let x = rem * 0.75 + cascade;
            let y = (viewport.1 - height) / 2. + cascade;
            (x, y)
        }
        Placement::Right => {
            let x = viewport.0 - width - rem * 0.75 - cascade;
            let y = (viewport.1 - height) / 2. + cascade;
            (x, y)
        }
        Placement::Bottom => {
            let x = (viewport.0 - width) / 2. + cascade;
            let y = viewport.1 - height - rem * 0.75 - cascade;
            (x, y)
        }
        Placement::TopLeft => {
            let x = rem * 0.75 + cascade;
            let y = rem * 0.75 + cascade;
            (x, y)
        }
        Placement::TopRight => {
            let x = viewport.0 - width - rem * 0.75 - cascade;
            let y = rem * 0.75 + cascade;
            (x, y)
        }
        Placement::BottomLeft => {
            let x = rem * 0.75 + cascade;
            let y = viewport.1 - height - rem * 0.75 - cascade;
            (x, y)
        }
        Placement::BottomRight => {
            let x = viewport.0 - width - rem * 0.75 - cascade;
            let y = viewport.1 - height - rem * 0.75 - cascade;
            (x, y)
        }
    };

    let x = base_x + drag_offset.0;
    let y = base_y + drag_offset.1;

    let margin = rem * 0.5;
    let clamped_x = x.clamp(margin, (viewport.0 - width - margin).max(margin));
    let clamped_y = y.clamp(margin, (viewport.1 - height - margin).max(margin));

    bounds(
        point(px(clamped_x), px(clamped_y)),
        size(px(width), px(height)),
    )
}

pub(crate) struct DocumentView {
    pub(crate) doc: Document,
    pub(crate) remote: Option<Document>,
    pub(crate) commands: mpsc::Sender<Command>,
    pub(crate) text: Entity<TextViewState>,
    pub(crate) new_item: Entity<InputState>,
    pub(crate) due: Entity<InputState>,
    pub(crate) editing_item: Option<String>,
    pub(crate) pending_item: Option<TodoItem>,
    pub(crate) status: String,
    pub(crate) minimized: bool,
    pub(crate) drag_offset: (f32, f32),
    pub(crate) dragging: bool,
    pub(crate) drag_start: Option<(f32, f32)>,
}

fn source(content: &Content) -> String {
    match content {
        Content::Note { markdown } => markdown.clone(),
        Content::Html { html } => html.clone(),
        _ => serde_json::to_string_pretty(content).unwrap_or_default(),
    }
}

impl DocumentView {
    fn new(
        doc: Document,
        commands: mpsc::Sender<Command>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let content = source(&doc.content);
        let text = cx.new(|cx| {
            if matches!(doc.content, Content::Html { .. }) {
                TextViewState::html(&content, cx).selectable(true)
            } else {
                TextViewState::markdown(&content, cx).selectable(true)
            }
        });
        let new_item = cx.new(|cx| InputState::new(window, cx).placeholder("New item"));
        let due =
            cx.new(|cx| InputState::new(window, cx).placeholder("Due time (optional ISO8601)"));
        Self {
            doc,
            remote: None,
            commands,
            text,
            new_item,
            due,
            editing_item: None,
            pending_item: None,
            status: String::new(),
            minimized: false,
            drag_offset: (0.0, 0.0),
            dragging: false,
            drag_start: None,
        }
    }

    fn apply(&mut self, doc: Document, window: &mut Window, cx: &mut Context<Self>) {
        let content = source(&doc.content);
        self.text
            .update(cx, |state, cx| state.set_text(&content, cx));
        if let Some(pending) = &self.pending_item
            && let Content::Todo { items } | Content::Reminder { items } = &doc.content
            && items.iter().any(|item| item == pending)
        {
            self.new_item
                .update(cx, |state, cx| state.set_value("", window, cx));
            self.due
                .update(cx, |state, cx| state.set_value("", window, cx));
            self.editing_item = None;
            self.pending_item = None;
        }
        self.doc = doc;
        self.remote = None;
        self.status = "Saved".into();
        cx.notify();
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dragging = true;
        self.drag_start = Some((f32::from(event.position.x), f32::from(event.position.y)));
        window.set_input_region(None);
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        if !event.dragging() {
            self.dragging = false;
            self.drag_start = None;
            cx.notify();
            return;
        }
        if let Some((start_x, start_y)) = self.drag_start {
            let current_x = f32::from(event.position.x);
            let current_y = f32::from(event.position.y);
            let dx = current_x - start_x;
            let dy = current_y - start_y;
            self.drag_offset.0 += dx;
            self.drag_offset.1 += dy;
            self.drag_start = Some((current_x, current_y));
            cx.notify();
        }
    }

    fn on_mouse_up(&mut self, cx: &mut Context<Self>) {
        if self.dragging {
            self.dragging = false;
            self.drag_start = None;
            cx.notify();
        }
    }

    fn todo_change(&mut self, index: usize, checked: bool, cx: &mut Context<Self>) {
        let mut doc = self.doc.clone();
        if let Content::Todo { items } | Content::Reminder { items } = &mut doc.content
            && let Some(item) = items.get_mut(index)
        {
            item.done = checked;
            let _ = self.commands.send(Command::Save(doc));
            self.status = "Saving…".into();
            cx.notify();
        }
    }

    fn edit_item(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Content::Todo { items } | Content::Reminder { items } = &self.doc.content
            && let Some(item) = items.get(index).cloned()
        {
            self.editing_item = Some(item.id);
            self.new_item
                .update(cx, |state, cx| state.set_value(item.text, window, cx));
            self.due.update(cx, |state, cx| {
                state.set_value(item.due_at.unwrap_or_default(), window, cx)
            });
            cx.notify();
        }
    }

    fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing_item = None;
        self.new_item
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.due
            .update(cx, |state, cx| state.set_value("", window, cx));
        cx.notify();
    }

    fn remove_item(&mut self, index: usize, cx: &mut Context<Self>) {
        let mut doc = self.doc.clone();
        if let Content::Todo { items } | Content::Reminder { items } = &mut doc.content
            && index < items.len()
        {
            let removed = items.remove(index);
            if self.editing_item.as_ref() == Some(&removed.id) {
                self.editing_item = None;
            }
            let _ = self.commands.send(Command::Save(doc));
            cx.notify();
        }
    }

    fn add_item(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let text = self.new_item.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }
        let due_at = self.due.read(cx).value().trim().to_string();
        let mut doc = self.doc.clone();
        if let Content::Todo { items } | Content::Reminder { items } = &mut doc.content {
            let item = TodoItem {
                id: self
                    .editing_item
                    .clone()
                    .unwrap_or_else(|| format!("item-{}-{}", doc.revision, items.len())),
                text,
                done: self
                    .editing_item
                    .as_ref()
                    .and_then(|id| items.iter().find(|item| &item.id == id))
                    .is_some_and(|item| item.done),
                due_at: (!due_at.is_empty()).then_some(due_at),
            };
            if self.editing_item.is_some() {
                let Some(existing) = items.iter_mut().find(|existing| existing.id == item.id)
                else {
                    self.status = "This item was removed elsewhere.".into();
                    cx.notify();
                    return;
                };
                *existing = item.clone();
            } else {
                items.push(item.clone());
            }
            self.pending_item = Some(item);
            let _ = self.commands.send(Command::Save(doc));
            self.status = "Saving…".into();
            cx.notify();
        }
    }
}

impl Render for DocumentView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let kind_icon = match &self.doc.content {
            Content::Note { .. } => IconName::FileText,
            Content::Todo { .. } => IconName::SquareCheck,
            Content::Reminder { .. } => IconName::Bell,
            Content::Chart { .. } => IconName::ChartBar,
            Content::Html { .. } => IconName::Globe,
        };

        let kind_badge = self.doc.content.kind();
        let placement = Placement::parse(&self.doc.tags);

        let mut title_bar = div()
            .flex()
            .items_center()
            .gap_2()
            .cursor_move()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, event: &MouseDownEvent, window, cx| {
                    view.on_mouse_down(event, window, cx);
                }),
            )
            .child(
                Icon::new(kind_icon)
                    .size(px(15.0))
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_sm()
                    .child(self.doc.title.clone()),
            )
            .child(
                div()
                    .text_xs()
                    .px_1p5()
                    .py_0p5()
                    .rounded_sm()
                    .bg(cx.theme().muted)
                    .text_color(cx.theme().muted_foreground)
                    .child(kind_badge),
            );

        if let Some(badge) = placement.label() {
            title_bar = title_bar.child(
                div()
                    .text_xs()
                    .px_1p5()
                    .py_0p5()
                    .rounded_sm()
                    .bg(cx.theme().muted)
                    .text_color(cx.theme().muted_foreground)
                    .child(badge),
            );
        }

        title_bar = title_bar
            .child(
                div()
                    .cursor_pointer()
                    .p_1()
                    .rounded_md()
                    .hover(|s| s.bg(cx.theme().muted))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, _, _, cx| {
                            view.minimized = !view.minimized;
                            cx.notify();
                        }),
                    )
                    .child(
                        Icon::new(if self.minimized {
                            IconName::ChevronDown
                        } else {
                            IconName::Minus
                        })
                        .size(px(14.0))
                        .text_color(cx.theme().muted_foreground),
                    ),
            )
            .child(
                div()
                    .cursor_pointer()
                    .p_1()
                    .rounded_md()
                    .hover(|s| s.bg(cx.theme().danger.opacity(0.15)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, _, _, _| {
                            let _ = view.commands.send(Command::Close(view.doc.id.clone()));
                        }),
                    )
                    .child(
                        Icon::new(IconName::X)
                            .size(px(14.0))
                            .text_color(cx.theme().muted_foreground),
                    ),
            );

        if self.minimized {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .justify_center()
                .px_4()
                .text_color(cx.theme().foreground)
                .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _, cx| {
                    view.on_mouse_move(event, cx);
                }))
                .on_mouse_up(MouseButton::Left, cx.listener(|view, _, _, cx| {
                    view.on_mouse_up(cx);
                }))
                .child(title_bar)
                .into_any_element();
        }

        let body = match &self.doc.content {
            Content::Note { .. } | Content::Html { .. } => TextView::new(&self.text)
                .selectable(true)
                .scrollable(true)
                .size_full()
                .into_any_element(),
            Content::Todo { items } | Content::Reminder { items } => {
                let mut list = div()
                    .id("todo-list")
                    .flex()
                    .flex_col()
                    .gap_2()
                    .overflow_y_scroll();
                for (index, item) in items.iter().enumerate() {
                    let label = if let Some(due) = &item.due_at {
                        format!("{} · {}", item.text, due)
                    } else {
                        item.text.clone()
                    };
                    let is_editing = self
                        .editing_item
                        .as_ref()
                        .is_some_and(|id| id == &item.id);
                    list = list.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .p_1()
                            .rounded_md()
                            .hover(|s| s.bg(cx.theme().muted.opacity(0.5)))
                            .child(
                                Checkbox::new(SharedString::from(item.id.clone()))
                                    .checked(item.done)
                                    .on_click(cx.listener(
                                        move |view, value: &bool, _, cx| {
                                            view.todo_change(index, *value, cx)
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |view, _, window, cx| {
                                            view.edit_item(index, window, cx)
                                        }),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .when(item.done, |this| this.line_through())
                                            .opacity(if item.done { 0.6 } else { 1.0 })
                                            .font_weight(if is_editing {
                                                FontWeight::SEMIBOLD
                                            } else {
                                                FontWeight::NORMAL
                                            })
                                            .child(label),
                                    ),
                            )
                            .child(
                                Button::new(SharedString::from(format!("remove-{}", item.id)))
                                    .label("Remove")
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.remove_item(index, cx)
                                    })),
                            ),
                    );
                }
                list.into_any_element()
            }
            Content::Chart { chart_type, points } => match chart_type {
                ChartType::Line => LineChart::new(points.clone())
                    .x(|point: &ChartPoint| point.label.clone())
                    .y(|point: &ChartPoint| point.value)
                    .stroke(cx.theme().blue)
                    .y_axis(true)
                    .appear_key(self.doc.revision)
                    .into_any_element(),
                ChartType::Bar => BarChart::new(points.clone())
                    .band(|point: &ChartPoint| point.label.clone())
                    .value(|point: &ChartPoint| point.value)
                    .appear_key(self.doc.revision)
                    .into_any_element(),
                ChartType::Area => AreaChart::new(points.clone())
                    .x(|point: &ChartPoint| point.label.clone())
                    .y(|point: &ChartPoint| point.value)
                    .stroke(cx.theme().blue)
                    .appear_key(self.doc.revision)
                    .into_any_element(),
                ChartType::Pie => PieChart::new(points.clone())
                    .value(|point: &ChartPoint| point.value as f32)
                    .label(|point: &ChartPoint| point.label.clone().into())
                    .appear_key(self.doc.revision)
                    .into_any_element(),
                ChartType::Radar => RadarChart::new(points.clone())
                    .label(|point: &ChartPoint| point.label.clone())
                    .value(|point: &ChartPoint| point.value)
                    .stroke(cx.theme().blue)
                    .appear_key(self.doc.revision)
                    .into_any_element(),
            },
        };

        let mut footer = div()
            .flex()
            .items_center()
            .gap_3()
            .child(div().flex_1().text_sm().child(self.status.clone()));

        if self.remote.is_some() {
            footer = footer.child(Button::new("reload").label("Reload saved").on_click(
                cx.listener(|view, _, window, cx| {
                    if let Some(doc) = view.remote.take() {
                        view.apply(doc, window, cx);
                    }
                }),
            ));
        }

        let mut root = div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .text_color(cx.theme().foreground)
            .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _, cx| {
                view.on_mouse_move(event, cx);
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|view, _, _, cx| {
                view.on_mouse_up(cx);
            }))
            .child(title_bar)
            .child(div().flex_1().min_h_0().overflow_hidden().child(body));

        if matches!(
            self.doc.content,
            Content::Todo { .. } | Content::Reminder { .. }
        ) {
            let is_editing = self.editing_item.is_some();
            let mut controls = div()
                .flex()
                .gap_2()
                .child(div().flex_1().child(Input::new(&self.due).w_full()))
                .child(
                    Button::new("add")
                        .label(if is_editing {
                            "Save item"
                        } else {
                            "Add"
                        })
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.add_item(window, cx)
                        })),
                );
            if is_editing {
                controls = controls.child(
                    Button::new("cancel-edit")
                        .label("Cancel")
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.cancel_edit(window, cx);
                        })),
                );
            }
            root = root.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(Input::new(&self.new_item).w_full())
                    .child(controls),
            );
        }

        root.child(footer).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{panel_bounds, Placement};
    use sofia_content::{Content, Document};

    #[test]
    fn test_placement_parsing() {
        assert_eq!(Placement::parse(&[]), Placement::Pill);
        assert_eq!(
            Placement::parse(&["note".into(), "important".into()]),
            Placement::Pill
        );
        assert_eq!(Placement::parse(&["pos:center".into()]), Placement::Center);
        assert_eq!(Placement::parse(&["pos:left".into()]), Placement::Left);
        assert_eq!(Placement::parse(&["pos:right".into()]), Placement::Right);
        assert_eq!(Placement::parse(&["pos:bottom".into()]), Placement::Bottom);
        assert_eq!(
            Placement::parse(&["pos:top_left".into()]),
            Placement::TopLeft
        );
        assert_eq!(
            Placement::parse(&["pos:top-left".into()]),
            Placement::TopLeft
        );
        assert_eq!(
            Placement::parse(&["pos:top_right".into()]),
            Placement::TopRight
        );
        assert_eq!(
            Placement::parse(&["pos:bottom_left".into()]),
            Placement::BottomLeft
        );
        assert_eq!(
            Placement::parse(&["pos:bottom_right".into()]),
            Placement::BottomRight
        );
        assert_eq!(Placement::parse(&["pos:pill".into()]), Placement::Pill);
        assert_eq!(Placement::parse(&["pos:unknown".into()]), Placement::Pill);
    }

    #[test]
    fn test_placement_labels() {
        assert_eq!(Placement::Pill.label(), None);
        assert_eq!(Placement::Center.label(), Some("center"));
        assert_eq!(Placement::Left.label(), Some("left"));
        assert_eq!(Placement::Right.label(), Some("right"));
        assert_eq!(Placement::Bottom.label(), Some("bottom"));
        assert_eq!(Placement::TopLeft.label(), Some("top-left"));
        assert_eq!(Placement::TopRight.label(), Some("top-right"));
        assert_eq!(Placement::BottomLeft.label(), Some("bottom-left"));
        assert_eq!(Placement::BottomRight.label(), Some("bottom-right"));
    }

    #[test]
    fn test_panel_bounds_placements() {
        let mut doc = Document {
            id: "doc-1".into(),
            title: "Test".into(),
            tags: vec!["pos:center".into()],
            content: Content::Note {
                markdown: "hello".into(),
            },
            open: true,
            revision: 1,
            updated_at: 0,
            width_rem: 20.,
            height_rem: 15.,
        };
        let viewport = (1920., 1080.);
        let rem = 16.;
        let pill = (960., 20.);
        let pill_size = (200., 40.);

        let b = panel_bounds(&doc, (0., 0.), false, 0, pill, pill_size, viewport, rem);
        let expected_w = 20. * rem;
        let expected_h = 15. * rem;
        assert_eq!(f32::from(b.size.width), expected_w);
        assert_eq!(f32::from(b.size.height), expected_h);
        assert_eq!(f32::from(b.origin.x), (1920. - expected_w) / 2.);
        assert_eq!(f32::from(b.origin.y), (1080. - expected_h) / 2.);

        // Minimized bounds height
        let b_min = panel_bounds(&doc, (0., 0.), true, 0, pill, pill_size, viewport, rem);
        assert_eq!(f32::from(b_min.size.height), 2.5 * rem);

        // Drag offset
        let b_drag = panel_bounds(&doc, (50., -30.), false, 0, pill, pill_size, viewport, rem);
        assert_eq!(f32::from(b_drag.origin.x), (1920. - expected_w) / 2. + 50.);
        assert_eq!(f32::from(b_drag.origin.y), (1080. - expected_h) / 2. - 30.);

        // Top-left placement
        doc.tags = vec!["pos:top_left".into()];
        let b_tl = panel_bounds(&doc, (0., 0.), false, 0, pill, pill_size, viewport, rem);
        assert_eq!(f32::from(b_tl.origin.x), rem * 0.75);
        assert_eq!(f32::from(b_tl.origin.y), rem * 0.75);

        // Top-right placement
        doc.tags = vec!["pos:top_right".into()];
        let b_tr = panel_bounds(&doc, (0., 0.), false, 0, pill, pill_size, viewport, rem);
        assert_eq!(f32::from(b_tr.origin.x), 1920. - expected_w - rem * 0.75);
        assert_eq!(f32::from(b_tr.origin.y), rem * 0.75);

        // Clamping within viewport
        let b_clamped =
            panel_bounds(&doc, (5000., 5000.), false, 0, pill, pill_size, viewport, rem);
        assert_eq!(f32::from(b_clamped.origin.x), 1920. - expected_w - rem * 0.5);
        assert_eq!(f32::from(b_clamped.origin.y), 1080. - expected_h - rem * 0.5);
    }
}
