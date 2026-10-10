use gpui_kit::*;
use merman::svg::HostTheme;
use pulldown_cmark::{Event, Options, Parser};
use std::sync::{Arc, OnceLock};

#[derive(Clone)]
pub(crate) struct MermaidBlock {
    pub(crate) code: String,
    pub(crate) rendered: Arc<std::sync::Mutex<Option<(Arc<gpui::Image>, f32, f32)>>>,
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

fn hsla_to_hex(hsla: gpui::Hsla) -> String {
    let rgba = hsla.to_rgb();
    let r = (rgba.r * 255.0).round().clamp(0.0, 255.0) as u8;
    let g = (rgba.g * 255.0).round().clamp(0.0, 255.0) as u8;
    let b = (rgba.b * 255.0).round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", r, g, b)
}

fn strip_foreign_objects(svg: &mut String) {
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

pub fn build_host_theme(theme: &gpui_kit::component::Theme) -> HostTheme {
    use merman::svg::{HostThemeAppearance, ThemeRole};

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

    let roles = [
        (ThemeRole::Canvas, hsla_to_hex(theme.background)),
        (ThemeRole::Surface, hsla_to_hex(theme.popover)),
        (ThemeRole::SurfaceAlt, hsla_to_hex(theme.muted)),
        (ThemeRole::SurfaceMuted, hsla_to_hex(theme.secondary)),
        (ThemeRole::Text, hsla_to_hex(theme.foreground)),
        (ThemeRole::SubtleText, hsla_to_hex(theme.muted_foreground)),
        (ThemeRole::Border, hsla_to_hex(theme.border)),
        (ThemeRole::Line, hsla_to_hex(theme.primary)),
        (ThemeRole::ClusterBackground, hsla_to_hex(theme.popover)),
        (ThemeRole::ClusterBorder, hsla_to_hex(theme.border)),
        (ThemeRole::EdgeLabelBackground, hsla_to_hex(theme.background)),
        (ThemeRole::ActorBackground, hsla_to_hex(theme.popover)),
        (ThemeRole::ActorBorder, hsla_to_hex(theme.border)),
        (ThemeRole::ActorText, hsla_to_hex(theme.foreground)),
        (ThemeRole::Error, hsla_to_hex(theme.red)),
    ];
    for (role, color) in roles {
        if let Ok(h) = host.clone().try_with_role(role, color) {
            host = h;
        }
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

pub fn render_mermaid_svg(
    code: &str,
    theme: &gpui_kit::component::Theme,
    fontdb: &Arc<usvg::fontdb::Database>,
) -> Result<(Arc<gpui::Image>, f32, f32), String> {
    use merman::svg::{Presentation, PresentationProfile, SvgPipeline};
    use merman::{OperationControl, RenderOutput, RenderRequest, Renderer, SvgRequest};

    let host_theme = build_host_theme(theme);
    let presentation = Presentation::new()
        .with_profile(PresentationProfile::MermanModern)
        .with_theme(host_theme);
    let resolved = presentation.resolve();
    let renderer = Renderer::new().with_engine(resolved.materialize_engine(merman::Engine::new()));

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
    let image = Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, png));
    Ok((image, width, height))
}

pub fn note_extensions() -> &'static gpui_kit::component::text::MarkdownExtensions {
    use gpui_kit::component::ActiveTheme;
    use gpui_kit::component::text::MarkdownExtensions;

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
                            *guard = render_mermaid_svg(&block.code, cx.theme(), &fontdb).ok();
                        }

                        if let Some((image, width, height)) = guard.as_ref() {
                            return div()
                                .w_full()
                                .my_3()
                                .flex()
                                .justify_center()
                                .items_center()
                                .child(
                                    gpui::img(image.clone())
                                        .w(px(*width))
                                        .h(px(*height))
                                        .max_w_full(),
                                )
                                .into_any_element();
                        }

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
