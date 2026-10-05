use std::time::Duration;

use serde_json::json;

use super::*;

// needs TREX_DATABASE_URL and TREX_REDIS_URL: cargo test -- --ignored
#[tokio::test]
#[ignore]
async fn session_lifecycle() {
    dotenvy::dotenv().ok();
    let store = Store::connect(
        &std::env::var("TREX_DATABASE_URL").unwrap(),
        &std::env::var("TREX_REDIS_URL").unwrap(),
    )
    .await
    .unwrap();
    let workspace = |name: &'static str| {
        let store = &store;
        async move {
            let email = format!("{name}-{}@test.trex", Uuid::now_v7());
            let (_, workspace) = store
                .create_user(&email, name, "hash")
                .await
                .unwrap()
                .unwrap();
            workspace.id
        }
    };
    let (alice, mallory) = (workspace("alice").await, workspace("mallory").await);

    let project = store.create_project(alice, "Backend", None).await.unwrap();
    assert!(
        store
            .create_session(mallory, "gpt-6.1-sol", None, false, Some(project.id))
            .await
            .is_err(),
        "no sessions in another workspace's project"
    );
    let in_project = store
        .create_session(alice, "gpt-6.1-sol", None, false, Some(project.id))
        .await
        .unwrap();

    let session = store
        .create_session(alice, "gpt-6.1-sol", Some("low"), true, None)
        .await
        .unwrap();
    assert_eq!(session.status, SessionStatus::Idle);
    assert!(session.fast);
    assert!(store.session(mallory, session.id).await.unwrap().is_none());
    assert!(
        store
            .delete_session(mallory, session.id)
            .await
            .unwrap()
            .is_none()
    );
    let ids = |sessions: Vec<Session>| sessions.into_iter().map(|s| s.id).collect::<Vec<_>>();
    assert_eq!(
        ids(store
            .sessions(alice, 10, None, SessionFilter::All)
            .await
            .unwrap()),
        [session.id, in_project.id]
    );
    assert_eq!(
        ids(store
            .sessions(alice, 10, None, SessionFilter::NoProject)
            .await
            .unwrap()),
        [session.id]
    );
    assert_eq!(
        ids(store
            .sessions(alice, 10, None, SessionFilter::Project(project.id))
            .await
            .unwrap()),
        [in_project.id]
    );

    assert!(
        store
            .set_title_if_missing(alice, session.id, "Generated")
            .await
            .unwrap()
    );
    assert!(
        !store
            .set_title_if_missing(alice, session.id, "Again")
            .await
            .unwrap()
    );
    let renamed = store
        .update_session(alice, session.id, Some("Renamed"), Some(Some(project.id)))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (renamed.title.as_deref(), renamed.project_id),
        (Some("Renamed"), Some(project.id))
    );
    let other = store.create_project(mallory, "Theirs", None).await.unwrap();
    assert!(
        store
            .update_session(alice, session.id, None, Some(Some(other.id)))
            .await
            .unwrap()
            .is_none(),
        "can't move a chat into another workspace's project"
    );
    assert!(
        store
            .update_session(mallory, session.id, Some("x"), None)
            .await
            .unwrap()
            .is_none()
    );
    store.delete_project(alice, project.id).await.unwrap();
    let kept = store.session(alice, in_project.id).await.unwrap().unwrap();
    assert_eq!(kept.project_id, None, "deleting a project keeps its chats");
    let moved_back = store.session(alice, session.id).await.unwrap().unwrap();
    assert_eq!(
        (moved_back.title.as_deref(), moved_back.project_id),
        (Some("Renamed"), None)
    );
    assert!(
        store
            .sessions(alice, 10, Some(in_project.id), SessionFilter::All)
            .await
            .unwrap()
            .is_empty()
    );

    let (run, other) = (Uuid::now_v7(), Uuid::now_v7());
    assert!(store.start_run(alice, session.id, run).await.unwrap());
    assert!(
        !store.start_run(alice, session.id, other).await.unwrap(),
        "one run at a time"
    );
    assert!(!store.start_run(mallory, session.id, other).await.unwrap());
    assert_eq!(
        store.heartbeat_run(session.id, run).await.unwrap(),
        Lease::Held
    );
    assert_eq!(
        store.heartbeat_run(session.id, other).await.unwrap(),
        Lease::Lost
    );

    let item = |n: i32| json!({ "n": n });
    store
        .put_session_items(alice, session.id, 0, &[item(1), item(2)])
        .await
        .unwrap();
    store
        .put_session_items(alice, session.id, 1, &[item(2), item(3)])
        .await
        .unwrap();
    store
        .put_session_items(mallory, session.id, 3, &[item(666)])
        .await
        .unwrap();
    store
        .append_session_items(alice, session.id, &[item(4)])
        .await
        .unwrap();
    let items = store.session_items(alice, session.id).await.unwrap();
    assert_eq!(
        items,
        [item(1), item(2), item(3), item(4)],
        "saving again is a no-op"
    );
    assert!(
        store
            .session_items(mallory, session.id)
            .await
            .unwrap()
            .is_empty()
    );

    assert!(
        store
            .queue_message(alice, session.id, &json!("first"))
            .await
            .unwrap()
    );
    assert!(
        store
            .queue_message(alice, session.id, &json!({"n": 2}))
            .await
            .unwrap()
    );
    assert!(
        !store
            .queue_message(mallory, session.id, &json!("evil"))
            .await
            .unwrap()
    );
    assert_eq!(
        store
            .finish_run(alice, session.id, run, SessionStatus::Idle, None, None)
            .await
            .unwrap(),
        Finish::MessagesQueued
    );
    assert!(
        store
            .take_queued_messages(mallory, session.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store.take_queued_messages(alice, session.id).await.unwrap(),
        [json!("first"), json!({"n": 2})]
    );
    assert!(
        store
            .take_queued_messages(alice, session.id)
            .await
            .unwrap()
            .is_empty()
    );

    assert!(
        !store
            .request_cancel(Some(mallory), session.id)
            .await
            .unwrap(),
        "only its workspace"
    );
    assert!(store.request_cancel(Some(alice), session.id).await.unwrap());
    assert_eq!(
        store.heartbeat_run(session.id, run).await.unwrap(),
        Lease::CancelRequested
    );
    assert!(
        !store
            .queue_message(alice, session.id, &json!("too late"))
            .await
            .unwrap(),
        "a run asked to stop takes no more messages"
    );
    assert!(!store.run_taken_over(session.id, run).await.unwrap());

    sqlx::query("UPDATE sessions SET run_heartbeat_at = NOW() - INTERVAL '1 hour' WHERE id = $1")
        .bind(session.id)
        .execute(&store.pg)
        .await
        .unwrap();
    let stale = store
        .claim_stale_runs(Duration::from_secs(30), 1000)
        .await
        .unwrap();
    let resumed = stale
        .iter()
        .find(|stale| stale.session == session.id)
        .expect("the stale run is claimed");
    assert_eq!(resumed.workspace, alice);
    assert_ne!(resumed.run, run);
    assert_eq!(
        store.heartbeat_run(session.id, run).await.unwrap(),
        Lease::Lost,
        "the old run lost its lease"
    );
    assert!(store.run_taken_over(session.id, run).await.unwrap());
    assert_eq!(
        store.heartbeat_run(session.id, resumed.run).await.unwrap(),
        Lease::CancelRequested,
        "the cancel carries over to the run that took over"
    );
    assert_eq!(
        store
            .finish_run(alice, session.id, run, SessionStatus::Idle, None, None)
            .await
            .unwrap(),
        Finish::NotOwner
    );
    assert!(
        store
            .claim_stale_runs(Duration::from_secs(30), 1000)
            .await
            .unwrap()
            .iter()
            .all(|stale| stale.session != session.id),
        "a fresh lease isn't stale"
    );

    let question = json!({"call_id": "call_1"});
    assert_eq!(
        store
            .finish_run(
                alice,
                session.id,
                resumed.run,
                SessionStatus::NeedsInput,
                Some(&question),
                None,
            )
            .await
            .unwrap(),
        Finish::Finished
    );
    assert!(
        !store
            .queue_message(alice, session.id, &json!("late"))
            .await
            .unwrap(),
        "only running sessions queue"
    );
    let reloaded = store.session(alice, session.id).await.unwrap().unwrap();
    assert_eq!(reloaded.status, SessionStatus::NeedsInput);
    assert_eq!(reloaded.pending_question, Some(question));

    store
        .charge_usage(
            &UsageRecord {
                workspace_id: alice,
                session_id: session.id,
                model: "gpt-6.1-sol",
                input_tokens: 10,
                cached_input_tokens: 2,
                cache_write_tokens: 0,
                output_tokens: 5,
                reasoning_tokens: 1,
                duration_ms: 1500,
                first_token_ms: Some(300),
            },
            7,
        )
        .await
        .unwrap();
    let usage = store.session_usage(alice, session.id).await.unwrap();
    assert_eq!(
        usage
            .iter()
            .map(|entry| (entry.input_tokens, entry.credits, entry.duration_ms))
            .collect::<Vec<_>>(),
        [(10, 7, 1500)]
    );
    assert!(
        store
            .session_usage(mallory, session.id)
            .await
            .unwrap()
            .is_empty()
    );
    let timed = store.session_items_timed(alice, session.id).await.unwrap();
    assert!(!timed.is_empty() && timed.iter().all(|(_, _, at)| *at > 1_700_000_000_000));
    assert_eq!(
        store.count_session_items(alice, session.id).await.unwrap(),
        timed.len() as i64
    );

    let first = store
        .publish_event(session.id, &json!({"type": "a"}))
        .await
        .unwrap();
    store
        .publish_event(session.id, &json!({"type": "b"}))
        .await
        .unwrap();
    let mut connection = store.event_connection().await.unwrap();
    let all = store
        .read_events(&mut connection, session.id, "0", Duration::from_millis(100))
        .await
        .unwrap();
    let resumed = store
        .read_events(
            &mut connection,
            session.id,
            &first,
            Duration::from_millis(100),
        )
        .await
        .unwrap();
    let none = store
        .read_events(&mut connection, session.id, "$", Duration::from_secs(2))
        .await
        .unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(resumed.len(), 1);
    assert_eq!(resumed[0].event, json!({"type": "b"}));
    assert!(none.is_empty());
    assert_eq!(
        store.last_event_id(session.id).await.unwrap().as_deref(),
        Some(resumed[0].id.as_str())
    );

    sqlx::query(
        "UPDATE sessions SET status = 'needs_input', pending_question = '{}'::JSONB WHERE id = $1",
    )
    .bind(session.id)
    .execute(&store.pg)
    .await
    .unwrap();
    assert!(
        !store
            .dismiss_question(Some(mallory), session.id)
            .await
            .unwrap(),
        "only its workspace"
    );
    assert!(
        store
            .dismiss_question(Some(alice), session.id)
            .await
            .unwrap()
    );
    let dismissed = store.session(alice, session.id).await.unwrap().unwrap();
    assert_eq!(dismissed.status, SessionStatus::Idle);
    assert!(
        !store.dismiss_question(None, session.id).await.unwrap(),
        "nothing to dismiss"
    );

    store
        .delete_session(alice, session.id)
        .await
        .unwrap()
        .unwrap();
    store.delete_events(session.id).await.unwrap();
    let usage_left: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM usage_records WHERE session_id = $1")
            .bind(session.id)
            .fetch_one(&store.pg)
            .await
            .unwrap();
    assert_eq!(usage_left, 0);
    assert!(store.last_event_id(session.id).await.unwrap().is_none());
    sqlx::query("DELETE FROM users WHERE id IN (SELECT user_id FROM workspace_members WHERE workspace_id = ANY($1))")
        .bind([alice, mallory])
        .execute(&store.pg)
        .await
        .unwrap();
    sqlx::query("DELETE FROM workspaces WHERE id = ANY($1)")
        .bind([alice, mallory])
        .execute(&store.pg)
        .await
        .unwrap();
}
