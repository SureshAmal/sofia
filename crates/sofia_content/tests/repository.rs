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
