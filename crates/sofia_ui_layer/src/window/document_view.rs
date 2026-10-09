use gpui_kit::assets::IconName;
use gpui_kit::base::input::Copy;
use gpui_kit::base::{ElementExt, TextSelectionScopeId};
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable,
    button::{Button, ButtonVariants as _},
    chart::{AreaChart, BarChart, LineChart, PieChart, RadarChart},
    checkbox::Checkbox,
    input::{Input, InputState},
    text::{MarkdownExtensions, TextView, TextViewState},
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use pulldown_cmark::{Event, Options, Parser};
use sofia_content::{ChartPoint, ChartType, Content, Document, TodoItem};
use std::sync::{Arc, OnceLock, mpsc};

pub(crate) enum Command {
    Save(Document),
    Close(String),
    Refresh,
}

type RenderedDiagram = Option<(Arc<[u8]>, f32, f32)>;

#[derive(Clone)]
pub(crate) struct MermaidBlock {
    pub(crate) code: String,
    pub(crate) rendered: Arc<std::sync::Mutex<RenderedDiagram>>,
}

fn hsla_to_hex(hsla: gpui::Hsla) -> String {
    let rgba = hsla.to_rgb();
    let r = (rgba.r * 255.0).round().clamp(0.0, 255.0) as u8;
    let g = (rgba.g * 255.0).round().clamp(0.0, 255.0) as u8;
    let b = (rgba.b * 255.0).round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", r, g, b)
}

fn strip_foreign_objects(svg: &mut String) {
    // `resvg` cannot paint HTML inside SVG foreignObject nodes. Merman also
    // emits an SVG text fallback for those labels, so retaining both layers
    // produces dark, doubled glyphs. Remove only the unsupported HTML nodes.
    while let Some(start) = svg.find("<foreignObject") {
        let Some(end_rel) = svg[start..].find("</foreignObject>") else {
            svg.truncate(start);
            break;
        };
        let end = start + end_rel + "</foreignObject>".len();
        svg.replace_range(start..end, "");
    }
    while let Some(start) = svg.find("<foreignobject") {
        let Some(end_rel) = svg[start..].find("</foreignobject>") else {
            svg.truncate(start);
            break;
        };
        let end = start + end_rel + "</foreignobject>".len();
        svg.replace_range(start..end, "");
    }
}

