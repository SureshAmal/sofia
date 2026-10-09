use sofia_protocol::{ServerEvent, TurnState};
use sofia_trace_store::{Recorder, Store};
use uuid::Uuid;

#[test]
fn records_two_runs_with_tool_order_duration_and_errors() {
    let path = std::env::temp_dir().join(format!("sofia-trace-{}.db", Uuid::new_v4()));
    let store = Store::open(&path).unwrap();
    let mut recorder = Recorder::new(store);
    let session = Some(Uuid::new_v4());
    recorder
        .record(
            &ServerEvent::InputText {
                text: "Make a note".into(),
            },
            session,
        )
        .unwrap();
    recorder
        .record(
            &ServerEvent::ToolCallRequested {
                call_id: "one".into(),
                name: "create_note".into(),
            },
            session,
        )
        .unwrap();
    recorder
        .record(
            &ServerEvent::ToolCallFinished {
                call_id: "one".into(),
                name: "create_note".into(),
                success: false,
            },
            session,
        )
        .unwrap();
    recorder
        .record(
            &ServerEvent::Error {
                message: "create_note: timed out".into(),
            },
            session,
        )
        .unwrap();
    recorder
        .record(
            &ServerEvent::AssistantTextFinal {
                turn_id: Uuid::new_v4(),
                text: "That failed".into(),
                interrupted: false,
            },
            session,
        )
        .unwrap();
    recorder
        .record(
            &ServerEvent::TurnStateChanged {
                state: TurnState::Listening,
            },
            session,
        )
        .unwrap();
    recorder
        .record(
            &ServerEvent::InputText {
                text: "Try again".into(),
            },
            session,
        )
        .unwrap();
    recorder
        .record(
            &ServerEvent::ToolCallRequested {
                call_id: "two".into(),
                name: "create_note".into(),
            },
            session,
        )
        .unwrap();
    recorder
        .record(
            &ServerEvent::ToolCallFinished {
                call_id: "two".into(),
                name: "create_note".into(),
                success: true,
            },
            session,
        )
        .unwrap();
    recorder
        .record(
            &ServerEvent::TurnStateChanged {
                state: TurnState::Ready,
            },
            session,
        )
        .unwrap();
    drop(recorder);
    let store = Store::open(&path).unwrap();
    let runs = store.runs().unwrap();
    assert_eq!(runs.len(), 2);
    let first = runs.iter().find(|run| run.input == "Make a note").unwrap();
    assert_eq!(first.status, "error");
    assert_eq!(first.tool_count, 1);
    assert_eq!(
        store
            .steps(&first.id)
            .unwrap()
            .iter()
            .map(|step| step.kind.as_str())
            .collect::<Vec<_>>(),
        ["input", "tool_error", "error", "output"]
    );
    let second = runs.iter().find(|run| run.input == "Try again").unwrap();
    assert_eq!(second.status, "ok");
    assert_eq!(store.steps(&second.id).unwrap()[1].kind, "tool_ok");
    assert!(store.steps(&second.id).unwrap()[1].duration_ms.is_some());
    std::fs::remove_file(&path).unwrap();
}
