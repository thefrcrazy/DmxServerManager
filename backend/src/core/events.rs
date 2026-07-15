use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiEvent {
    pub id: String,
    pub event_type: String,
    pub instance_id: Option<String>,
    #[serde(skip)]
    pub audience_user_id: Option<String>,
    pub payload: serde_json::Value,
    pub created_at: String,
}

#[derive(Clone)]
pub struct EventHub {
    sender: broadcast::Sender<ApiEvent>,
    history: Arc<Mutex<VecDeque<ApiEvent>>>,
    capacity: usize,
}

#[derive(Debug, Clone)]
pub enum ReplayResult {
    NotRequested,
    Found(Vec<ApiEvent>),
    Missing,
}

impl EventHub {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            history: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ApiEvent> {
        self.sender.subscribe()
    }

    pub fn publish(
        &self,
        event_type: impl Into<String>,
        instance_id: Option<String>,
        payload: serde_json::Value,
    ) {
        let event = ApiEvent {
            id: uuid::Uuid::new_v4().to_string(),
            event_type: event_type.into(),
            instance_id,
            audience_user_id: None,
            payload,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        {
            let mut history = self
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if history.len() == self.capacity {
                history.pop_front();
            }
            history.push_back(event.clone());
        }
        let _ = self.sender.send(event);
    }

    pub fn publish_to_user(
        &self,
        event_type: impl Into<String>,
        user_id: impl Into<String>,
        payload: serde_json::Value,
    ) {
        let event = ApiEvent {
            id: uuid::Uuid::new_v4().to_string(),
            event_type: event_type.into(),
            instance_id: None,
            audience_user_id: Some(user_id.into()),
            payload,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        {
            let mut history = self
                .history
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if history.len() == self.capacity {
                history.pop_front();
            }
            history.push_back(event.clone());
        }
        let _ = self.sender.send(event);
    }

    pub fn replay_after(&self, last_event_id: Option<&str>) -> ReplayResult {
        let Some(last_event_id) = last_event_id else {
            return ReplayResult::NotRequested;
        };
        let history = self
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(position) = history.iter().position(|event| event.id == last_event_id) else {
            return ReplayResult::Missing;
        };
        ReplayResult::Found(history.iter().skip(position + 1).cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_is_bounded_and_reports_missing_ids() {
        let events = EventHub::new(2);
        events.publish("first", None, serde_json::json!({}));
        let first = match events.replay_after(Some("missing")) {
            ReplayResult::Missing => events.history.lock().unwrap().back().unwrap().id.clone(),
            _ => panic!("unknown ids must request a reset"),
        };
        events.publish("second", None, serde_json::json!({}));
        events.publish("third", None, serde_json::json!({}));

        assert!(matches!(
            events.replay_after(Some(&first)),
            ReplayResult::Missing
        ));
        let second = events.history.lock().unwrap().front().unwrap().id.clone();
        let ReplayResult::Found(replayed) = events.replay_after(Some(&second)) else {
            panic!("the retained cursor must be replayable");
        };
        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0].event_type, "third");
    }
}
