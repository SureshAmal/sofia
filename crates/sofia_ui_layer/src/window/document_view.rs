use gpui_kit::assets::IconName;
use gpui_kit::base::input::Copy;
use gpui_kit::base::{ElementExt, TextSelectionScopeId};
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    input::{Input, InputState},
    text::{TextView, TextViewState},
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use sofia_content::{ChartPoint, ChartType, Content, Document, TodoItem};
use std::sync::mpsc;

use super::views::{card_view, chart_view, note_view};

pub use note_view::parse_markdown_blocks;

pub(crate) enum Command {
    Save(Document),
    Close(String),
    Refresh,
}

pub(crate) struct DocumentView {
    pub(crate) doc: Document,
    pub(crate) scope_id: TextSelectionScopeId,
    pub(crate) remote: Option<Document>,
    pub(crate) commands: mpsc::Sender<Command>,
    pub(crate) text: Entity<TextViewState>,
    pub(crate) new_item: Entity<InputState>,
    pub(crate) due: Entity<InputState>,
    pub(crate) editing_item: Option<String>,
    pub(crate) pending_item: Option<TodoItem>,
    pub(crate) status: String,
    pub(crate) hovered: bool,
    pub(crate) drag_offset: (f32, f32),
    pub(crate) dragging: bool,
    pub(crate) dragged_once: bool,
    pub(crate) drag_start_cursor: Option<(f32, f32)>,
    pub(crate) drag_start_offset: (f32, f32),
    pub(crate) resizing: bool,
    pub(crate) resize_start_cursor: Option<(f32, f32)>,
    pub(crate) resize_start_size: (f32, f32),
}

#[derive(Clone)]
pub(crate) struct WindowDrag(pub(crate) EntityId);

impl Render for WindowDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

#[derive(Clone)]
pub(crate) struct WindowResize(pub(crate) EntityId);

impl Render for WindowResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

fn source(content: &Content) -> String {
    match content {
        Content::Note { markdown } => markdown.clone(),
        Content::Html { html } => html.clone(),
        _ => serde_json::to_string_pretty(content).unwrap_or_default(),
    }
}

impl DocumentView {
    pub(crate) fn new(
        doc: Document,
        commands: mpsc::Sender<Command>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let initial_size = (doc.width_rem, doc.height_rem);
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
        let initial_offset = doc
            .tags
            .iter()
            .find_map(|t| {
                let coords = t.strip_prefix("offset:")?;
                let (x_str, y_str) = coords.split_once(',')?;
                let x = x_str.trim().parse::<f32>().ok()?;
                let y = y_str.trim().parse::<f32>().ok()?;
                Some((x, y))
            })
            .unwrap_or((0.0, 0.0));

        Self {
            doc,
            scope_id: TextSelectionScopeId::new(),
            remote: None,
            commands,
            text,
            new_item,
            due,
            editing_item: None,
            pending_item: None,
            status: String::new(),
            hovered: false,
            drag_offset: initial_offset,
            dragging: false,
            dragged_once: initial_offset != (0.0, 0.0),
            drag_start_cursor: None,
            drag_start_offset: (0.0, 0.0),
            resizing: false,
            resize_start_cursor: None,
            resize_start_size: initial_size,
        }
    }

