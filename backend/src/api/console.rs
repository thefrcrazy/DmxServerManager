use actix_web::{web, HttpRequest, HttpResponse, Result};
use actix_ws::Message;
use futures::StreamExt;
use tracing::{error, info};

use crate::error::AppError;
use crate::services::ProcessManager;

pub async fn ws_handler(
    req: HttpRequest,
    body: web::Payload,
    path: web::Path<String>,
    pm: web::Data<ProcessManager>,
) -> Result<HttpResponse, AppError> {
    let server_id = path.into_inner();

    let (response, mut session, mut msg_stream) = actix_ws::handle(&req, body)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // Get log receiver for this server
    let mut log_rx = pm.subscribe_logs(&server_id);

    info!("WebSocket connected for server: {}", server_id);

    // Spawn task to handle WebSocket messages
    let server_id_clone = server_id.clone();
    let pm_clone = pm.clone();

    actix_web::rt::spawn(async move {
        loop {
            tokio::select! {
                // Handle incoming messages from client
                Some(msg) = msg_stream.next() => {
                    match msg {
                        Ok(Message::Text(text)) => {
                            // Client sending command to server
                            if let Err(e) = pm_clone.send_command(&server_id_clone, &text).await {
                                error!("Failed to send command: {}", e);
                            }
                        }
                        Ok(Message::Ping(bytes)) => {
                            if session.pong(&bytes).await.is_err() {
                                break;
                            }
                        }
                        Ok(Message::Close(_)) | Err(_) => {
                            break;
                        }
                        _ => {}
                    }
                }
                // Forward server logs to client
                // Forward server logs to client
                result = log_rx.recv() => {
                   match result {
                       Ok(log_line) => {
                           if session.text(log_line).await.is_err() {
                               break;
                           }
                       }
                       Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                           // Lagged, just skip
                       }
                       Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                           break; 
                       }
                   }
                }
            }
        }

        info!("WebSocket disconnected for server: {}", server_id_clone);
        let _ = session.close(None).await;
    });

    Ok(response)
}
