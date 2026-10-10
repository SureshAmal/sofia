use serde::{Deserialize, Serialize};

/// Semantic color / styling intent mapped directly to theme tokens.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ColorIntent {
    #[default]
    Default,
    Primary,
    Secondary,
    Accent,
    Success,
    Warning,
    Danger,
    Muted,
}

/// Visual button / badge variant aligned with gpui-component styling.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ComponentVariant {
    #[default]
    Default,
    Primary,
    Outline,
    Ghost,
    Secondary,
    Destructive,
}

/// Trend indicator for metric widgets.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TrendDirection {
    #[default]
    Neutral,
    Up,
    Down,
}

/// 1-to-1 Declarative Generative UI Node.
/// Can be nested and recursively mapped to gpui-kit native components.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiNode {
    /// Vertical container (div().flex().flex_col())
    Column {
        #[serde(default)]
        gap: Option<f32>,
        #[serde(default)]
        children: Vec<UiNode>,
    },
    /// Horizontal container (div().flex().flex_row())
    Row {
        #[serde(default)]
        gap: Option<f32>,
        #[serde(default)]
        align: Option<String>, // "start", "center", "end", "between"
        #[serde(default)]
        children: Vec<UiNode>,
    },
    /// Card container with optional title, description, and bordered frame
    Card {
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        children: Vec<UiNode>,
    },
    /// Text element with theme-aware typography and sizing
    Text {
        content: String,
        #[serde(default)]
        size: Option<String>, // "xs", "sm", "base", "lg", "xl"
        #[serde(default)]
        weight: Option<String>, // "normal", "medium", "semibold", "bold"
        #[serde(default)]
        intent: ColorIntent,
        #[serde(default)]
        code: bool,
    },
    /// Badge pill with status or tag
    Badge {
        label: String,
        #[serde(default)]
        variant: ComponentVariant,
        #[serde(default)]
        intent: ColorIntent,
    },
    /// Interactive button emitting an action ID when clicked
    Button {
        id: String,
        label: String,
        #[serde(default)]
        variant: ComponentVariant,
        #[serde(default)]
        disabled: bool,
    },
    /// Interactive toggle switch
    Switch {
        id: String,
        label: String,
        #[serde(default)]
        checked: bool,
        #[serde(default)]
        disabled: bool,
    },
    /// Interactive checkbox item
    Checkbox {
        id: String,
        label: String,
        #[serde(default)]
        checked: bool,
        #[serde(default)]
        disabled: bool,
    },
    /// Progress bar indicator (0.0 to 1.0)
    Progress {
        value: f32,
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        intent: ColorIntent,
    },
    /// KPI / Stat metric display: large number + label + trend delta
    Metric {
        label: String,
        value: String,
        #[serde(default)]
        delta: Option<String>,
        #[serde(default)]
        trend: TrendDirection,
    },
    /// Key-Value grid representation for system stats, info rows, or properties
    KeyValue {
        items: Vec<(String, String)>,
    },
    /// Clean structured data table
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    /// Thin theme divider separator
    Divider {
        #[serde(default)]
        vertical: bool,
    },
    /// Collapsible accordion section
    Accordion {
        title: String,
        #[serde(default)]
        default_open: bool,
        children: Vec<UiNode>,
    },
}

impl UiNode {
    /// Validates max tree depth and element counts to prevent runaway payloads.
    pub fn validate(&self, depth: usize) -> Result<(), String> {
        if depth > 12 {
            return Err("Generative UI exceeds max nesting depth of 12".into());
        }
        match self {
            Self::Column { children, .. } | Self::Row { children, .. } | Self::Card { children, .. } | Self::Accordion { children, .. } => {
                if children.len() > 100 {
                    return Err("Container cannot exceed 100 children".into());
                }
                for child in children {
                    child.validate(depth + 1)?;
                }
            }
            Self::Table { headers, rows } => {
                if rows.len() > 100 {
                    return Err("Table cannot exceed 100 rows".into());
                }
                let expected_cols = headers.len();
                for row in rows {
                    if row.len() != expected_cols {
                        return Err("Table row column count must match headers count".into());
                    }
                }
            }
            Self::KeyValue { items } => {
                if items.len() > 100 {
                    return Err("KeyValue cannot exceed 100 items".into());
                }
            }
            _ => {}
        }
        Ok(())
    }
}