    pub(crate) fn apply(&mut self, doc: Document, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing_item.is_some() {
            // User is actively editing an item; stage remote updates so they don't overwrite typing.
            self.remote = Some(doc);
            self.status = "Remote update available".into();
            cx.notify();
            return;
        }

        let content = source(&doc.content);
        self.text.update(cx, |state, cx| {
            state.set_text(&content, cx);
        });
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

    pub(crate) fn on_drag_start(&mut self, cursor: (f32, f32), window: &mut Window) {
        self.dragging = true;
        self.dragged_once = true;
        self.drag_start_cursor = Some(cursor);
        self.drag_start_offset = self.drag_offset;
        window.set_input_region(None);
    }

    pub(crate) fn on_drag_move_event(&mut self, cursor: (f32, f32), cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        let Some((start_x, start_y)) = self.drag_start_cursor else {
            return;
        };
        let dx = cursor.0 - start_x;
        let dy = cursor.1 - start_y;
        self.drag_offset = (self.drag_start_offset.0 + dx, self.drag_start_offset.1 + dy);
        cx.notify();
    }

    pub(crate) fn on_drag_end(&mut self, cx: &mut Context<Self>) {
        let changed = self.dragging || self.drag_start_cursor.is_some();
        self.dragging = false;
        self.drag_start_cursor = None;
        if changed {
            // Persist the user's dragged position into tags as offset:x,y
            let mut doc = self.doc.clone();
            doc.tags.retain(|t| !t.starts_with("offset:"));
            doc.tags.push(format!("offset:{:.1},{:.1}", self.drag_offset.0, self.drag_offset.1));
            self.doc = doc.clone();
            let _ = self.commands.send(Command::Save(doc));
            cx.notify();
        }
    }

    pub(crate) fn on_resize_start(&mut self, cursor: (f32, f32), window: &mut Window) {
        self.resizing = true;
        self.resize_start_cursor = Some(cursor);
        self.resize_start_size = (self.doc.width_rem, self.doc.height_rem);
        window.set_input_region(None);
    }

    pub(crate) fn on_resize_move(
        &mut self,
        cursor: (f32, f32),
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if !self.resizing {
            return;
        }
        let Some((start_x, start_y)) = self.resize_start_cursor else {
            return;
        };
        let rem = f32::from(window.rem_size()).max(1.0);
        self.doc.width_rem =
            (self.resize_start_size.0 + (cursor.0 - start_x) / rem).clamp(16.0, 96.0);
        self.doc.height_rem =
            (self.resize_start_size.1 + (cursor.1 - start_y) / rem).clamp(8.0, 64.0);
        cx.notify();
    }

    pub(crate) fn on_resize_end(&mut self, cx: &mut Context<Self>) {
        if !self.resizing {
            self.resize_start_cursor = None;
            return;
        }
        self.resizing = false;
        self.resize_start_cursor = None;
        let _ = self.commands.send(Command::Save(self.doc.clone()));
        self.status = "Saving…".into();
        cx.notify();
    }

    pub(crate) fn todo_change(&mut self, index: usize, checked: bool, cx: &mut Context<Self>) {
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

    pub(crate) fn edit_item(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(crate) fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing_item = None;
        self.new_item
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.due
            .update(cx, |state, cx| state.set_value("", window, cx));
        cx.notify();
    }

    pub(crate) fn remove_item(&mut self, index: usize, cx: &mut Context<Self>) {
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

    pub(crate) fn add_item(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
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

    // --- Component Render Methods on DocumentView ---

    fn render_title_bar(&self, cx: &Context<Self>) -> AnyElement {
        let kind_icon = match &self.doc.content {
            Content::Note { .. } => IconName::FileText,
            Content::Todo { .. } => IconName::SquareCheck,
            Content::Reminder { .. } => IconName::Bell,
            Content::Chart { .. } => IconName::ChartBar,
            Content::Html { .. } => IconName::Globe,
            Content::Card { .. } => IconName::LayoutDashboard,
        };

        let drag_handle = div()
            .id(SharedString::from(format!("drag-handle-{}", self.doc.id)))
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap_2()
            .cursor_move()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, event: &MouseDownEvent, window, cx| {
                    gpui_kit::base::GlobalState::suppress_text_selection(cx);
                    view.on_drag_start(
                        (f32::from(event.position.x), f32::from(event.position.y)),
                        window,
                    );
                }),
            )
            .on_drag(WindowDrag(cx.entity_id()), |drag, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| drag.clone())
            })
            .on_drag_move(
                cx.listener(|view, event: &DragMoveEvent<WindowDrag>, _, cx| {
                    if event.drag(cx).0 == cx.entity_id() {
                        let cursor = (
                            f32::from(event.event.position.x),
                            f32::from(event.event.position.y),
                        );
                        view.on_drag_move_event(cursor, cx);
                    }
                }),
            )
            .child(
                Icon::new(kind_icon)
                    .size(px(15.0))
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                div()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_sm()
                    .child(self.doc.title.clone()),
            );

        let mut bar = div()
            .id(SharedString::from(format!("title-bar-{}", self.doc.id)))
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .on_hover(cx.listener(|view, hovered, _, cx| {
                if view.hovered != *hovered {
                    view.hovered = *hovered;
                    cx.notify();
                }
            }))
            .child(drag_handle);

        if self.hovered {
            if let Content::Note { markdown } = &self.doc.content {
                let note_markdown = markdown.clone();
                let note_id = self.doc.id.clone();
                bar = bar.child(
                    Button::new("open-external-btn")
                        .ghost()
                        .xsmall()
                        .icon(IconName::ExternalLink)
                        .tooltip("Open in external editor")
                        .on_click(cx.listener(move |_view, _, _, _| {
                            let temp_dir = std::env::temp_dir();
                            let file_path = temp_dir.join(format!("sofia_note_{}.md", note_id));
                            if std::fs::write(&file_path, &note_markdown).is_ok() {
                                let _ = open::that(&file_path);
                            }
                        })),
                );
            }

            let doc_id = self.doc.id.clone();
            bar = bar.child(
                Button::new("close-window-btn")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Close)
                    .tooltip("Close window")
                    .on_click(cx.listener(move |view, _, _, _| {
                        let _ = view.commands.send(Command::Close(doc_id.clone()));
                    })),
            );
        }

