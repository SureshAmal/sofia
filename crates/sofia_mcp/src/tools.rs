use base64::{Engine as _, engine::general_purpose::STANDARD};
use rmcp::model::Tool;
use serde_json::{Value, json};
use sofia_content::{Content, Store};
fn string(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("{key} is required"))
}
fn optional<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}
fn resolve(store: &Store, args: &Value) -> Result<sofia_content::Document, String> {
    store.resolve(optional(args, "id"), optional(args, "title"))
}
pub fn execute(store: &Store, name: &str, args: Value) -> Result<(Value, Option<String>), String> {
    let value = match name {
        "sofia_clipboard_read" => match crate::clipboard::read()? {
            crate::clipboard::ClipboardData::Text(text) => json!({"ok": true, "text": text}),
            crate::clipboard::ClipboardData::Image { bytes, mime_type } => json!({
                "ok": true,
                "image": {"mime_type": mime_type, "data": STANDARD.encode(bytes)}
            }),
        },
        "sofia_list_applications" => {
            let query = optional(&args, "query");
            json!({"ok": true, "applications": crate::apps::list(query)?})
        }
        "sofia_open_application" => {
            let query = string(&args, "query")?;
            let app = crate::apps::launch(&query)?;
            json!({"ok": true, "opened": app})
        }
        "sofia_open_web" => {
            let query = string(&args, "query")?;
            let url = crate::web::open(&query)?;
            json!({"ok": true, "url": url})
        }
        "sofia_screenshot" => {
            let bytes = crate::screenshot::capture()?;
            json!({
                "ok": true,
                "image": {"mime_type": "image/png", "data": STANDARD.encode(bytes)}
            })
        }
        "sofia_window_protocol" => {
            json!({
                "version": 1,
                "kinds": ["note", "todo", "reminder", "chart", "html"],
                "tags": {
                    "note": "markwindow",
                    "todo": "userwindow",
                    "reminder": "userwindow",
                    "chart": "visualizerwindow",
                    "html": "webwindow"
                },
                "chart_types": ["line", "bar", "area", "pie", "radar"],
                "placements": ["pill", "center", "left", "right", "bottom", "top_left", "top_right", "bottom_left", "bottom_right"],
                "html": "GPUI native HTML rendering with inline styles, colors, tables and formatting",
                "size_units": "rem",
                "live_updates": true,
                "edit_policy": "Direct inline edits on click with instant autosave; stale edits rejected",
                "inline_editing_rules": "Markdown is live rendered; todos and reminders edit directly on click without a raw editor (click checkbox to toggle done, click text to edit inline with instant autosave)",
                "reminders": "Stored due_at only; no scheduled alarm service yet",
                "window_tracking": "Documents and windows explicitly track open/closed state. Use sofia_list_windows to see active and closed presentation windows (with open_count and closed_count), or sofia_list_documents to search all content with open/closed status",
                "database": "sofia_mcp.db; separate from LLM history"
            })
        }
        "sofia_create_document" => {
            let content: Content =
                serde_json::from_value(args.get("content").cloned().ok_or("content is required")?)
                    .map_err(|e| e.to_string())?;
            let mut tags: Vec<String> =
                serde_json::from_value(args.get("tags").cloned().unwrap_or(json!([])))
                    .map_err(|e| e.to_string())?;
            if let Some(placement) = optional(&args, "placement") {
                let tag = format!("pos:{}", placement);
                tags.retain(|t| !t.starts_with("pos:"));
                tags.push(tag);
            }
            let doc = store.create(
                string(&args, "title")?,
                tags,
                content,
                args.get("width_rem").and_then(Value::as_f64).unwrap_or(32.) as f32,
                args.get("height_rem")
                    .and_then(Value::as_f64)
                    .unwrap_or(24.) as f32,
            )?;
            if args.get("open").and_then(Value::as_bool).unwrap_or(true) {
                let doc = store.set_open(&doc.id, true)?;
                return Ok((doc.json(), Some(doc.id)));
            }
            doc.json()
        }
        "sofia_list_documents" => serde_json::to_value(store.list(
            optional(&args, "query"),
            optional(&args, "kind"),
            optional(&args, "tag"),
        )?)
        .map_err(|e| e.to_string())?,
        "sofia_get_document" => resolve(store, &args)?.json(),
        "sofia_update_document" => {
            let mut doc = store.get(&string(&args, "id")?)?;
            let revision = args
                .get("expected_revision")
                .and_then(Value::as_i64)
                .ok_or("expected_revision is required")?;
            if let Some(content) = args.get("content") {
                let next: Content =
                    serde_json::from_value(content.clone()).map_err(|e| e.to_string())?;
                if next.kind() != doc.content.kind() {
                    return Err("Document kind cannot change".into());
                }
                doc.content = next;
            }
            if let Some(title) = optional(&args, "new_title") {
                doc.title = title.into();
            }
            if let Some(tags) = args.get("tags") {
                doc.tags = serde_json::from_value(tags.clone()).map_err(|e| e.to_string())?;
            }
            if !doc.tags.iter().any(|tag| tag == doc.content.tag()) {
                doc.tags.push(doc.content.tag().into());
            }
            let doc = store.update(doc, revision)?;
            return Ok((doc.json(), Some(doc.id)));
        }
        "sofia_open_window" | "sofia_close_window" => {
            let mut doc = resolve(store, &args)?;
            let mut changed = false;
            if name == "sofia_open_window" {
                if let Some(width) = args.get("width_rem").and_then(Value::as_f64) {
                    doc.width_rem = width as f32;
                    changed = true;
                }
                if let Some(height) = args.get("height_rem").and_then(Value::as_f64) {
                    doc.height_rem = height as f32;
                    changed = true;
                }
                if let Some(placement) = optional(&args, "placement") {
                    doc.tags.retain(|t| !t.starts_with("pos:"));
                    doc.tags.push(format!("pos:{}", placement));
                    changed = true;
                }
            }
            if changed {
                let rev = doc.revision;
                doc = store.update(doc, rev)?;
            }
            let doc = store.set_open(&doc.id, name == "sofia_open_window")?;
            // Opening or closing a presentation window is a UI side effect.
            // Do not send the whole note/chart/HTML body back to Gemini: that
            // large result can trigger a separate spoken acknowledgement for
            // every window in a multi-window request. The UI receives the
            // complete document through ContentChanged over IPC.
            return Ok((
                json!({
                    "ok": true,
                    "id": doc.id,
                    "title": doc.title,
                    "open": doc.open,
                }),
                Some(doc.id),
            ));
        }
        "sofia_list_windows" => {
            let include_closed = args
                .get("include_closed")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let open: Vec<sofia_content::DocumentSummary> = store
                .open_documents()?
                .into_iter()
                .map(sofia_content::DocumentSummary::from)
                .collect();
            let open_count = open.len();
            let closed_count = store.closed_count()?;
            let closed: Vec<sofia_content::DocumentSummary> = if include_closed {
                store
                    .closed_documents()?
                    .into_iter()
                    .map(sofia_content::DocumentSummary::from)
                    .collect()
            } else {
                Vec::new()
            };
            json!({
                "open": open,
                "closed": closed,
                "open_count": open_count,
                "closed_count": closed_count,
            })
        }
        "sofia_delete_documents" => {
            let ids: Vec<String> =
                if let Some(ids_array) = args.get("ids").and_then(Value::as_array) {
                    ids_array
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                } else if let Some(id_str) = args.get("id").and_then(Value::as_str) {
                    vec![id_str.to_string()]
                } else {
                    return Err(
                    "Specify 'ids' (array of document IDs) or 'id' (single document ID) to delete"
                        .into(),
                );
                };
            if ids.is_empty() {
                return Err("No document IDs provided for deletion".into());
            }
            let deleted = store.delete_many(&ids)?;
            let deleted_count = deleted.len();
            json!({
                "deleted": deleted,
                "deleted_count": deleted_count,
                "requested_count": ids.len(),
            })
        }
        _ => return Err("Unknown Sofia tool".into()),
    };
    Ok((value, None))
}
fn object(properties: Value, required: Vec<&str>) -> Value {
    json!({"type":"object","properties":properties,"required":required})
}
fn content_schema() -> Value {
    let item = object(
        json!({"id":{"type":"string"},"text":{"type":"string"},"done":{"type":"boolean"},"due_at":{"type":"string","description":"Optional ISO8601 due time"}}),
        vec!["id", "text"],
    );
    let mut options = Vec::new();
    for kind in ["note", "todo", "reminder", "chart", "html"] {
        let mut properties = json!({"kind":{"type":"string","enum":[kind]}});
        let fields = match kind {
            "note" => {
                properties["markdown"] = json!({"type":"string"});
                vec!["kind", "markdown"]
            }
            "todo" | "reminder" => {
                properties["items"] = json!({"type":"array","items":item});
                vec!["kind", "items"]
            }
            "chart" => {
                properties["chart_type"] =
                    json!({"type":"string","enum":["line","bar","area","pie","radar"]});
                properties["points"] = json!({"type":"array","items":object(json!({"label":{"type":"string"},"value":{"type":"number"}}),vec!["label","value"])});
                vec!["kind", "chart_type", "points"]
            }
            _ => {
                properties["html"] = json!({"type":"string","description":"HTML formatted document rendered with CSS styling and tables"});
                vec!["kind", "html"]
            }
        };
        options.push(object(properties, fields));
    }
    json!({"anyOf":options})
}
pub fn declarations() -> Vec<Tool> {
    let selector = json!({"id":{"type":"string"},"title":{"type":"string","description":"Exact unique title; prefer ID"}});
    let placements = json!([
        "pill",
        "center",
        "left",
        "right",
        "bottom",
        "top_left",
        "top_right",
        "bottom_left",
        "bottom_right"
    ]);
    let mut create = json!({
        "title":{"type":"string"},
        "tags":{"type":"array","items":{"type":"string"}},
        "open":{"type":"boolean"},
        "placement":{"type":"string","enum":placements.clone(),"description":"Initial window placement (default: pill)"},
        "width_rem":{"type":"number"},
        "height_rem":{"type":"number"}
    });
    create["content"] = content_schema();
    let mut update = json!({"id":{"type":"string"},"expected_revision":{"type":"integer"},"new_title":{"type":"string"},"tags":{"type":"array","items":{"type":"string"}}});
    update["content"] = content_schema();
    let mut open = selector.clone();
    open["placement"] = json!({"type":"string","enum":placements,"description":"Window placement (pill, center, left, right, bottom, top_left, top_right, bottom_left, bottom_right)"});
    open["width_rem"] = json!({"type":"number"});
    open["height_rem"] = json!({"type":"number"});
    let list_windows = json!({
        "include_closed": {"type": "boolean", "description": "When true, includes closed windows with open=false in addition to active windows"}
    });
    let delete_documents = json!({
        "ids": {
            "type": "array",
            "items": {"type": "string"},
            "description": "List of document IDs to permanently delete from local storage"
        },
        "id": {
            "type": "string",
            "description": "Single document ID to delete (optional if 'ids' is provided)"
        }
    });
    [
        ("sofia_clipboard_read","Read the current Linux clipboard item and provide text or image content directly to the model",object(json!({}),vec![])),
        ("sofia_list_applications","List installed Linux desktop applications, optionally filtered with fuzzy matching",object(json!({"query":{"type":"string","description":"Optional application name or desktop ID search"}}),vec![])),
        ("sofia_open_application","Find and launch an installed Linux desktop application by fuzzy name or desktop ID",object(json!({"query":{"type":"string"}}),vec!["query"])),
        ("sofia_open_web","Open a URL or DuckDuckGo query in the system browser. Bangs such as !yt rust are passed directly to DuckDuckGo.",object(json!({"query":{"type":"string"}}),vec!["query"])),
        ("sofia_screenshot","Capture the current Wayland screen with grim and provide the PNG image directly to the model",object(json!({}),vec![])),
        ("sofia_window_protocol","Get supported content kinds, tags, chart types (line, bar, area, pie, radar), window placements, inline editing rules, and open/closed window tracking",object(json!({}),vec![])),
        ("sofia_create_document","Persist and optionally open notes, todos, reminders, charts (line, bar, area, pie, radar), or HTML. Opens by default.",object(create,vec!["title","content"])),
        ("sofia_list_documents","Search saved content by indexed text, kind or tag. Returns document summaries with open/closed status.",object(json!({"query":{"type":"string"},"kind":{"type":"string"},"tag":{"type":"string"}}),vec![])),
        ("sofia_get_document","Read saved content and revision by ID or exact unique title",object(selector.clone(),vec![])),
        ("sofia_update_document","Edit saved content and update open window live. Supply latest expected_revision to avoid overwriting user edits.",object(update,vec!["id","expected_revision"])),
        ("sofia_delete_documents","Permanently delete one or more notes/documents by their IDs from storage.",object(delete_documents,vec![])),
        ("sofia_open_window","Open saved content by ID or unique title at a specific position (pill, center, left, right, bottom, top_left, top_right, bottom_left, bottom_right)",object(open,vec![])),
        ("sofia_close_window","Close a presentation window into the pill without deleting its saved content",object(selector,vec![])),
        ("sofia_list_windows","List open (and optionally closed) windows with IDs, titles, open status, kinds, and tags",object(list_windows,vec![])),
    ].into_iter().map(|(name,description,schema)|Tool::new(name,description,schema.as_object().unwrap().clone())).collect()
}
