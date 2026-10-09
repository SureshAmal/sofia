use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Emotion and sentiment detected during user interaction or assistant turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EmotionTone {
    Neutral,
    Satisfied,
    Frustrated,
    Curious,
    Urgent,
    Excited,
    Confused,
    Disappointed,
    Custom(String),
}

impl Default for EmotionTone {
    fn default() -> Self {
        Self::Neutral
    }
}

/// Rich emotional context captured during an interaction.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EmotionContext {
    pub tone: EmotionTone,
    /// Valence score from -1.0 (very negative) to +1.0 (very positive)
    pub valence: f32,
    /// Arousal / urgency score from 0.0 (calm) to 1.0 (very intense/urgent)
    pub arousal: f32,
    /// Trigger context (e.g. "tool error on shell command", "app layout resized")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
}

/// Entity in the World Facts knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    pub name: String,
    pub entity_type: String, // e.g. "person", "project", "tool", "document", "topic", "preference"
    pub summary: String,
    pub attributes: Value,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Directed relation between two entities in the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    pub id: String,
    pub source_id: String,
    pub relation: String, // e.g. "prefers", "worked_on", "created_document", "complained_about", "uses_tool"
    pub target_id: String,
    pub weight: f32,
    pub context: Option<String>,
    pub created_at: i64,
}

/// Interaction with tools or documents during an experience.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDocRef {
    pub kind: String, // "tool", "document", "chart", "note", "app"
    pub identifier: String, // tool name or document id/title
    pub action: String, // "execute", "create", "modify", "view", "close"
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome_summary: Option<String>,
}

/// An episodic interaction experience (temporal series).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Experience {
    pub id: String,
    pub session_id: Option<String>,
    pub timestamp: i64,
    pub user_query: String,
    pub summary: String,
    pub emotion: EmotionContext,
    pub tools_and_docs: Vec<ToolDocRef>,
    pub entities_involved: Vec<String>,
    pub importance: f32, // 0.0 to 1.0
}

/// Dynamic user preference that adapts over time based on feedback & emotions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPreference {
    pub key: String, // e.g. "ui.style", "code.comments", "response.verbosity", "os.platform"
    pub value: String, // e.g. "minimalist direct without nested backgrounds"
    pub category: String, // "coding", "ui", "tone", "system", "workflow"
    pub confidence: f32, // increases as reinforced by user affirmations/satisfaction
    pub source_context: String, // why this preference was established
    pub updated_at: i64,
}

/// Hybrid recall result combining World Facts, Experiences, and Preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallResult {
    pub query: String,
    pub preferences: Vec<UserPreference>,
    pub entities: Vec<Entity>,
    pub related_experiences: Vec<Experience>,
}