        bar.into_any_element()
    }

    fn render_note(&self, _cx: &Context<Self>) -> AnyElement {
        TextView::new(&self.text)
            .selectable(true)
            .scrollable(true)
            .size_full()
            .markdown_extensions(note_view::note_extensions().clone())
            .into_any_element()
    }

    fn render_todo_items(&self, items: &[TodoItem], cx: &Context<Self>) -> AnyElement {
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
            let is_editing = self.editing_item.as_ref().is_some_and(|id| id == &item.id);

            if is_editing {
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .py_0p5()
                        .child(
                            Checkbox::new(SharedString::from(item.id.clone()))
                                .checked(item.done)
                                .on_click(cx.listener(move |view, value: &bool, _, cx| {
                                    view.todo_change(index, *value, cx);
                                })),
                        )
                        .child(div().flex_1().child(Input::new(&self.new_item).w_full()))
                        .child(
                            Button::new(SharedString::from(format!("save-edit-{}", item.id)))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Check)
                                .tooltip("Save")
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.add_item(window, cx);
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("cancel-edit-{}", item.id)))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Close)
                                .tooltip("Cancel")
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.cancel_edit(window, cx);
                                })),
                        ),
                );
            } else {
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .py_0p5()
                        .child(
                            Checkbox::new(SharedString::from(item.id.clone()))
                                .checked(item.done)
                                .on_click(cx.listener(move |view, value: &bool, _, cx| {
                                    view.todo_change(index, *value, cx);
                                })),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("todo-text-{}", item.id)))
                                .flex_1()
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.edit_item(index, window, cx);
                                }))
                                .child(
                                    div()
                                        .text_sm()
                                        .when(item.done, |this| this.line_through())
                                        .opacity(if item.done { 0.6 } else { 1.0 })
                                        .child(label),
                                 ),
                        )
                        .child(
                            Button::new(SharedString::from(format!("remove-{}", item.id)))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Trash)
                                .tooltip("Remove")
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.remove_item(index, cx);
                                })),
                        ),
                );
            }
        }
        list.into_any_element()
    }

    fn render_chart(
        &self,
        chart_type: &ChartType,
        points: &[ChartPoint],
        cx: &Context<Self>,
    ) -> AnyElement {
        let chart_id = SharedString::from(format!("chart-{}", self.doc.id));
        chart_view::render_chart(chart_id, chart_type, points, cx)
    }

    fn render_card(&self, root: &sofia_content::ui::UiNode, cx: &Context<Self>) -> AnyElement {
        div()
            .id(SharedString::from(format!("card-body-{}", self.doc.id)))
            .flex_1()
            .w_full()
            .overflow_y_scroll()
            .child(card_view::render_generative_node(root, cx))
            .into_any_element()
    }
}

