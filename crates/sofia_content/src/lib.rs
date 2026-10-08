//! Shared user-content repository, separate from model history.
mod store;
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub use store::Store;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Content {
    Note {
        markdown: String,
    },
    Todo {
        items: Vec<TodoItem>,
    },
    Reminder {
        items: Vec<TodoItem>,
    },
    Chart {
        chart_type: ChartType,
        points: Vec<ChartPoint>,
    },
    Html {
        html: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TodoItem {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub due_at: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ChartPoint {
    pub label: String,
    pub value: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ChartType {
    Line,
    Bar,
    Area,
    Pie,
    Radar,
}
impl Content {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Note { .. } => "note",
            Self::Todo { .. } => "todo",
            Self::Reminder { .. } => "reminder",
            Self::Chart { .. } => "chart",
            Self::Html { .. } => "html",
        }
    }
    pub fn tag(&self) -> &'static str {
        match self {
            Self::Note { .. } => "markwindow",
            Self::Todo { .. } | Self::Reminder { .. } => "userwindow",
            Self::Chart { .. } => "visualizerwindow",
            Self::Html { .. } => "webwindow",
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > 512 * 1024 {
            return Err("Content exceeds 512 KiB".into());
        }
        match self {
            Self::Todo { items } | Self::Reminder { items } => {
                if items.len() > 500 {
                    return Err("At most 500 items".into());
                }
                let mut ids = std::collections::HashSet::new();
                for item in items {
                    if item.id.trim().is_empty()
                        || item.text.trim().is_empty()
                        || !ids.insert(&item.id)
                    {
                        return Err("Todo/reminder items need unique IDs and nonempty text".into());
                    }
                }
            }
            Self::Chart { points, .. }
                if points.is_empty()
                    || points.len() > 1000
                    || points
                        .iter()
                        .any(|point| !point.value.is_finite() || point.label.len() > 128) =>
            {
                return Err("Chart requires 1–1000 finite values with short labels".into());
            }
            _ => {}
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Document {
    pub id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub content: Content,
    pub revision: i64,
    pub updated_at: i64,
    pub open: bool,
    pub width_rem: f32,
    pub height_rem: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentSummary {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub tags: Vec<String>,
    pub revision: i64,
    pub updated_at: i64,
    pub open: bool,
}
impl From<Document> for DocumentSummary {
    fn from(doc: Document) -> Self {
        Self {
            id: doc.id,
            title: doc.title,
            kind: doc.content.kind().into(),
            tags: doc.tags,
            revision: doc.revision,
            updated_at: doc.updated_at,
            open: doc.open,
        }
    }
}
impl From<&Document> for DocumentSummary {
    fn from(doc: &Document) -> Self {
        Self {
            id: doc.id.clone(),
            title: doc.title.clone(),
            kind: doc.content.kind().into(),
            tags: doc.tags.clone(),
            revision: doc.revision,
            updated_at: doc.updated_at,
            open: doc.open,
        }
    }
}
impl Document {
    pub fn validate(&self) -> Result<(), String> {
        if self.title.trim().is_empty()
            || self.title.len() > 256
            || self.tags.len() > 32
            || self.tags.iter().any(|tag| tag.len() > 64)
        {
            return Err("Use a title of 1–256 bytes and up to 32 short tags".into());
        }
        if !self.width_rem.is_finite()
            || !self.height_rem.is_finite()
            || !(18.0..=64.0).contains(&self.width_rem)
            || !(12.0..=48.0).contains(&self.height_rem)
        {
            return Err("Window size must be 18–64 rem wide and 12–48 rem high".into());
        }
        self.content.validate()
    }
    pub fn json(&self) -> Value {
        serde_json::to_value(self).expect("validated document")
    }
}
