//! Modular content window system for Sofia.

pub mod document_view;
pub mod layout;
pub mod manager;

pub use document_view::parse_markdown_blocks;
pub use layout::{panel_bounds, Placement};
pub use manager::WindowManager;

#[cfg(test)]
mod tests {
    use super::document_view::parse_markdown_blocks;
    use super::layout::{panel_bounds, Placement};
    use sofia_content::{Content, Document};

    #[test]
    fn markdown_editor_keeps_top_level_blocks_intact() {
        let source = "# Title\n\nA **bold** paragraph.\n\n- one\n- two\n\n```rust\nfn main() {}\n```";
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
        let diagram = "graph TD;\n    A-->B;\n    A-->C;\n    B-->D;\n    C-->D;";
        let rendered = mermaid_rs_renderer::render(diagram);
        assert!(rendered.is_ok(), "Mermaid rendering should succeed");
        let svg = rendered.unwrap();
        assert!(svg.contains("<svg"), "Rendered output should contain <svg tag");
        assert!(svg.contains("</svg>"), "Rendered output should contain </svg> tag");
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
        assert_eq!(f32::from(b_clamped.origin.x), 1920. - expected_w - rem * 0.5);
        assert_eq!(f32::from(b_clamped.origin.y), 1080. - expected_h - rem * 0.5);
    }
}
