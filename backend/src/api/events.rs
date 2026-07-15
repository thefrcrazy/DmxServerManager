use std::{collections::HashSet, convert::Infallible, time::Duration};

use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::sse::{Event, KeepAlive, Sse},
};
use futures::{Stream, StreamExt, stream};
use serde::Deserialize;

use crate::{
    api::auth::{AuthUser, authorize_instance, instance_grant_scope, refresh_session_auth},
    core::{
        AppState,
        error::AppError,
        events::{ApiEvent, ReplayResult},
    },
};

#[derive(Debug, Deserialize)]
pub struct EventQuery {
    pub server_id: Option<String>,
}

pub async fn stream_events(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<EventQuery>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, AppError> {
    let can_read_servers = auth.has_permission("server.read");
    let can_read_chat = auth.has_permission("chat.read");
    let can_read_notifications = auth.has_permission("notifications.read");
    if !can_read_servers && !can_read_chat && !can_read_notifications {
        return Err(AppError::Forbidden("auth.permission_denied".into()));
    }
    let selected_server = if let Some(server_id) = query.server_id {
        auth.require("server.read")?;
        uuid::Uuid::parse_str(&server_id)
            .map_err(|_| AppError::BadRequest("servers.invalid_id".into()))?;
        authorize_instance(&state, &auth, &server_id, "server.read").await?;
        Some(server_id)
    } else {
        None
    };
    let grant_scope = instance_grant_scope(&state, &auth).await?;
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    // Subscribe before taking the replay snapshot. Any event published during
    // the snapshot is present in the live receiver; duplicate IDs are filtered.
    let receiver = state.events.subscribe();
    let replay = state.events.replay_after(last_event_id.as_deref());
    let reset_required = matches!(&replay, ReplayResult::Missing);
    let replay_available = matches!(&replay, ReplayResult::Found(_));
    let mut replay_ids = HashSet::new();
    let replay_events = match replay {
        ReplayResult::Found(events) => events
            .into_iter()
            .filter(|event| {
                visible(
                    event,
                    selected_server.as_deref(),
                    &grant_scope,
                    &auth,
                    can_read_chat,
                    can_read_notifications,
                )
            })
            .inspect(|event| {
                replay_ids.insert(event.id.clone());
            })
            .map(api_event_to_sse)
            .collect::<Vec<_>>(),
        ReplayResult::NotRequested | ReplayResult::Missing => Vec::new(),
    };
    let replayed = replay_available;
    let connected = stream::once(async move {
        Ok(Event::default().event("stream.connected").data(
            serde_json::json!({
                "last_event_id": last_event_id,
                "replayed": replayed,
            })
            .to_string(),
        ))
    });
    let reset = stream::iter(reset_required.then(|| {
        Ok(Event::default().event("stream.reset").data(
            serde_json::json!({
                "type": "stream.reset",
                "server_id": selected_server.clone(),
                "payload": {"reason": "cursor_not_available"},
                "created_at": chrono::Utc::now().to_rfc3339(),
            })
            .to_string(),
        ))
    }));
    let replay = stream::iter(replay_events.into_iter().map(Ok));
    let revalidation_state = state.clone();
    let revalidation_interval = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(5),
        Duration::from_secs(5),
    );
    let output = stream::unfold(
        (
            receiver,
            grant_scope,
            selected_server,
            replay_ids,
            auth,
            can_read_chat,
            can_read_notifications,
            revalidation_state,
            revalidation_interval,
        ),
        |(
            mut receiver,
            mut grant_scope,
            selected_server,
            mut replay_ids,
            mut auth,
            mut can_read_chat,
            mut can_read_notifications,
            revalidation_state,
            mut revalidation_interval,
        )| async move {
            loop {
                tokio::select! {
                    _ = revalidation_interval.tick() => {
                        let refreshed = match refresh_session_auth(
                            &revalidation_state.pool,
                            &auth.session_id,
                        )
                        .await
                        {
                            Ok(Some(refreshed))
                                if refreshed.id == auth.id && !refreshed.must_change_password =>
                            {
                                refreshed
                            }
                            Ok(Some(_)) | Ok(None) | Err(_) => return None,
                        };
                        let refreshed_scope = match instance_grant_scope(
                            &revalidation_state,
                            &refreshed,
                        )
                        .await
                        {
                            Ok(scope) => scope,
                            Err(_) => return None,
                        };
                        if selected_server.as_deref().is_some_and(|instance_id| {
                            !refreshed_scope.allows(&refreshed, instance_id, "server.read")
                        }) {
                            return None;
                        }
                        let still_has_stream_permission = refreshed.has_permission("server.read")
                            || refreshed.has_permission("chat.read")
                            || refreshed.has_permission("notifications.read");
                        if !still_has_stream_permission {
                            return None;
                        }
                        can_read_chat = refreshed.has_permission("chat.read");
                        can_read_notifications = refreshed.has_permission("notifications.read");
                        auth = refreshed;
                        grant_scope = refreshed_scope;
                    }
                    message = receiver.recv() => match message {
                    Ok(api_event) => {
                        if replay_ids.remove(&api_event.id) {
                            continue;
                        }
                        if !visible(
                            &api_event,
                            selected_server.as_deref(),
                            &grant_scope,
                            &auth,
                            can_read_chat,
                            can_read_notifications,
                        ) {
                            continue;
                        }
                        return Some((
                            Ok(api_event_to_sse(api_event)),
                            (
                                receiver,
                                grant_scope,
                                selected_server,
                                replay_ids,
                                auth,
                                can_read_chat,
                                can_read_notifications,
                                revalidation_state,
                                revalidation_interval,
                            ),
                        ));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        let event = Event::default()
                            .event("stream.lagged")
                            .data(skipped.to_string());
                        return Some((
                            Ok(event),
                            (
                                receiver,
                                grant_scope,
                                selected_server,
                                replay_ids,
                                auth,
                                can_read_chat,
                                can_read_notifications,
                                revalidation_state,
                                revalidation_interval,
                            ),
                        ));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                    },
                }
            }
        },
    );

    Ok(
        Sse::new(connected.chain(reset).chain(replay).chain(output)).keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keep-alive"),
        ),
    )
}

