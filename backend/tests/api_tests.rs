// Integration tests for API handlers
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use tower::ServiceExt;

mod common;
use common::*;

// ============= Auth Tests =============

#[tokio::test]
async fn test_auth_status_returns_needs_setup_when_no_users() {
    let app = create_test_app().await;
    
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/auth/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
    
    let body = get_body_json(response).await;
    assert_eq!(body["needs_setup"], true);
}

#[tokio::test]
async fn test_register_first_user_becomes_admin() {
    let app = create_test_app().await;
    
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    json!({
                        "username": "admin",
                        "password": "password123"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::CREATED);
    
    let body = get_body_json(response).await;
    assert!(body["token"].is_string());
    assert_eq!(body["user"]["role"], "admin");
}

#[tokio::test]
async fn test_login_with_invalid_credentials_returns_401() {
    let app = create_test_app().await;
    
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    json!({
                        "username": "nonexistent",
                        "password": "wrongpassword"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_me_without_auth_returns_401() {
    let app = create_test_app().await;
    
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/auth/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

// ============= Server Tests =============

#[tokio::test]
async fn test_list_servers_empty() {
    let (app, token) = create_test_app_with_auth().await;
    
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/servers")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
    
    let body = get_body_json(response).await;
    assert!(body.is_array());
    assert_eq!(body.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_get_nonexistent_server_returns_404() {
    let (app, token) = create_test_app_with_auth().await;
    
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/servers/nonexistent-id")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// ============= System Tests =============

#[tokio::test]
async fn test_system_stats() {
    let (app, token) = create_test_app_with_auth().await;
    
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/system/stats")
                .header("Authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
    
    let body = get_body_json(response).await;
    assert!(body["cpu"].is_number());
    assert!(body["ram"].is_number());
    assert!(body["cpu_cores"].is_number());
}