fn build_host_theme(theme: &gpui_kit::component::Theme) -> merman::svg::HostTheme {
    use merman::svg::{HostTheme, HostThemeAppearance, ThemeRole};

    let appearance = if theme.mode.is_dark() {
        HostThemeAppearance::Dark
    } else {
        HostThemeAppearance::Light
    };

    let font_family = theme.font_family.to_string();
    let mut host = HostTheme::new().with_appearance(appearance);
    if let Ok(updated) = host
        .clone()
        .try_with_font_family(format!("{font_family}, sans-serif"))
    {
        host = updated;
    }

    if let Ok(h) = host
        .clone()
        .try_with_role(ThemeRole::Canvas, hsla_to_hex(theme.background))
    {
        host = h;
    }
    if let Ok(h) = host
        .clone()
        .try_with_role(ThemeRole::Surface, hsla_to_hex(theme.popover))
    {
        host = h;
    }
    if let Ok(h) = host
        .clone()
        .try_with_role(ThemeRole::SurfaceAlt, hsla_to_hex(theme.muted))
    {
        host = h;
    }
    if let Ok(h) = host
        .clone()
        .try_with_role(ThemeRole::SurfaceMuted, hsla_to_hex(theme.secondary))
    {
        host = h;
    }
    if let Ok(h) = host
        .clone()
        .try_with_role(ThemeRole::Text, hsla_to_hex(theme.foreground))
    {
        host = h;
    }
    if let Ok(h) = host
        .clone()
        .try_with_role(ThemeRole::SubtleText, hsla_to_hex(theme.muted_foreground))
    {
        host = h;
    }
    if let Ok(h) = host
        .clone()
        .try_with_role(ThemeRole::Border, hsla_to_hex(theme.border))
    {
        host = h;
    }
    if let Ok(h) = host
        .clone()
        .try_with_role(ThemeRole::Line, hsla_to_hex(theme.primary))
    {
        host = h;
    }
    if let Ok(h) = host
        .clone()
        .try_with_role(ThemeRole::ClusterBackground, hsla_to_hex(theme.popover))
    {
        host = h;
    }
    if let Ok(h) = host
        .clone()
        .try_with_role(ThemeRole::ClusterBorder, hsla_to_hex(theme.border))
    {
        host = h;
    }
    if let Ok(h) = host.clone().try_with_role(
        ThemeRole::EdgeLabelBackground,
        hsla_to_hex(theme.background),
    ) {
        host = h;
    }
    if let Ok(h) = host
        .clone()
        .try_with_role(ThemeRole::ActorBackground, hsla_to_hex(theme.popover))
    {
        host = h;
    }
    if let Ok(h) = host
        .clone()
        .try_with_role(ThemeRole::ActorBorder, hsla_to_hex(theme.border))
    {
        host = h;
    }
    if let Ok(h) = host
        .clone()
        .try_with_role(ThemeRole::ActorText, hsla_to_hex(theme.foreground))
    {
        host = h;
    }
    if let Ok(h) = host
        .clone()
        .try_with_role(ThemeRole::Error, hsla_to_hex(theme.red))
    {
        host = h;
    }

    let series = [
        hsla_to_hex(theme.primary),
        hsla_to_hex(theme.blue),
        hsla_to_hex(theme.green),
        hsla_to_hex(theme.yellow),
        hsla_to_hex(theme.magenta),
        hsla_to_hex(theme.cyan),
    ];
    if let Ok(h) = host.clone().try_with_series_palette(series) {
        host = h;
    }
    host
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

pub fn parse_markdown_blocks(markdown: &str) -> Vec<std::ops::Range<usize>> {
    let mut blocks = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (event, range) in Parser::new_ext(markdown, Options::all()).into_offset_iter() {
        match event {
            Event::Start(_) => {
                if depth == 0 {
                    start = range.start;
                }
                depth += 1;
            }
            Event::End(_) => {
                depth -= 1;
                if depth == 0 {
                    blocks.push(start..range.end);
                }
            }
            Event::Rule if depth == 0 => blocks.push(range),
            _ => {}
        }
    }
    if blocks.is_empty() {
        blocks.push(0..markdown.len());
    }
    blocks
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
            drag_offset: (0.0, 0.0),
            dragging: false,
            dragged_once: false,
            drag_start_cursor: None,
            drag_start_offset: (0.0, 0.0),
            resizing: false,
            resize_start_cursor: None,
            resize_start_size: initial_size,
        }
    }

    pub(crate) fn apply(&mut self, doc: Document, window: &mut Window, cx: &mut Context<Self>) {
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
        // Always clear the transient pointer state.  The release can arrive
        // after the drag target has left the title bar, so relying on the
        // `dragging` flag here can leave a stale cursor anchor behind.
        let changed = self.dragging || self.drag_start_cursor.is_some();
        self.dragging = false;
        self.drag_start_cursor = None;
        if changed {
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
            .markdown_extensions(Self::note_extensions().clone())
            .into_any_element()
    }

    fn render_mermaid_svg(
        code: &str,
        theme: &gpui_kit::component::Theme,
        fontdb: &Arc<usvg::fontdb::Database>,
    ) -> Result<(Arc<[u8]>, f32, f32), String> {
        use merman::svg::{Presentation, PresentationProfile, SvgPipeline};
        use merman::{OperationControl, RenderOutput, RenderRequest, Renderer, SvgRequest};

        let host_theme = build_host_theme(theme);
        let presentation = Presentation::new()
            .with_profile(PresentationProfile::MermanModern)
            .with_theme(host_theme);
        let resolved = presentation.resolve();
        let renderer =
            Renderer::new().with_engine(resolved.materialize_engine(merman::Engine::new()));

        let request = SvgRequest {
            pipeline: Some(SvgPipeline::resvg_safe()),
            presentation: resolved.render_policy(),
            options: merman::svg::SvgRenderOptions {
                diagram_id: Some("sofia-mermaid-diagram".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let mut svg_str =
            match renderer.render(RenderRequest::svg(code, OperationControl::new(), request)) {
                Ok(RenderOutput::Svg(Some(output))) => output.svg().to_string(),
                Ok(RenderOutput::Svg(None)) => {
                    return Err("Mermaid produced no SVG output".to_string());
                }
                Ok(_) => return Err("Mermaid produced non-SVG output".to_string()),
                Err(e) => return Err(format!("Mermaid render failed: {e}")),
            };

        strip_foreign_objects(&mut svg_str);

        // Remove any hardcoded background styles/fills so diagram background is completely transparent
        svg_str = svg_str.replace("background-color:white", "background-color:transparent");
        svg_str = svg_str.replace("background-color: white", "background-color: transparent");
        svg_str = svg_str.replace("background-color:#ffffff", "background-color:transparent");
        svg_str = svg_str.replace("background-color: #ffffff", "background-color: transparent");
        svg_str = svg_str.replace("background:#ffffff", "background:transparent");
        svg_str = svg_str.replace("background: #ffffff", "background: transparent");
        svg_str = svg_str.replace("background:white", "background:transparent");
        svg_str = svg_str.replace("background: white", "background: transparent");
        svg_str = svg_str.replace("fill=\"#FFFFFF\"", "fill=\"none\"");
        svg_str = svg_str.replace("fill=\"#ffffff\"", "fill=\"none\"");
        svg_str = svg_str.replace("fill=\"white\"", "fill=\"none\"");

        let opt = usvg::Options {
            fontdb: fontdb.clone(),
            ..Default::default()
        };
        let rtree =
            usvg::Tree::from_str(&svg_str, &opt).map_err(|e| format!("SVG parse error: {e}"))?;

        let size = rtree.size();
        let width = size.width();
        let height = size.height();
        let scale = 2.0f32;
        let px_w = (width * scale).ceil().max(1.0) as u32;
        let px_h = (height * scale).ceil().max(1.0) as u32;
        let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
        let mut pixmap = resvg::tiny_skia::Pixmap::new(px_w, px_h)
            .ok_or_else(|| "Failed to allocate diagram bitmap".to_string())?;
        resvg::render(&rtree, transform, &mut pixmap.as_mut());
        let png = pixmap
            .encode_png()
            .map_err(|e| format!("PNG encoding error: {e}"))?;
        Ok((Arc::from(png.into_boxed_slice()), width, height))
    }

    fn note_extensions() -> &'static MarkdownExtensions {
        static EXTENSIONS: OnceLock<MarkdownExtensions> = OnceLock::new();
        static FONTDB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();

        EXTENSIONS.get_or_init(|| {
            let fontdb = FONTDB.get_or_init(|| {
                let mut db = usvg::fontdb::Database::new();
                db.load_system_fonts();
                Arc::new(db)
            });

            MarkdownExtensions::default()
                .block_parser(|node, _ctx| {
                    if let markdown::mdast::Node::Code(code) = node
                        && code.lang.as_deref() == Some("mermaid")
                    {
                        let block = MermaidBlock {
                            code: code.value.clone(),
                            rendered: Arc::new(std::sync::Mutex::new(None)),
                        };
                        return Some(gpui_kit::base::MarkdownNode::new("mermaid", block));
                    }
                    None
                })
                .block_renderer("mermaid", {
                    let fontdb = fontdb.clone();
                    move |node, _window, cx| {
                        if let Some(block) = node.data::<MermaidBlock>() {
                            let mut guard = block.rendered.lock().unwrap();
                            if guard.is_none() {
                                *guard =
                                    Self::render_mermaid_svg(&block.code, cx.theme(), &fontdb).ok();
                            }

                            if let Some((png, width, height)) = guard.as_ref() {
                                return div()
                                    .w_full()
                                    .my_3()
                                    .flex()
                                    .justify_center()
                                    .items_center()
                                    .child(
                                        gpui::img(Arc::new(gpui::Image::from_bytes(
                                            gpui::ImageFormat::Png,
                                            png.to_vec(),
                                        )))
                                        .w(px(*width))
                                        .h(px(*height))
                                        .max_w_full(),
                                    )
                                    .into_any_element();
                            }

                            // Keep the Markdown source visible when the
                            // optional diagram renderer cannot parse a block.
                            return div()
                                .w_full()
                                .my_3()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("Diagram could not be rendered")
                                .child(
                                    div()
                                        .w_full()
                                        .p_2()
                                        .rounded_md()
                                        .bg(cx.theme().muted.opacity(0.35))
                                        .child(block.code.clone()),
                                )
                                .into_any_element();
                        }
                        div().into_any_element()
                    }
                })
        })
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
                        .p_1()
                        .rounded_md()
                        .bg(cx.theme().muted.opacity(0.3))
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
                        .p_1()
                        .rounded_md()
                        .hover(|s| s.bg(cx.theme().muted.opacity(0.5)))
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
        let labels = points
            .iter()
            .map(|point| point.label.clone())
            .collect::<Vec<_>>();
        let colors = [
            cx.theme().red,
            cx.theme().yellow,
            cx.theme().green,
            cx.theme().cyan,
            cx.theme().blue,
            cx.theme().magenta,
        ];
        match chart_type {
            ChartType::Line => LineChart::new(points.to_vec())
                .x(|point: &ChartPoint| point.label.clone())
                .y(|point: &ChartPoint| point.value)
                .stroke(cx.theme().blue)
                .dot()
                .y_axis(true)
                .appear_key(self.doc.revision)
                .into_any_element(),
            ChartType::Bar => BarChart::new(points.to_vec())
                .band(|point: &ChartPoint| point.label.clone())
                .value(|point: &ChartPoint| point.value)
                .fill(move |point: &ChartPoint, _, _, _| {
                    colors[labels
                        .iter()
                        .position(|label| label == &point.label)
                        .unwrap_or(0)
                        % colors.len()]
                })
                .appear_key(self.doc.revision)
                .into_any_element(),
            ChartType::Area => AreaChart::new(points.to_vec())
                .x(|point: &ChartPoint| point.label.clone())
                .y(|point: &ChartPoint| point.value)
                .stroke(cx.theme().blue)
                .fill(cx.theme().blue.opacity(0.2))
                .appear_key(self.doc.revision)
                .into_any_element(),
            ChartType::Pie => PieChart::new(points.to_vec())
                .value(|point: &ChartPoint| point.value as f32)
                .label(|point: &ChartPoint| point.label.clone().into())
                .color(move |point: &ChartPoint| {
                    colors[labels
                        .iter()
                        .position(|label| label == &point.label)
                        .unwrap_or(0)
                        % colors.len()]
                })
                .appear_key(self.doc.revision)
                .into_any_element(),
            ChartType::Radar => RadarChart::new(points.to_vec())
                .label(|point: &ChartPoint| point.label.clone())
                .value(|point: &ChartPoint| point.value)
                .stroke(cx.theme().blue)
                .fill(cx.theme().blue.opacity(0.2))
                .dot()
                .appear_key(self.doc.revision)
                .into_any_element(),
        }
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

        let mut root = div()
            .id(SharedString::from(format!("doc-view-root-{}", self.doc.id)))
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .text_color(cx.theme().foreground)
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