fn visible(
    event: &ApiEvent,
    selected_server: Option<&str>,
    grant_scope: &crate::api::auth::InstanceGrantScope,
    auth: &AuthUser,
    can_read_chat: bool,
    can_read_notifications: bool,
) -> bool {
    if event
        .audience_user_id
        .as_deref()
        .is_some_and(|audience| audience != auth.id)
    {
        return false;
    }
    if let Some(selected) = selected_server {
        if event.instance_id.as_deref() != Some(selected) {
            return false;
        }
        grant_scope.allows(auth, selected, required_instance_permission(event))
    } else if event.event_type.starts_with("chat.") {
        can_read_chat
    } else if event.event_type.starts_with("notification.") {
        can_read_notifications && event.audience_user_id.as_deref() == Some(auth.id.as_str())
    } else if let Some(instance_id) = &event.instance_id {
        grant_scope.allows(auth, instance_id, required_instance_permission(event))
    } else {
        // Catalogue, release and webhook events are Owner-only. Dedicated
        // per-user global features are handled above.
        auth.role == "owner"
    }
}

fn required_instance_permission(event: &ApiEvent) -> &'static str {
    if event.event_type == "job.waiting_for_user"
        || (event.event_type == "job.updated"
            && event
                .payload
                .get("interaction")
                .is_some_and(|interaction| !interaction.is_null()))
    {
        "server.update_game"
    } else if event.event_type.starts_with("job.") {
        "job.read"
    } else if matches!(
        event.event_type.as_str(),
        "server.log" | "server.console_command"
    ) {
        "server.console.read"
    } else if event.event_type.starts_with("file.") {
        "server.files.read"
    } else if event.event_type.starts_with("backup.") {
        "server.backup.read"
    } else if event.event_type.starts_with("schedule.") {
        "schedule.manage"
    } else if event.event_type.starts_with("mod.") {
        "mods.manage"
    } else {
        "server.read"
    }
}

