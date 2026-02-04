use axum::{
    extract::{Path, State, ws::{Message, WebSocket, WebSocketUpgrade}},
    response::IntoResponse,
};
use tracing::{error, info};
use futures::{sink::SinkExt, stream::StreamExt};

use crate::core::AppState;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Path(server_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, server_id, state))
}

async fn handle_socket(socket: WebSocket, server_id: String, state: AppState) {
    let pm = state.process_manager;
    let mut log_rx = pm.subscribe_logs(&server_id);

    info!("WebSocket connected for server: {}", server_id);

    let (mut sender, mut receiver) = socket.split();

    // Send last known metrics immediately
    if let Some(metrics) = pm.get_last_metrics(&server_id).await {
        let _ = sender.send(Message::Text(metrics)).await;
    }

    // Task to handle incoming messages (commands from client)
    let mut recv_task = {
        let pm = pm.clone();
        let server_id = server_id.clone();
        
        tokio::spawn(async move {
            while let Some(Ok(msg)) = receiver.next().await {
                match msg {
                    Message::Text(text) => {
                         // Client sending command to server
                         if let Err(e) = pm.send_command(&server_id, &text).await {
                             error!("Failed to send command: {}", e);
                         }
                    }
                    Message::Close(_) => return,
                    _ => {}
                }
            }
        })
    };

    // Task to broadcast logs to client
    let server_id_clone = server_id.clone();
    let mut send_task = tokio::spawn(async move {
        loop {
            // Check for log messages
            match log_rx.recv().await {
                Ok(log_line) => {
                    if sender.send(Message::Text(log_line)).await.is_err() {
                        return; // Client disconnected
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    error!("WebSocket lagged, skipped {} messages for server {}", n, server_id_clone);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    return; // Channel closed
                }
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = (&mut recv_task) => send_task.abort(),
        _ = (&mut send_task) => recv_task.abort(),
    };

    info!("WebSocket disconnected for server: {}", server_id);
}