impl Render for DocumentView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title_bar = self.render_title_bar(cx);

        let body = match &self.doc.content {
            Content::Note { .. } => self.render_note(cx),
            Content::Html { .. } => TextView::new(&self.text)
                .selectable(true)
                .scrollable(true)
                .size_full()
                .into_any_element(),
            Content::Todo { items } | Content::Reminder { items } => {
                self.render_todo_items(items, cx)
            }
            Content::Chart { chart_type, points } => self.render_chart(chart_type, points, cx),
            Content::Card { root } => self.render_card(root, cx),
        };

        let mut footer = div()
            .flex()
            .items_center()
            .gap_3()
            .child(div().flex_1().text_sm().child(self.status.clone()));

        if self.remote.is_some() {
            footer = footer.child(
                Button::new("reload")
                    .ghost()
                    .xsmall()
                    .label("Reload")
                    .on_click(cx.listener(|view, _, window, cx| {
                        if let Some(doc) = view.remote.take() {
                            view.apply(doc, window, cx);
                        }
                    })),
            );
        }

        let scope_id = self.scope_id;
        let mut root = div()
            .id(SharedString::from(format!("doc-view-root-{}", self.doc.id)))
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .text_color(cx.theme().foreground)
            .on_mouse_down(MouseButton::Left, move |_event, window, cx| {
                gpui_kit::base::TextSelection::activate_scope(scope_id, window, cx);
            })
            .on_action(cx.listener(|_, _: &Copy, window, cx| {
                let text = gpui_kit::base::TextSelection::selected_text(window, cx);
                if !text.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }))
            .child(title_bar)
            .child(body);

        if matches!(
            self.doc.content,
            Content::Todo { .. } | Content::Reminder { .. }
        ) && self.editing_item.is_none()
        {
            root = root.child(
                div()
                    .flex()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.new_item).w_full()))
                    .child(div().w(px(140.0)).child(Input::new(&self.due).w_full()))
                    .child(
                        Button::new("add")
                            .ghost()
                            .small()
                            .icon(IconName::Plus)
                            .tooltip("Add item")
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.add_item(window, cx);
                            })),
                    ),
            );
        }

        let resize_handle = div()
            .id(SharedString::from(format!("resize-handle-{}", self.doc.id)))
            .absolute()
            .right_0()
            .bottom_0()
            .w(rems(1.0))
            .h(rems(1.0))
            .cursor(gpui::CursorStyle::ResizeUpRightDownLeft)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, event: &MouseDownEvent, window, cx| {
                    gpui_kit::base::GlobalState::suppress_text_selection(cx);
                    view.on_resize_start(
                        (f32::from(event.position.x), f32::from(event.position.y)),
                        window,
                    );
                }),
            )
            .on_drag(WindowResize(cx.entity_id()), |drag, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| drag.clone())
            })
            .on_drag_move(
                cx.listener(|view, event: &DragMoveEvent<WindowResize>, window, cx| {
                    if event.drag(cx).0 == cx.entity_id() {
                        view.on_resize_move(
                            (
                                f32::from(event.event.position.x),
                                f32::from(event.event.position.y),
                            ),
                            window,
                            cx,
                        );
                    }
                }),
            );

        root.child(footer)
            .child(resize_handle)
            .text_selection_scope(self.scope_id)
            .into_any_element()
    }
}
