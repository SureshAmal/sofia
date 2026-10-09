//! Window positioning, layout cascade, and placement calculation.
use gpui_kit::*;
use sofia_content::Document;

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

pub fn panel_bounds(
    doc: &Document,
    drag_offset: (f32, f32),
    index: usize,
    pill: (f32, f32),
    pill_size: (f32, f32),
    viewport: (f32, f32),
    rem: f32,
) -> Bounds<Pixels> {
    let width = (doc.width_rem * rem).min((viewport.0 - rem * 2.).max(rem));
    let height = (doc.height_rem * rem).min((viewport.1 - rem * 2.).max(rem));

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
