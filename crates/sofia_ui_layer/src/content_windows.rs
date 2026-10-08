//! Native GPUI Kit presenters in the existing desktop layer.
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme,
    button::Button,
    chart::{BarChart, LineChart},
    checkbox::Checkbox,
    input::{Input, InputState, Textarea, TextareaState},
    text::{TextView, TextViewState},
};
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

enum Command {
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
                            self.panels.push(Panel {
                                view: cx.new(|cx| DocumentView::new(doc, commands, window, cx)),
                                closing: None,
                                generation: NEXT_PANEL_GENERATION.fetch_add(1, Ordering::Relaxed),
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
                panel_bounds(
                    &panel.view.read(cx).doc,
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
                let target = panel_bounds(
                    &panel.view.read(cx).doc,
                    index,
                    pill,
                    pill_size,
                    viewport,
                    rem,
                );
                let closing = panel.closing.is_some();
                let id = SharedString::from(format!(
                    "content-window-{}-{}",
                    panel.view.read(cx).doc.id,
                    panel.generation
                ));
                div()
                    .id(id.clone())
                    .absolute()
                    .overflow_hidden()
                    .rounded_xl()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background.opacity(0.9))
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
                        SpringAnimation::new(SpringConfig::new(250., 30., 1.))
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
fn panel_bounds(
    doc: &Document,
    index: usize,
    pill: (f32, f32),
    pill_size: (f32, f32),
    viewport: (f32, f32),
    rem: f32,
) -> Bounds<Pixels> {
    let width = (doc.width_rem * rem).min((viewport.0 - rem * 2.).max(rem));
    let height = (doc.height_rem * rem).min((viewport.1 - rem * 2.).max(rem));
    let right = pill.0 + pill_size.0 / 2. >= viewport.0 / 2.;
    let offset = index as f32 * rem;
    let x = if right {
        pill.0 - width - rem * 0.75 - offset
    } else {
        pill.0 + pill_size.0 + rem * 0.75 + offset
    };
    let y = pill.1 + (pill_size.1 - height) / 2. + offset;
    bounds(
        point(
            px(x.clamp(rem * 0.5, (viewport.0 - width - rem * 0.5).max(rem * 0.5))),
            px(y.clamp(rem * 0.5, (viewport.1 - height - rem * 0.5).max(rem * 0.5))),
        ),
        size(px(width), px(height)),
    )
}
struct DocumentView {
    doc: Document,
    remote: Option<Document>,
    commands: mpsc::Sender<Command>,
    editor: Entity<TextareaState>,
    text: Entity<TextViewState>,
    new_item: Entity<InputState>,
    due: Entity<InputState>,
    editing: bool,
    editing_item: Option<String>,
    pending_item: Option<TodoItem>,
    status: String,
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
        let editor = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(12)
                .default_value(content.clone())
        });
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
            editor,
            text,
            new_item,
            due,
            editing: false,
            editing_item: None,
            pending_item: None,
            status: String::new(),
        }
    }
    fn apply(&mut self, doc: Document, window: &mut Window, cx: &mut Context<Self>) {
        let buffer = self.editor.read(cx).value().to_string();
        if self.editing && buffer != source(&self.doc.content) && buffer != source(&doc.content) {
            self.remote = Some(doc);
            self.status = "Updated elsewhere. Keep your draft or reload the saved version.".into();
            cx.notify();
            return;
        }
        let content = source(&doc.content);
        self.editor
            .update(cx, |state, cx| state.set_value(&content, window, cx));
        self.text
            .update(cx, |state, cx| state.set_text(&content, cx));
        if let Some(pending) = &self.pending_item {
            if let Content::Todo { items } | Content::Reminder { items } = &doc.content {
                if items.iter().any(|item| item == pending) {
                    self.new_item
                        .update(cx, |state, cx| state.set_value("", window, cx));
                    self.due
                        .update(cx, |state, cx| state.set_value("", window, cx));
                    self.editing_item = None;
                    self.pending_item = None;
                }
            }
        }
        self.doc = doc;
        self.remote = None;
        self.status = "Saved".into();
        cx.notify();
    }
    fn save(&mut self, cx: &mut Context<Self>) {
        let value = self.editor.read(cx).value().to_string();
        let mut doc = self.doc.clone();
        let content = match doc.content {
            Content::Note { .. } => Content::Note { markdown: value },
            Content::Html { .. } => Content::Html { html: value },
            _ => match serde_json::from_str::<Content>(&value) {
                Ok(content) => content,
                Err(_) => {
                    self.status = "Invalid content JSON".into();
                    cx.notify();
                    return;
                }
            },
        };
        if content.kind() != doc.content.kind() {
            self.status = "Document kind cannot change".into();
            cx.notify();
            return;
        }
        doc.content = content;
        match doc.validate() {
            Ok(()) => {
                self.status = "Saving…".into();
                let _ = self.commands.send(Command::Save(doc));
            }
            Err(error) => self.status = error,
        }
        cx.notify();
    }
    fn todo_change(&mut self, index: usize, checked: bool, cx: &mut Context<Self>) {
        let mut doc = self.doc.clone();
        if let Content::Todo { items } | Content::Reminder { items } = &mut doc.content {
            items[index].done = checked;
            let _ = self.commands.send(Command::Save(doc));
            self.status = "Saving…".into();
            cx.notify();
        }
    }
    fn edit_item(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Content::Todo { items } | Content::Reminder { items } = &self.doc.content {
            let item = items[index].clone();
            self.editing_item = Some(item.id);
            self.new_item
                .update(cx, |state, cx| state.set_value(item.text, window, cx));
            self.due.update(cx, |state, cx| {
                state.set_value(item.due_at.unwrap_or_default(), window, cx)
            });
            cx.notify();
        }
    }
    fn remove_item(&mut self, index: usize, cx: &mut Context<Self>) {
        let mut doc = self.doc.clone();
        if let Content::Todo { items } | Content::Reminder { items } = &mut doc.content {
            items.remove(index);
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
        let body = if self.editing {
            Textarea::new(&self.editor).size_full().into_any_element()
        } else {
            match &self.doc.content {
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
                        .gap_3()
                        .overflow_y_scroll();
                    for (index, item) in items.iter().enumerate() {
                        let label = if let Some(due) = &item.due_at {
                            format!("{} · {}", item.text, due)
                        } else {
                            item.text.clone()
                        };
                        list = list.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div().flex_1().child(
                                        Checkbox::new(SharedString::from(item.id.clone()))
                                            .label(label)
                                            .checked(item.done)
                                            .on_click(cx.listener(
                                                move |view, value: &bool, _, cx| {
                                                    view.todo_change(index, *value, cx)
                                                },
                                            )),
                                    ),
                                )
                                .child(
                                    Button::new(SharedString::from(format!("edit-{}", item.id)))
                                        .label("Edit")
                                        .on_click(cx.listener(move |view, _, window, cx| {
                                            view.edit_item(index, window, cx)
                                        })),
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
                },
            }
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
                        view.editing = false;
                        view.apply(doc, window, cx);
                    }
                }),
            ));
        }
        if self.editing {
            footer = footer.child(
                Button::new("save")
                    .icon(IconName::Check)
                    .text_color(cx.theme().foreground)
                    .on_click(cx.listener(|view, _, _, cx| view.save(cx))),
            );
        }
        let mut root = div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.doc.title.clone()),
                    )
                    .child(
                        Button::new("edit")
                            .icon(if self.editing {
                                IconName::Eye
                            } else {
                                IconName::Pencil
                            })
                            .text_color(cx.theme().foreground)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.editing = !view.editing;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("close")
                            .icon(IconName::X)
                            .text_color(cx.theme().foreground)
                            .on_click(cx.listener(|view, _, _, _| {
                                let _ = view.commands.send(Command::Close(view.doc.id.clone()));
                            })),
                    ),
            )
            .child(div().flex_1().min_h_0().overflow_hidden().child(body));
        if !self.editing
            && matches!(
                self.doc.content,
                Content::Todo { .. } | Content::Reminder { .. }
            )
        {
            root =
                root.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(Input::new(&self.new_item).w_full())
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(div().flex_1().child(Input::new(&self.due).w_full()))
                                .child(
                                    Button::new("add")
                                        .label(if self.editing_item.is_some() {
                                            "Save item"
                                        } else {
                                            "Add"
                                        })
                                        .on_click(cx.listener(|view, _, window, cx| {
                                            view.add_item(window, cx)
                                        })),
                                ),
                        ),
                );
        }
        root.child(footer)
    }
}
