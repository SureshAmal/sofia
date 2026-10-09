//! Modular content window system for Sofia.

pub mod document_view;
pub mod layout;
pub mod manager;

pub use document_view::parse_markdown_blocks;
pub use layout::{Placement, panel_bounds};
pub use manager::WindowManager;

#[cfg(test)]
mod tests {
    use super::document_view::parse_markdown_blocks;
    use super::layout::{Placement, panel_bounds};
    use sofia_content::{Content, Document};

    #[test]
    fn markdown_editor_keeps_top_level_blocks_intact() {
        let source =
            "# Title\n\nA **bold** paragraph.\n\n- one\n- two\n\n```rust\nfn main() {}\n```";
        let blocks = parse_markdown_blocks(source);
        assert_eq!(
            blocks
                .iter()
                .map(|range| &source[range.clone()])
                .collect::<Vec<_>>(),
            [
                "# Title\n",
                "A **bold** paragraph.\n",
                "- one\n- two\n\n",
                "```rust\nfn main() {}\n```"
            ]
        );
    }

    #[test]
    fn test_mermaid_diagram_renders_to_svg() {
        use merman::svg::{
            HostTheme, HostThemePreset, Presentation, PresentationProfile, SvgPipeline,
        };
        use merman::{OperationControl, RenderOutput, RenderRequest, Renderer, SvgRequest};

        let diagram = "graph TD;\n    A-->B;\n    A-->C;\n    B-->D;\n    C-->D;";
        let presentation = Presentation::new()
            .with_profile(PresentationProfile::MermanModern)
            .with_theme(HostTheme::from_preset(HostThemePreset::OneDark));
        let resolved = presentation.resolve();
        let renderer =
            Renderer::new().with_engine(resolved.materialize_engine(merman::Engine::new()));

        let request = SvgRequest {
            pipeline: Some(SvgPipeline::resvg_safe()),
            presentation: resolved.render_policy(),
            ..Default::default()
        };

        let output = renderer
            .render(RenderRequest::svg(
                diagram,
                OperationControl::new(),
                request,
            ))
            .expect("Mermaid rendering should succeed");
        let svg = match output {
            RenderOutput::Svg(Some(out)) => out.svg().to_string(),
            _ => panic!("Expected SVG output"),
        };
        assert!(
            svg.contains("<svg"),
            "Rendered output should contain <svg tag"
        );

        let mut opt = usvg::Options::default();
        let mut fontdb = usvg::fontdb::Database::new();
        fontdb.load_system_fonts();
        opt.fontdb = std::sync::Arc::new(fontdb);
        let rtree = usvg::Tree::from_str(&svg, &opt).unwrap();
        let pixmap_size = rtree.size().to_int_size();
        let mut pixmap =
            resvg::tiny_skia::Pixmap::new(pixmap_size.width(), pixmap_size.height()).unwrap();
        resvg::render(
            &rtree,
            resvg::tiny_skia::Transform::default(),
            &mut pixmap.as_mut(),
        );
        let png = pixmap.encode_png().unwrap();
        assert!(!png.is_empty());
    }

    #[test]
    fn test_merman_render() {
        use merman::svg::{
            HostTheme, HostThemePreset, Presentation, PresentationProfile, SvgPipeline,
        };
        use merman::{OperationControl, RenderOutput, RenderRequest, Renderer, SvgRequest};

        let diagram = r#"
flowchart TD
    Client[Instagram Mobile Client] --> HTTPS[HTTPS]
    HTTPS --> LB[Load Balancer]
    subgraph Core[Core Services]
        API[API Gateway / Web]
    end
    subgraph Storage[Static Assets/Images]
        CDN[CDN]
    end
    LB --> API
    LB --> CDN
"#;
        let presentation = Presentation::new()
            .with_profile(PresentationProfile::MermanModern)
            .with_theme(HostTheme::from_preset(HostThemePreset::OneDark));
        let resolved = presentation.resolve();
        let renderer =
            Renderer::new().with_engine(resolved.materialize_engine(merman::Engine::new()));
        let request = SvgRequest {
            pipeline: Some(SvgPipeline::resvg_safe()),
            presentation: resolved.render_policy(),
            ..Default::default()
        };
        let output = renderer
            .render(RenderRequest::svg(
                diagram,
                OperationControl::new(),
                request,
            ))
            .expect("merman render failed");
        if let RenderOutput::Svg(Some(svg_output)) = output {
            let svg = svg_output.svg();
            std::fs::write("/tmp/diagram.svg", &svg).unwrap();
            eprintln!("SVG saved to /tmp/diagram.svg, len: {}", svg.len());
        }
    }

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

        let b = panel_bounds(&doc, (0., 0.), 0, pill, pill_size, viewport, rem);
        let expected_w = 20. * rem;
        let expected_h = 15. * rem;
        assert_eq!(f32::from(b.size.width), expected_w);
        assert_eq!(f32::from(b.size.height), expected_h);
        assert_eq!(f32::from(b.origin.x), (1920. - expected_w) / 2.);
        assert_eq!(f32::from(b.origin.y), (1080. - expected_h) / 2.);

        // Drag offset
        let b_drag = panel_bounds(&doc, (50., -30.), 0, pill, pill_size, viewport, rem);
        assert_eq!(f32::from(b_drag.origin.x), (1920. - expected_w) / 2. + 50.);
        assert_eq!(f32::from(b_drag.origin.y), (1080. - expected_h) / 2. - 30.);

        // Top-left placement
        doc.tags = vec!["pos:top_left".into()];
        let b_tl = panel_bounds(&doc, (0., 0.), 0, pill, pill_size, viewport, rem);
        assert_eq!(f32::from(b_tl.origin.x), rem * 0.75);
        assert_eq!(f32::from(b_tl.origin.y), rem * 0.75);

        // Top-right placement
        doc.tags = vec!["pos:top_right".into()];
        let b_tr = panel_bounds(&doc, (0., 0.), 0, pill, pill_size, viewport, rem);
        assert_eq!(f32::from(b_tr.origin.x), 1920. - expected_w - rem * 0.75);
        assert_eq!(f32::from(b_tr.origin.y), rem * 0.75);

        // Clamping within viewport
        let b_clamped = panel_bounds(&doc, (5000., 5000.), 0, pill, pill_size, viewport, rem);
        assert_eq!(
            f32::from(b_clamped.origin.x),
            1920. - expected_w - rem * 0.5
        );
        assert_eq!(
            f32::from(b_clamped.origin.y),
            1080. - expected_h - rem * 0.5
        );
    }
}
