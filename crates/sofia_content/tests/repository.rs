use sofia_content::{Content, Store};
struct Sandbox(std::path::PathBuf);
impl Sandbox {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("sofia-content-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn persists_indexes_updates_and_rejects_stale_writes() {
    let sandbox = Sandbox::new();
    let path = sandbox.0.join("content.db");
    let store = Store::open(&path).unwrap();
    let doc = store
        .create(
            "My note".into(),
            vec!["project".into()],
            Content::Note {
                markdown: "Remember the telescope".into(),
            },
            32.,
            24.,
        )
        .unwrap();
    assert_eq!(
        store
            .list(Some("telescope"), Some("note"), Some("markwindow"))
            .unwrap()
            .len(),
        1
    );
    let mut changed = store.set_open(&doc.id, true).unwrap();
    changed.content = Content::Note {
        markdown: "New astronomy text".into(),
    };
    let saved = store.update(changed.clone(), doc.revision).unwrap();
    assert!(saved.open);
    assert_eq!(saved.revision, 2);
    assert!(store.update(changed, doc.revision).is_err());
    assert!(
        store
            .list(Some("telescope"), None, None)
            .unwrap()
            .is_empty()
    );
    assert_eq!(store.list(Some("astronomy"), None, None).unwrap().len(), 1);
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.get(&doc.id).unwrap(), saved);
    store.set_open(&doc.id, false).unwrap();
    assert!(store.open_documents().unwrap().is_empty());
    assert!(store.get(&doc.id).is_ok());
}
#[test]
fn duplicate_titles_need_ids_and_two_connections_see_edits() {
    let sandbox = Sandbox::new();
    let path = sandbox.0.join("content.db");
    let first = Store::open(&path).unwrap();
    let second = Store::open(&path).unwrap();
    for _ in 0..2 {
        first
            .create(
                "Duplicate".into(),
                vec![],
                Content::Note {
                    markdown: "Text".into(),
                },
                32.,
                24.,
            )
            .unwrap();
    }
    assert!(second.resolve(None, Some("Duplicate")).is_err());
    let doc = second.list(None, None, None).unwrap().remove(0);
    first.set_open(&doc.id, true).unwrap();
    assert_eq!(second.open_documents().unwrap().len(), 1);
}

#[test]
fn chart_type_serde_lowercase_and_document_summary_open() {
    use sofia_content::{ChartPoint, ChartType, Document, DocumentSummary};

    for (variant, expected) in [
        (ChartType::Line, "\"line\""),
        (ChartType::Bar, "\"bar\""),
        (ChartType::Area, "\"area\""),
        (ChartType::Pie, "\"pie\""),
        (ChartType::Radar, "\"radar\""),
    ] {
        let serialized = serde_json::to_string(&variant).unwrap();
        assert_eq!(serialized, expected);
        let deserialized: ChartType = serde_json::from_str(expected).unwrap();
        assert_eq!(deserialized, variant);
    }

    let doc = Document {
        id: "doc-1".into(),
        title: "Test Doc".into(),
        tags: vec!["tag1".into()],
        content: Content::Chart {
            chart_type: ChartType::Area,
            points: vec![ChartPoint {
                label: "A".into(),
                value: 10.0,
            }],
        },
        revision: 3,
        updated_at: 1000,
        open: true,
        width_rem: 32.0,
        height_rem: 24.0,
    };

    let summary_from_ref = DocumentSummary::from(&doc);
    assert_eq!(summary_from_ref.id, "doc-1");
    assert_eq!(summary_from_ref.title, "Test Doc");
    assert_eq!(summary_from_ref.kind, "chart");
    assert!(summary_from_ref.open);

    let mut doc_closed = doc.clone();
    doc_closed.open = false;
    let summary_from_val = DocumentSummary::from(doc_closed);
    assert!(!summary_from_val.open);
}

#[test]
fn open_and_closed_documents_tracking() {
    let sandbox = Sandbox::new();
    let path = sandbox.0.join("content.db");
    let store = Store::open(&path).unwrap();

    let doc1 = store
        .create(
            "Doc 1".into(),
            vec![],
            Content::Note {
                markdown: "One".into(),
            },
            32.,
            24.,
        )
        .unwrap();

    let doc2 = store
        .create(
            "Doc 2".into(),
            vec![],
            Content::Note {
                markdown: "Two".into(),
            },
            32.,
            24.,
        )
        .unwrap();

    assert_eq!(store.closed_count().unwrap(), 2);
    assert_eq!(store.closed_documents().unwrap().len(), 2);
    assert_eq!(store.open_documents().unwrap().len(), 0);

    store.set_open(&doc1.id, true).unwrap();
    assert_eq!(store.closed_count().unwrap(), 1);
    assert_eq!(store.closed_documents().unwrap().len(), 1);
    assert_eq!(store.open_documents().unwrap().len(), 1);
    assert_eq!(store.open_documents().unwrap()[0].id, doc1.id);
    assert_eq!(store.closed_documents().unwrap()[0].id, doc2.id);

    // Document summaries from list() have explicit open flag
    let summaries = store.list(None, None, None).unwrap();
    let s1 = summaries.iter().find(|s| s.id == doc1.id).unwrap();
    let s2 = summaries.iter().find(|s| s.id == doc2.id).unwrap();
    assert!(s1.open);
    assert!(!s2.open);
}