fn api_event_to_sse(api_event: ApiEvent) -> Event {
    let event_type = api_event.event_type.clone();
    let data = serde_json::json!({
        "type": event_type,
        "server_id": api_event.instance_id,
        "payload": api_event.payload,
        "created_at": api_event.created_at,
    });
    Event::default()
        .id(api_event.id)
        .event(api_event.event_type)
        .json_data(data)
        .unwrap_or_else(|_| Event::default().event("serialization_error"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::auth::InstanceGrantScope;

    fn event(event_type: &str, instance_id: Option<&str>, audience: Option<&str>) -> ApiEvent {
        ApiEvent {
            id: "event-id".into(),
            event_type: event_type.into(),
            instance_id: instance_id.map(str::to_string),
            audience_user_id: audience.map(str::to_string),
            payload: serde_json::json!({}),
            created_at: "2026-07-13T00:00:00Z".into(),
        }
    }

    #[test]
    fn private_notifications_never_cross_user_boundaries() {
        let notification = event("notification.created", None, Some("alice"));
        let alice = AuthUser::for_test("alice", "viewer", ["notifications.read"]);
        let bob = AuthUser::for_test("bob", "viewer", ["notifications.read"]);
        let scope = InstanceGrantScope::for_test(false, []);
        assert!(visible(&notification, None, &scope, &alice, false, true,));
        assert!(!visible(&notification, None, &scope, &bob, true, true,));
    }

    #[test]
    fn instance_and_chat_events_apply_independent_permissions() {
        let auth = AuthUser::for_test("alice", "viewer", ["server.read", "chat.read"]);
        let scope =
            InstanceGrantScope::for_test(false, [("server-a".to_string(), Vec::<String>::new())]);
        assert!(visible(
            &event("server.state", Some("server-a"), None),
            None,
            &scope,
            &auth,
            false,
            false,
        ));
        assert!(!visible(
            &event("server.state", Some("server-b"), None),
            None,
            &scope,
            &auth,
            true,
            false,
        ));
        assert!(visible(
            &event("chat.message_created", None, None),
            None,
            &scope,
            &auth,
            true,
            false,
        ));
    }

    #[test]
    fn waiting_interactions_and_logs_require_their_high_risk_permissions() {
        let viewer = AuthUser::for_test("viewer", "viewer", ["server.read", "job.read"]);
        let scope =
            InstanceGrantScope::for_test(false, [("server-a".to_string(), Vec::<String>::new())]);
        let waiting = ApiEvent {
            payload: serde_json::json!({
                "job_id": "job-id",
                "interaction": {
                    "kind": "oauth_device",
                    "verification_uri": "https://oauth.accounts.hytale.com/oauth2/device/verify",
                    "user_code": "SECRET-CODE"
                }
            }),
            ..event("job.waiting_for_user", Some("server-a"), None)
        };
        let updated = ApiEvent {
            payload: serde_json::json!({"interaction": waiting.payload["interaction"]}),
            ..event("job.updated", Some("server-a"), None)
        };

        assert!(!visible(&waiting, None, &scope, &viewer, false, false));
        assert!(!visible(&updated, None, &scope, &viewer, false, false));
        assert!(!visible(
            &event("server.log", Some("server-a"), None),
            None,
            &scope,
            &viewer,
            false,
            false,
        ));

        let operator = AuthUser::for_test(
            "operator",
            "operator",
            [
                "server.read",
                "job.read",
                "server.update_game",
                "server.console.read",
            ],
        );
        assert!(visible(&waiting, None, &scope, &operator, false, false));
        assert!(visible(
            &event("server.log", Some("server-a"), None),
            None,
            &scope,
            &operator,
            false,
            false,
        ));
    }
}
