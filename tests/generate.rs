use inkworm::ui::generate::PastingState;
use inkworm::ui::task_msg::{GenerateProgress, TaskMsg};

#[test]
fn pasting_state_transitions() {
    let mut state = PastingState::new();
    assert_eq!(state.byte_count(), 0);
    assert_eq!(state.word_count(), 0);
    assert!(!state.can_submit(100));

    state.text = "test article".to_string();
    assert_eq!(state.byte_count(), 12);
    assert_eq!(state.word_count(), 2);
    assert!(state.can_submit(100));

    state.text = "a".repeat(101);
    assert!(!state.can_submit(100));
}

#[test]
fn generate_progress_enum_variants() {
    // Smoke test that all variants compile
    let _p1 = GenerateProgress::Phase1Started;
    let _p2 = GenerateProgress::Phase1Done { sentence_count: 5 };
    let _p3 = GenerateProgress::Phase2Progress { done: 3, total: 5 };
}

#[tokio::test]
async fn task_msg_channel_flow() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);

    tokio::spawn(async move {
        tx.send(TaskMsg::Generate(GenerateProgress::Phase1Started))
            .await
            .unwrap();
        tx.send(TaskMsg::Generate(GenerateProgress::Phase1Done {
            sentence_count: 3,
        }))
        .await
        .unwrap();
    });

    let msg1 = rx.recv().await.unwrap();
    assert!(matches!(
        msg1,
        TaskMsg::Generate(GenerateProgress::Phase1Started)
    ));

    let msg2 = rx.recv().await.unwrap();
    assert!(matches!(
        msg2,
        TaskMsg::Generate(GenerateProgress::Phase1Done { sentence_count: 3 })
    ));
}
