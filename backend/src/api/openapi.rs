use axum::Json;
use serde_json::{Map, Value, json};

pub async fn get_document() -> Json<Value> {
    Json(document())
}

pub fn document() -> Value {
    let mut paths = build_paths();
    apply_common_operation_contracts(&mut paths);
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "DmxServerManager API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Single-host game server management API. Secret values are write-only."
        },
        "servers": [{"url": "/api/v1"}],
        "paths": paths,
        "components": build_components()
    })
}

fn build_paths() -> Value {
    merge_objects([
        auth_paths(),
        profile_paths(),
        catalog_paths(),
        release_paths(),
        administration_paths(),
        server_paths(),
        operations_paths(),
    ])
}

fn build_components() -> Value {
    let schemas = merge_objects([
        core_schemas(),
        profile_schemas(),
        catalog_schemas(),
        release_schemas(),
        administration_schemas(),
        server_schemas(),
        operations_schemas(),
    ]);
    json!({
        "parameters": {
            "CsrfToken": {
                "name": "X-CSRF-Token", "in": "header", "required": true,
                "description": "Token bound to the current opaque session.",
                "schema": {"type": "string", "minLength": 1}
            },
            "IfMatch": {
                "name": "If-Match", "in": "header", "required": true,
                "description": "Numeric entity tag returned by the resource. Strong and weak HTTP forms are accepted.",
                "schema": {"type": "string", "pattern": "^(?:W/)?\\\"[1-9][0-9]*\\\"$"}
            },
            "StrongIfMatch": {
                "name": "If-Match", "in": "header", "required": true,
                "description": "Strong entity tag returned by the resource.",
                "schema": {"type": "string", "pattern": "^\\\"[1-9][0-9]*\\\"$"}
            },
            "IdempotencyKey": {
                "name": "Idempotency-Key", "in": "header", "required": false,
                "schema": {"type": "string", "minLength": 1, "maxLength": 128}
            },
            "ServerId": {
                "name": "id", "in": "path", "required": true,
                "schema": {"type": "string", "format": "uuid"}
            }
        },
        "headers": {
            "ETag": {
                "description": "Strong version used with If-Match.",
                "schema": {"type": "string", "pattern": "^\\\"[1-9][0-9]*\\\"$"}
            },
            "TraceId": {
                "description": "Request trace identifier.",
                "schema": {"type": "string", "format": "uuid"}
            }
        },
        "responses": {
            "Problem": {
                "description": "Request rejected.",
                "headers": {"X-Trace-Id": {"$ref": "#/components/headers/TraceId"}},
                "content": {"application/problem+json": {"schema": {"$ref": "#/components/schemas/Problem"}}}
            },
            "Unauthorized": {
                "description": "A valid opaque session cookie is required.",
                "headers": {"X-Trace-Id": {"$ref": "#/components/headers/TraceId"}},
                "content": {"application/problem+json": {"schema": {"$ref": "#/components/schemas/Problem"}}}
            },
            "Forbidden": {
                "description": "Permission, assignment or CSRF validation failed.",
                "headers": {"X-Trace-Id": {"$ref": "#/components/headers/TraceId"}},
                "content": {"application/problem+json": {"schema": {"$ref": "#/components/schemas/Problem"}}}
            },
            "Conflict": {
                "description": "The resource state conflicts with the request.",
                "headers": {"X-Trace-Id": {"$ref": "#/components/headers/TraceId"}},
                "content": {"application/problem+json": {"schema": {"$ref": "#/components/schemas/Problem"}}}
            },
            "PreconditionRequired": {
                "description": "A valid If-Match header is required.",
                "headers": {"X-Trace-Id": {"$ref": "#/components/headers/TraceId"}},
                "content": {"application/problem+json": {"schema": {"$ref": "#/components/schemas/Problem"}}}
            },
            "TooManyRequests": {
                "description": "Too many authentication attempts.",
                "headers": {
                    "X-Trace-Id": {"$ref": "#/components/headers/TraceId"},
                    "Retry-After": {"schema": {"type": "integer", "minimum": 1}}
                },
                "content": {"application/problem+json": {"schema": {"$ref": "#/components/schemas/Problem"}}}
            }
        },
        "securitySchemes": {
            "cookieAuth": {
                "type": "apiKey", "in": "cookie", "name": "dmx_session",
                "description": "Opaque HttpOnly SameSite=Strict session cookie. Secure is mandatory outside loopback development."
            }
        },
        "schemas": schemas
    })
}

fn apply_common_operation_contracts(paths: &mut Value) {
    let Some(paths) = paths.as_object_mut() else {
        return;
    };
    for (path, item) in paths {
        let Some(item) = item.as_object_mut() else {
            continue;
        };
        for (method, operation) in item {
            if !matches!(method.as_str(), "get" | "post" | "put" | "patch" | "delete") {
                continue;
            }
            let Some(operation) = operation.as_object_mut() else {
                continue;
            };
            let public = matches!(
                (path.as_str(), method.as_str()),
                ("/health", "get")
                    | ("/auth/status", "get")
                    | ("/auth/setup", "post")
                    | ("/auth/login", "post")
            );
            if !public {
                operation.insert("security".into(), json!([{"cookieAuth": []}]));
                if matches!(method.as_str(), "post" | "put" | "patch" | "delete") {
                    operation
                        .entry("parameters")
                        .or_insert_with(|| json!([]))
                        .as_array_mut()
                        .expect("OpenAPI operation parameters must be an array")
                        .push(json!({"$ref": "#/components/parameters/CsrfToken"}));
                }
            }
            let responses = operation
                .entry("responses")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .expect("OpenAPI operation responses must be an object");
            responses
                .entry("default")
                .or_insert_with(|| json!({"$ref": "#/components/responses/Problem"}));
            if !public {
                responses
                    .entry("401")
                    .or_insert_with(|| json!({"$ref": "#/components/responses/Unauthorized"}));
                responses
                    .entry("403")
                    .or_insert_with(|| json!({"$ref": "#/components/responses/Forbidden"}));
            }
        }
    }
}

fn merge_objects<const N: usize>(values: [Value; N]) -> Value {
    let mut merged = Map::new();
    for value in values {
        let object = value
            .as_object()
            .expect("OpenAPI groups must be JSON objects");
        for (key, value) in object {
            assert!(
                merged.insert(key.clone(), value.clone()).is_none(),
                "duplicate OpenAPI key: {key}"
            );
        }
    }
    Value::Object(merged)
}

fn auth_paths() -> Value {
    json!({
        "/health": {
            "get": {
                "operationId": "health",
                "tags": ["system"],
                "responses": {
                    "200": {"description": "Service and database are healthy.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/HealthResponse"}}}},
                    "503": {"description": "Database health check failed.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/HealthResponse"}}}}
                }
            }
        },
        "/openapi.json": {
            "get": {
                "operationId": "openApi",
                "tags": ["system"],
                "responses": {"200": {"description": "OpenAPI 3.1 document.", "content": {"application/json": {"schema": {"type": "object", "additionalProperties": true}}}}}
            }
        },
        "/auth/status": {
            "get": {
                "operationId": "setupStatus",
                "tags": ["auth"],
                "responses": {"200": {"description": "Whether initial Owner setup is required.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SetupStatus"}}}}}
            }
        },
        "/auth/setup": {
            "post": {
                "operationId": "setupOwner",
                "tags": ["auth"],
                "parameters": [{
                    "name": "X-Setup-Token", "in": "header", "required": false,
                    "description": "One-time setup token; required when the effective client is not loopback.",
                    "schema": {"type": "string", "minLength": 1}
                }],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SetupRequest"}}}},
                "responses": {"201": {
                    "description": "Owner and opaque session created.",
                    "headers": {"Set-Cookie": {"description": "HttpOnly SameSite=Strict session cookie.", "schema": {"type": "string"}}},
                    "content": {"application/json": {"schema": {"$ref": "#/components/schemas/AuthResponse"}}}
                }}
            }
        },
        "/auth/login": {
            "post": {
                "operationId": "login",
                "tags": ["auth"],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/LoginRequest"}}}},
                "responses": {
                    "200": {
                        "description": "Authenticated; session cookie and CSRF token issued.",
                        "headers": {"Set-Cookie": {"description": "HttpOnly SameSite=Strict session cookie.", "schema": {"type": "string"}}},
                        "content": {"application/json": {"schema": {"$ref": "#/components/schemas/AuthResponse"}}}
                    },
                    "429": {"$ref": "#/components/responses/TooManyRequests"}
                }
            }
        },
        "/auth/me": {
            "get": {
                "operationId": "me",
                "tags": ["auth"],
                "responses": {"200": {"description": "Current user and rotated CSRF token.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/AuthResponse"}}}}}
            }
        },
        "/auth/logout": {
            "post": {
                "operationId": "logout",
                "tags": ["auth"],
                "responses": {"200": {
                    "description": "Session revoked and cookie expired.",
                    "headers": {"Set-Cookie": {"description": "Expired session cookie.", "schema": {"type": "string"}}},
                    "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SuccessResponse"}}}
                }}
            }
        },
        "/auth/password": {
            "put": {
                "operationId": "changePassword",
                "tags": ["auth"],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ChangePasswordRequest"}}}},
                "responses": {"200": {
                    "description": "Password changed, all sessions revoked and cookie expired.",
                    "headers": {"Set-Cookie": {"description": "Expired session cookie.", "schema": {"type": "string"}}},
                    "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SuccessResponse"}}}
                }}
            }
        }
    })
}

fn profile_paths() -> Value {
    json!({
        "/game-profiles": {
            "get": {
                "operationId": "listGameProfiles", "tags": ["game profiles"],
                "responses": {"200": {"description": "Latest revision of every visible profile.", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/GameProfile"}}}}}}
            }
        },
        "/game-profiles/{id}/revisions": {
            "get": {
                "operationId": "listGameProfileRevisions", "tags": ["game profiles"],
                "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string", "minLength": 1, "maxLength": 64}}],
                "responses": {"200": {"description": "All immutable revisions for a profile.", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/GameProfile"}}}}}}
            }
        },
        "/game-profiles/{id}/version-catalog": {
            "get": {
                "operationId": "getGameProfileVersionCatalog", "tags": ["game profiles"],
                "parameters": [
                    {"name": "id", "in": "path", "required": true, "schema": {"type": "string", "minLength": 1, "maxLength": 64}},
                    {"name": "game_version", "in": "query", "required": false, "schema": {"type": "string", "minLength": 1, "maxLength": 96}},
                    {"name": "loader", "in": "query", "required": false, "schema": {"type": "string", "enum": ["vanilla", "paper", "fabric", "forge", "neoforge", "spigot", "purpur", "quilt"]}}
                ],
                "responses": {"200": {"description": "Current official game and loader versions available to the selected built-in profile.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ProfileVersionCatalog"}}}}}
            }
        },
        "/game-profiles/steam": {
            "post": {
                "operationId": "createSteamProfile", "tags": ["game profiles"],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CreateSteamProfileRequest"}}}},
                "responses": {"201": {
                    "description": "First immutable Steam profile revision created.",
                    "headers": {"ETag": {"$ref": "#/components/headers/ETag"}},
                    "content": {"application/json": {"schema": {"$ref": "#/components/schemas/GameProfile"}}}
                }}
            }
        },
        "/game-profiles/steam/{id}": {
            "put": {
                "operationId": "reviseSteamProfile", "tags": ["game profiles"],
                "parameters": [
                    {"name": "id", "in": "path", "required": true, "schema": {"type": "string", "minLength": 7, "maxLength": 64}},
                    {"$ref": "#/components/parameters/IfMatch"}
                ],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SteamProfileDefinition"}}}},
                "responses": {
                    "201": {
                        "description": "New immutable profile revision created.",
                        "headers": {"ETag": {"$ref": "#/components/headers/ETag"}},
                        "content": {"application/json": {"schema": {"$ref": "#/components/schemas/GameProfile"}}}
                    },
                    "409": {"$ref": "#/components/responses/Conflict"},
                    "428": {"$ref": "#/components/responses/PreconditionRequired"}
                }
            },
            "delete": {
                "operationId": "deleteSteamProfile", "tags": ["game profiles"],
                "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string", "minLength": 7, "maxLength": 64}}],
                "responses": {"200": {"description": "Unused custom profile and all revisions deleted.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SuccessResponse"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        }
    })
}

fn catalog_paths() -> Value {
    json!({
        "/catalog": {
            "get": {
                "operationId": "listCatalogPackages", "tags": ["catalog"],
                "parameters": [{"name": "kind", "in": "query", "required": false, "schema": {"enum": ["steam_profile", "theme"]}}],
                "responses": {"200": {"description": "Latest installed revision of each local package.", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/CatalogPackage"}}}}}}
            }
        },
        "/catalog/import": {
            "post": {
                "operationId": "importCatalogPackage", "tags": ["catalog"],
                "description": "Owner-only validated local .dmxpack import. The package never carries executable code, arbitrary CSS, shell commands or download URLs.",
                "parameters": [
                    {"name": "X-Dmx-Package-Sha256", "in": "header", "required": true, "schema": {"type": "string", "pattern": "^[0-9A-Fa-f]{64}$"}},
                    {"$ref": "#/components/parameters/IdempotencyKey"}
                ],
                "requestBody": {"required": true, "content": {
                    "application/vnd.dmxpack+zip": {"schema": {"type": "string", "format": "binary", "maxLength": 16777216}},
                    "application/zip": {"schema": {"type": "string", "format": "binary", "maxLength": 16777216}}
                }},
                "responses": {
                    "202": {"description": "Package staged and persistent import job queued.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Job"}}}},
                    "409": {"$ref": "#/components/responses/Conflict"}
                }
            }
        },
        "/catalog/theme": {
            "get": {
                "operationId": "getActiveCatalogTheme", "tags": ["catalog"],
                "description": "Returns only the validated closed design-token set and local checksum-pinned PNG assets for the exact active revision. No manifest, CSS, HTML or JavaScript is exposed.",
                "responses": {"200": {
                    "description": "Global active theme, or the built-in default.",
                    "headers": {"ETag": {"$ref": "#/components/headers/ETag"}},
                    "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ActiveTheme"}}}
                }}
            },
            "put": {
                "operationId": "selectCatalogTheme", "tags": ["catalog"],
                "description": "Owner only. Selects one exact installed theme revision or restores the built-in default.",
                "parameters": [{"$ref": "#/components/parameters/IfMatch"}],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ThemeSelection"}}}},
                "responses": {
                    "200": {
                        "description": "Global theme selection updated.",
                        "headers": {"ETag": {"$ref": "#/components/headers/ETag"}},
                        "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ActiveTheme"}}}
                    },
                    "409": {"$ref": "#/components/responses/Conflict"},
                    "428": {"$ref": "#/components/responses/PreconditionRequired"}
                }
            }
        },
        "/catalog/{kind}/{id}/revisions": {
            "get": {
                "operationId": "listCatalogRevisions", "tags": ["catalog"],
                "parameters": catalog_path_parameters(false, false),
                "responses": {"200": {"description": "All active immutable revisions for the package.", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/CatalogPackage"}}}}}}
            }
        },
        "/catalog/{kind}/{id}/revisions/{revision}": {
            "get": {
                "operationId": "getCatalogRevision", "tags": ["catalog"],
                "parameters": catalog_path_parameters(true, false),
                "responses": {"200": {"description": "One immutable installed package revision.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CatalogPackage"}}}}}
            },
            "delete": {
                "operationId": "deleteCatalogRevision", "tags": ["catalog"],
                "description": "Owner-only deletion. A revision pinned by an instance cannot be deleted and its revision number can never be reused.",
                "parameters": catalog_path_parameters(true, false),
                "responses": {
                    "200": {"description": "Unused package revision deleted and tombstoned.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SuccessResponse"}}}},
                    "409": {"$ref": "#/components/responses/Conflict"}
                }
            }
        },
        "/catalog/{kind}/{id}/revisions/{revision}/assets/{asset}": {
            "get": {
                "operationId": "getCatalogAsset", "tags": ["catalog"],
                "parameters": catalog_path_parameters(true, true),
                "responses": {"200": {
                    "description": "Checksum-verified PNG asset declared by the package manifest.",
                    "headers": {"ETag": {"description": "Quoted SHA-256 of the immutable asset.", "schema": {"type": "string", "pattern": "^\\\"[0-9a-f]{64}\\\"$"}}},
                    "content": {"image/png": {"schema": {"type": "string", "format": "binary"}}}
                }}
            }
        }
    })
}

fn catalog_path_parameters(revision: bool, asset: bool) -> Value {
    let mut parameters = vec![
        json!({"name": "kind", "in": "path", "required": true, "schema": {"enum": ["steam_profile", "theme"]}}),
        json!({"name": "id", "in": "path", "required": true, "schema": {"type": "string", "minLength": 7, "maxLength": 64, "pattern": "^(?:steam|theme)-[a-z0-9]+(?:-[a-z0-9]+)*$"}}),
    ];
    if revision {
        parameters.push(json!({"name": "revision", "in": "path", "required": true, "schema": {"type": "integer", "minimum": 1}}));
    }
    if asset {
        parameters.push(json!({"name": "asset", "in": "path", "required": true, "schema": {"enum": ["icon", "logo", "preview"]}}));
    }
    Value::Array(parameters)
}

fn release_paths() -> Value {
    json!({
        "/releases/panel": {
            "get": {
                "operationId": "getPanelReleaseStatus", "tags": ["releases"],
                "description": "Owner only. Returns only locally verified release metadata; the configured manifest URL and Ed25519 public key are never exposed. The panel never replaces itself.",
                "responses": {"200": {"description": "Current signed-release check state and the platform-specific verified target, when available.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ReleaseStatus"}}}}}
            }
        },
        "/releases/panel/check": {
            "post": {
                "operationId": "checkPanelRelease", "tags": ["releases"],
                "description": "Owner only. Fetches the bounded manifest, verifies its Ed25519 signature and every required checksum, and returns instructions without executing them.",
                "responses": {"200": {"description": "Signed-release check completed. Verification failures are represented by state and a non-sensitive error_code.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ReleaseStatus"}}}}}
            }
        }
    })
}

fn administration_paths() -> Value {
    json!({
        "/audit": {
            "get": {
                "operationId": "listAuditEvents", "tags": ["administration"],
                "parameters": [
                    {"name": "before_id", "in": "query", "required": false, "schema": {"type": "integer", "minimum": 1}},
                    {"name": "limit", "in": "query", "required": false, "schema": {"type": "integer", "minimum": 1, "maximum": 200, "default": 100}},
                    {"name": "resource_type", "in": "query", "required": false, "schema": {"type": "string", "maxLength": 64}},
                    {"name": "resource_id", "in": "query", "required": false, "schema": {"type": "string", "maxLength": 128}},
                    {"name": "outcome", "in": "query", "required": false, "schema": {"enum": ["success", "denied", "failure"]}}
                ],
                "responses": {"200": {"description": "Immutable audit page.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/AuditPage"}}}}}
            }
        },
        "/permissions": {
            "get": {
                "operationId": "listPermissions", "tags": ["administration"],
                "responses": {"200": {"description": "Closed permission catalogue; Owner only.", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/Permission"}}}}}}
            }
        },
        "/roles": {
            "get": {
                "operationId": "listRoles", "tags": ["administration"],
                "responses": {"200": {"description": "Roles visible to the current user administrator.", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/Role"}}}}}}
            },
            "post": {
                "operationId": "createRole", "tags": ["administration"],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CreateRoleRequest"}}}},
                "responses": {"201": {"description": "Custom role created.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Role"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        },
        "/roles/{id}": {
            "patch": {
                "operationId": "updateRole", "tags": ["administration"],
                "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/UpdateRoleRequest"}}}},
                "responses": {"200": {"description": "Custom role updated and affected sessions revoked.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Role"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            },
            "delete": {
                "operationId": "deleteRole", "tags": ["administration"],
                "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}],
                "responses": {"200": {"description": "Unused custom role deleted.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SuccessResponse"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        },
        "/users": {
            "get": {
                "operationId": "listUsers", "tags": ["administration"],
                "responses": {"200": {"description": "Manageable local users.", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/ManagedUser"}}}}}}
            },
            "post": {
                "operationId": "createUser", "tags": ["administration"],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CreateUserRequest"}}}},
                "responses": {"201": {"description": "Local account created with mandatory password change.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ManagedUser"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        },
        "/users/{id}": {
            "patch": {
                "operationId": "updateUser", "tags": ["administration"],
                "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string", "format": "uuid"}}],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/UpdateUserRequest"}}}},
                "responses": {"200": {"description": "Account updated and sessions revoked.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ManagedUser"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        },
        "/users/{id}/instances": {
            "get": {
                "operationId": "listUserInstanceGrants", "tags": ["administration"],
                "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string", "format": "uuid"}}],
                "responses": {"200": {"description": "Instance assignments for a manageable user.", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/InstanceGrant"}}}}}}
            }
        },
        "/users/{user_id}/instances/{instance_id}": {
            "put": {
                "operationId": "setUserInstanceGrant", "tags": ["administration"],
                "parameters": [
                    {"name": "user_id", "in": "path", "required": true, "schema": {"type": "string", "format": "uuid"}},
                    {"name": "instance_id", "in": "path", "required": true, "schema": {"type": "string", "format": "uuid"}}
                ],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SetGrantRequest"}}}},
                "responses": {"200": {"description": "Assignment created or replaced.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/InstanceGrant"}}}}}
            },
            "delete": {
                "operationId": "deleteUserInstanceGrant", "tags": ["administration"],
                "parameters": [
                    {"name": "user_id", "in": "path", "required": true, "schema": {"type": "string", "format": "uuid"}},
                    {"name": "instance_id", "in": "path", "required": true, "schema": {"type": "string", "format": "uuid"}}
                ],
                "responses": {"200": {"description": "Assignment removed.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SuccessResponse"}}}}}
            }
        }
    })
}

fn server_paths() -> Value {
    json!({
        "/servers": {
            "get": {
                "operationId": "listServers", "tags": ["servers"],
                "responses": {"200": {"description": "Instances assigned to the current user.", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/Instance"}}}}}}
            },
            "post": {
                "operationId": "createServer", "tags": ["servers"],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CreateInstanceRequest"}}}},
                "responses": {"201": {
                    "description": "Instance metadata created; installation is a separate job.",
                    "headers": {"ETag": {"$ref": "#/components/headers/ETag"}},
                    "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Instance"}}}
                }, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        },
        "/servers/{id}": {
            "get": {
                "operationId": "getServer", "tags": ["servers"],
                "parameters": [{"$ref": "#/components/parameters/ServerId"}],
                "responses": {"200": {
                    "description": "Instance configuration and current state.",
                    "headers": {"ETag": {"$ref": "#/components/headers/ETag"}},
                    "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Instance"}}}
                }}
            },
            "patch": {
                "operationId": "updateServer", "tags": ["servers"],
                "parameters": [{"$ref": "#/components/parameters/ServerId"}, {"$ref": "#/components/parameters/IfMatch"}],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/UpdateInstanceRequest"}}}},
                "responses": {
                    "200": {
                        "description": "Instance updated; install inputs may reset installation state.",
                        "headers": {"ETag": {"$ref": "#/components/headers/ETag"}},
                        "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Instance"}}}
                    },
                    "409": {"$ref": "#/components/responses/Conflict"}, "428": {"$ref": "#/components/responses/PreconditionRequired"}
                }
            },
            "delete": {
                "operationId": "deleteServer", "tags": ["servers"],
                "parameters": [{"$ref": "#/components/parameters/ServerId"}],
                "responses": {"200": {"description": "Stopped instance deleted; attached game data is detached.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SuccessResponse"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        },
        "/servers/{id}/profile-revision": {
            "put": {
                "operationId": "setServerProfileRevision", "tags": ["servers"],
                "parameters": [{"$ref": "#/components/parameters/ServerId"}, {"$ref": "#/components/parameters/IfMatch"}],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SetProfileRevisionRequest"}}}},
                "responses": {
                    "200": {
                        "description": "Instance pinned to a newer profile revision and marked for reinstall.",
                        "headers": {"ETag": {"$ref": "#/components/headers/ETag"}},
                        "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Instance"}}}
                    },
                    "409": {"$ref": "#/components/responses/Conflict"}, "428": {"$ref": "#/components/responses/PreconditionRequired"}
                }
            }
        },
        "/servers/{id}/secrets": {
            "get": {
                "operationId": "listSecretStatus", "tags": ["servers"],
                "parameters": [{"$ref": "#/components/parameters/ServerId"}],
                "responses": {"200": {"description": "Allowed secret names and configured flags only.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SecretStatusList"}}}}}
            }
        },
        "/servers/{id}/secrets/{name}": {
            "put": {
                "operationId": "setServerSecret", "tags": ["servers"],
                "parameters": [{"$ref": "#/components/parameters/ServerId"}, {"name": "name", "in": "path", "required": true, "schema": {"type": "string"}}],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SetSecretRequest"}}}},
                "responses": {"200": {"description": "Encrypted secret stored; value is never returned.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SecretStatus"}}}}}
            },
            "delete": {
                "operationId": "deleteServerSecret", "tags": ["servers"],
                "parameters": [{"$ref": "#/components/parameters/ServerId"}, {"name": "name", "in": "path", "required": true, "schema": {"type": "string"}}],
                "responses": {"200": {"description": "Encrypted secret removed.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SecretStatus"}}}}}
            }
        },
        "/servers/{id}/actions/install": {
            "post": {
                "operationId": "installServer", "tags": ["servers"],
                "parameters": [{"$ref": "#/components/parameters/ServerId"}, {"$ref": "#/components/parameters/IdempotencyKey"}],
                "responses": {"202": {"description": "Persistent installation or game-update job accepted.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Job"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        },
        "/servers/{id}/actions/start": {
            "post": {
                "operationId": "startServer", "tags": ["servers"],
                "parameters": [{"$ref": "#/components/parameters/ServerId"}, {"$ref": "#/components/parameters/IdempotencyKey"}],
                "responses": {"202": {"description": "Persistent start job accepted.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Job"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        },
        "/servers/{id}/actions/stop": {
            "post": {
                "operationId": "stopServer", "tags": ["servers"],
                "parameters": [{"$ref": "#/components/parameters/ServerId"}, {"$ref": "#/components/parameters/IdempotencyKey"}],
                "responses": {"202": {"description": "Persistent graceful-stop job accepted.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Job"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        },
        "/servers/{id}/actions/restart": {
            "post": {
                "operationId": "restartServer", "tags": ["servers"],
                "parameters": [{"$ref": "#/components/parameters/ServerId"}, {"$ref": "#/components/parameters/IdempotencyKey"}],
                "responses": {"202": {"description": "Persistent restart job accepted.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Job"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        },
        "/servers/{id}/actions/kill": {
            "post": {
                "operationId": "killServer", "tags": ["servers"],
                "parameters": [{"$ref": "#/components/parameters/ServerId"}, {"$ref": "#/components/parameters/IdempotencyKey"}],
                "responses": {"202": {"description": "Persistent forced-stop job accepted.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Job"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        },
        "/servers/{id}/console": {
            "post": {
                "operationId": "sendConsoleCommand", "tags": ["servers"],
                "parameters": [{"$ref": "#/components/parameters/ServerId"}],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ConsoleRequest"}}}},
                "responses": {"202": {"description": "Bounded command written to the managed process standard input.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ConsoleResponse"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        },
        "/servers/{id}/logs": {
            "get": {
                "operationId": "getServerLogHistory", "tags": ["servers"],
                "parameters": [
                    {"$ref": "#/components/parameters/ServerId"},
                    {"name": "source", "in": "query", "schema": {"enum": ["install", "console"], "default": "console"}},
                    {"name": "limit", "in": "query", "schema": {"type": "integer", "minimum": 1, "maximum": 500, "default": 500}}
                ],
                "responses": {"200": {"description": "Bounded persisted installer or game console history.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/LogHistoryResponse"}}}}}
            }
        },
        "/servers/{id}/imports/zip": {
            "post": {
                "operationId": "importServerZip", "tags": ["imports"],
                "parameters": [
                    {"$ref": "#/components/parameters/ServerId"}, {"$ref": "#/components/parameters/IdempotencyKey"},
                    {"name": "X-Dmx-Archive-Sha256", "in": "header", "required": false, "description": "Required for an Owner-supplied Bedrock archive requested by a waiting install job.", "schema": {"type": "string", "pattern": "^[0-9A-Fa-f]{64}$"}}
                ],
                "requestBody": {"required": true, "content": {
                    "application/zip": {"schema": {"type": "string", "format": "binary"}},
                    "application/octet-stream": {"schema": {"type": "string", "format": "binary"}}
                }},
                "responses": {"202": {"description": "Persistent ZIP import job accepted, or waiting Bedrock install requeued.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Job"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        },
        "/servers/{id}/imports/copy": {
            "post": {
                "operationId": "copyExistingServer", "tags": ["imports"],
                "parameters": [{"$ref": "#/components/parameters/ServerId"}, {"$ref": "#/components/parameters/IdempotencyKey"}],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ImportSourceRequest"}}}},
                "responses": {"202": {"description": "Persistent copy-into-managed-storage job accepted.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Job"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        },
        "/servers/{id}/imports/attach": {
            "post": {
                "operationId": "attachExistingServer", "tags": ["imports"],
                "parameters": [{"$ref": "#/components/parameters/ServerId"}, {"$ref": "#/components/parameters/IdempotencyKey"}],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ImportSourceRequest"}}}},
                "responses": {"202": {"description": "Owner-only persistent attach job accepted.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Job"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        }
    })
}

fn operations_paths() -> Value {
    json!({
        "/jobs": {
            "get": {
                "operationId": "listJobs", "tags": ["jobs"],
                "responses": {"200": {"description": "Visible jobs, newest first, capped at 200.", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/Job"}}}}}}
            }
        },
        "/jobs/{id}": {
            "get": {
                "operationId": "getJob", "tags": ["jobs"],
                "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string", "format": "uuid"}}],
                "responses": {"200": {"description": "Job state.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Job"}}}}}
            }
        },
        "/jobs/{id}/cancel": {
            "post": {
                "operationId": "cancelJob", "tags": ["jobs"],
                "description": "Cancels a queued, running or waiting installation job when cancellation is still possible.",
                "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string", "format": "uuid"}}],
                "responses": {
                    "202": {"description": "Cancellation accepted.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Job"}}}},
                    "409": {"$ref": "#/components/responses/Conflict"}
                }
            }
        },
        "/events": {
            "get": {
                "operationId": "streamEvents", "tags": ["events"],
                "description": "Authenticated SSE stream. Event IDs can be replayed while they remain in the bounded server history.",
                "parameters": [
                    {"name": "server_id", "in": "query", "required": false, "schema": {"type": "string", "format": "uuid"}},
                    {"name": "Last-Event-ID", "in": "header", "required": false, "schema": {"type": "string", "format": "uuid"}}
                ],
                "responses": {
                    "200": {
                        "description": "SSE events. JSON data events conform to EventEnvelope; stream.connected and stream.lagged are control events.",
                        "headers": {"Cache-Control": {"schema": {"type": "string"}}},
                        "content": {"text/event-stream": {"schema": {"type": "string"}}}
                    }
                }
            }
        },
        "/backups": {
            "get": {
                "operationId": "listBackups", "tags": ["backups"],
                "parameters": [{"name": "instance_id", "in": "query", "required": true, "schema": {"type": "string", "format": "uuid"}}],
                "responses": {"200": {"description": "Backups for the selected instance.", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/Backup"}}}}}}
            },
            "post": {
                "operationId": "createBackup", "tags": ["backups"],
                "parameters": [{"$ref": "#/components/parameters/IdempotencyKey"}],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/InstanceRequest"}}}},
                "responses": {
                    "202": {"description": "Persistent backup job accepted.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Job"}}}},
                    "409": {"$ref": "#/components/responses/Conflict"}
                }
            }
        },
        "/backups/{id}": {
            "get": {
                "operationId": "getBackup", "tags": ["backups"],
                "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string", "format": "uuid"}}],
                "responses": {"200": {"description": "Backup metadata.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Backup"}}}}}
            },
            "delete": {
                "operationId": "deleteBackup", "tags": ["backups"],
                "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string", "format": "uuid"}}],
                "responses": {"200": {"description": "Backup archive and record removed.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SuccessResponse"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        },
        "/backups/{id}/download": {
            "get": {
                "operationId": "downloadBackup", "tags": ["backups"],
                "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string", "format": "uuid"}}],
                "responses": {
                    "200": {
                        "description": "Verified backup archive.",
                        "headers": {
                            "Content-Length": {"schema": {"type": "integer", "minimum": 0}},
                            "Content-Disposition": {"schema": {"type": "string"}},
                            "Cache-Control": {"schema": {"const": "no-store"}}
                        },
                        "content": {"application/zip": {"schema": {"type": "string", "format": "binary"}}}
                    },
                    "409": {"$ref": "#/components/responses/Conflict"}
                }
            }
        },
        "/backups/{id}/restore": {
            "post": {
                "operationId": "restoreBackup", "tags": ["backups"],
                "parameters": [
                    {"name": "id", "in": "path", "required": true, "schema": {"type": "string", "format": "uuid"}},
                    {"$ref": "#/components/parameters/IdempotencyKey"}
                ],
                "responses": {
                    "202": {"description": "Persistent restore job accepted.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Job"}}}},
                    "409": {"$ref": "#/components/responses/Conflict"}
                }
            }
        },
        "/files": {
            "get": {
                "operationId": "listFiles", "tags": ["files"],
                "parameters": [
                    {"name": "instance_id", "in": "query", "required": true, "schema": {"type": "string", "format": "uuid"}},
                    {"name": "path", "in": "query", "required": false, "schema": {"type": "string", "maxLength": 1024, "default": ""}, "description": "Managed relative directory. Empty selects the instance root."}
                ],
                "responses": {"200": {"description": "Directory entries.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/FileList"}}}}}
            },
            "delete": {
                "operationId": "deleteFile", "tags": ["files"],
                "parameters": [
                    {"name": "instance_id", "in": "query", "required": true, "schema": {"type": "string", "format": "uuid"}},
                    {"name": "path", "in": "query", "required": true, "schema": {"type": "string", "minLength": 1, "maxLength": 1024}, "description": "Managed relative file or directory path."}
                ],
                "responses": {"200": {"description": "Entry removed.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SuccessResponse"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        },
        "/files/content": {
            "get": {
                "operationId": "downloadFile", "tags": ["files"],
                "parameters": [
                    {"name": "instance_id", "in": "query", "required": true, "schema": {"type": "string", "format": "uuid"}},
                    {"name": "path", "in": "query", "required": true, "schema": {"type": "string", "minLength": 1, "maxLength": 1024}}
                ],
                "responses": {"200": {"description": "Regular file bytes.", "headers": {"Content-Length": {"schema": {"type": "integer", "minimum": 0}}, "Cache-Control": {"schema": {"const": "no-store"}}}, "content": {"application/octet-stream": {"schema": {"type": "string", "format": "binary"}}}}}
            },
            "put": {
                "operationId": "uploadFile", "tags": ["files"],
                "parameters": [
                    {"name": "instance_id", "in": "query", "required": true, "schema": {"type": "string", "format": "uuid"}},
                    {"name": "path", "in": "query", "required": true, "schema": {"type": "string", "minLength": 1, "maxLength": 1024}}
                ],
                "requestBody": {"required": true, "content": {"application/octet-stream": {"schema": {"type": "string", "format": "binary", "maxLength": 1048576}}}},
                "responses": {"201": {"description": "File written atomically.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/FileWriteResponse"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        },
        "/files/text": {
            "get": {
                "operationId": "readTextFile", "tags": ["files"],
                "parameters": [
                    {"name": "instance_id", "in": "query", "required": true, "schema": {"type": "string", "format": "uuid"}},
                    {"name": "path", "in": "query", "required": true, "schema": {"type": "string", "minLength": 1, "maxLength": 1024}}
                ],
                "responses": {"200": {"description": "Bounded UTF-8 text file.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/TextReadResponse"}}}}}
            },
            "put": {
                "operationId": "writeTextFile", "tags": ["files"],
                "parameters": [
                    {"name": "instance_id", "in": "query", "required": true, "schema": {"type": "string", "format": "uuid"}},
                    {"name": "path", "in": "query", "required": true, "schema": {"type": "string", "minLength": 1, "maxLength": 1024}}
                ],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/TextWriteRequest"}}}},
                "responses": {"200": {"description": "Text file written atomically.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/FileWriteResponse"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        },
        "/files/directories": {
            "post": {
                "operationId": "createDirectory", "tags": ["files"],
                "parameters": [
                    {"name": "instance_id", "in": "query", "required": true, "schema": {"type": "string", "format": "uuid"}},
                    {"name": "path", "in": "query", "required": true, "schema": {"type": "string", "minLength": 1, "maxLength": 1024}}
                ],
                "responses": {"201": {"description": "Directory created.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SuccessResponse"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        },
        "/mods/providers": {
            "get": {
                "operationId": "getModProviderStatus", "tags": ["mods"],
                "description": "Owner only. Reports configuration booleans; provider credentials are never returned.",
                "responses": {"200": {"description": "Safe provider configuration status.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ModProviderStatus"}}}}}
            }
        },
        "/mods/providers/curseforge": {
            "put": {
                "operationId": "configureCurseForge", "tags": ["mods"],
                "description": "Owner only. Encrypts the write-only API key with the panel master key.",
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ConfigureCurseForgeRequest"}}}},
                "responses": {"200": {"description": "CurseForge is configured; the key is not returned.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ModProviderConfiguration"}}}}}
            },
            "delete": {
                "operationId": "clearCurseForge", "tags": ["mods"],
                "description": "Owner only. Removes the encrypted CurseForge API key.",
                "responses": {"200": {"description": "CurseForge is no longer configured.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ModProviderConfiguration"}}}}}
            }
        },
        "/servers/{id}/mods": {
            "get": {
                "operationId": "listServerMods", "tags": ["mods"],
                "parameters": [{"$ref": "#/components/parameters/ServerId"}],
                "responses": {"200": {"description": "Installed mods or plugins tracked by the manager.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/InstalledModList"}}}}}
            }
        },
        "/servers/{id}/mods/manual": {
            "post": {
                "operationId": "uploadServerMod", "tags": ["mods"],
                "parameters": [
                    {"$ref": "#/components/parameters/ServerId"},
                    {"name": "filename", "in": "query", "required": true, "schema": {"type": "string", "minLength": 5, "maxLength": 255, "pattern": "^[^/\\\\\\u0000-\\u001F\\u007F]+\\.[jJ][aA][rR]$"}}
                ],
                "requestBody": {"required": true, "content": {
                    "application/java-archive": {"schema": {"type": "string", "format": "binary", "maxLength": 536870912}},
                    "application/zip": {"schema": {"type": "string", "format": "binary", "maxLength": 536870912}},
                    "application/octet-stream": {"schema": {"type": "string", "format": "binary", "maxLength": 536870912}}
                }},
                "responses": {"201": {"description": "Validated JAR installed while the instance is stopped.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/InstalledMod"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        },
        "/servers/{id}/mods/provider": {
            "post": {
                "operationId": "installServerModFromProvider", "tags": ["mods"],
                "description": "Resolves a fixed Modrinth or CurseForge project/version, validates profile compatibility and dependencies, then downloads a checksummed JAR while the instance is stopped.",
                "parameters": [{"$ref": "#/components/parameters/ServerId"}],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ProviderInstallRequest"}}}},
                "responses": {"201": {"description": "Checksummed provider artifact installed.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/InstalledMod"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        },
        "/servers/{id}/mods/{mod_id}": {
            "delete": {
                "operationId": "deleteServerMod", "tags": ["mods"],
                "parameters": [
                    {"$ref": "#/components/parameters/ServerId"},
                    {"name": "mod_id", "in": "path", "required": true, "schema": {"type": "string", "format": "uuid"}}
                ],
                "responses": {"200": {"description": "Mod or plugin removed.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SuccessResponse"}}}}, "409": {"$ref": "#/components/responses/Conflict"}}
            }
        },
        "/servers/{id}/metrics": {
            "get": {
                "operationId": "getServerMetrics", "tags": ["metrics"],
                "parameters": [
                    {"$ref": "#/components/parameters/ServerId"},
                    {"name": "period", "in": "query", "required": false, "schema": {"enum": ["1h", "6h", "1d", "7d"], "default": "1d"}}
                ],
                "responses": {"200": {"description": "Chronological bounded metrics history.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/MetricsHistory"}}}}}
            }
        },
        "/schedules": {
            "get": {
                "operationId": "listSchedules", "tags": ["schedules"],
                "parameters": [{"name": "instance_id", "in": "query", "required": true, "schema": {"type": "string", "format": "uuid"}}],
                "responses": {"200": {"description": "Schedules for the selected instance.", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/Schedule"}}}}}}
            },
            "post": {
                "operationId": "createSchedule", "tags": ["schedules"],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CreateScheduleRequest"}}}},
                "responses": {"201": {"description": "Schedule created.", "headers": {"ETag": {"$ref": "#/components/headers/ETag"}}, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Schedule"}}}}}
            }
        },
        "/schedules/{id}": {
            "get": {
                "operationId": "getSchedule", "tags": ["schedules"],
                "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string", "format": "uuid"}}],
                "responses": {"200": {"description": "Schedule.", "headers": {"ETag": {"$ref": "#/components/headers/ETag"}}, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Schedule"}}}}}
            },
            "put": {
                "operationId": "updateSchedule", "tags": ["schedules"],
                "parameters": [
                    {"name": "id", "in": "path", "required": true, "schema": {"type": "string", "format": "uuid"}},
                    {"$ref": "#/components/parameters/IfMatch"}
                ],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/UpdateScheduleRequest"}}}},
                "responses": {
                    "200": {"description": "Schedule updated.", "headers": {"ETag": {"$ref": "#/components/headers/ETag"}}, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Schedule"}}}},
                    "409": {"$ref": "#/components/responses/Conflict"},
                    "428": {"$ref": "#/components/responses/PreconditionRequired"}
                }
            },
            "delete": {
                "operationId": "deleteSchedule", "tags": ["schedules"],
                "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string", "format": "uuid"}}],
                "responses": {"200": {"description": "Schedule removed.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SuccessResponse"}}}}}
            }
        },
        "/chat": {
            "get": {
                "operationId": "listChatMessages", "tags": ["chat"],
                "parameters": [
                    {"name": "before_id", "in": "query", "required": false, "schema": {"type": "string", "format": "uuid"}},
                    {"name": "limit", "in": "query", "required": false, "schema": {"type": "integer", "minimum": 1, "maximum": 100, "default": 50}}
                ],
                "responses": {"200": {"description": "Reverse-chronological chat page.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ChatPage"}}}}}
            },
            "post": {
                "operationId": "createChatMessage", "tags": ["chat"],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CreateChatMessageRequest"}}}},
                "responses": {"201": {"description": "Message persisted.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/ChatMessage"}}}}}
            }
        },
        "/chat/{id}": {
            "delete": {
                "operationId": "deleteChatMessage", "tags": ["chat"],
                "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string", "format": "uuid"}}],
                "responses": {"200": {"description": "Message contents redacted.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SuccessResponse"}}}}}
            }
        },
        "/notifications": {
            "get": {
                "operationId": "listNotifications", "tags": ["notifications"],
                "parameters": [
                    {"name": "before_id", "in": "query", "required": false, "schema": {"type": "string", "format": "uuid"}},
                    {"name": "limit", "in": "query", "required": false, "schema": {"type": "integer", "minimum": 1, "maximum": 100, "default": 50}},
                    {"name": "unread_only", "in": "query", "required": false, "schema": {"type": "boolean", "default": false}}
                ],
                "responses": {"200": {"description": "Current user's notification page.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/NotificationPage"}}}}}
            }
        },
        "/notifications/read-all": {
            "post": {
                "operationId": "readAllNotifications", "tags": ["notifications"],
                "responses": {"200": {"description": "All current-user notifications marked as read.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SuccessResponse"}}}}}
            }
        },
        "/notifications/{id}/read": {
            "put": {
                "operationId": "readNotification", "tags": ["notifications"],
                "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string", "format": "uuid"}}],
                "responses": {"200": {"description": "Notification marked as read.", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/SuccessResponse"}}}}}
            }
        },
        "/webhooks": {
            "get": {
                "operationId": "listWebhooks", "tags": ["webhooks"],
                "description": "Owner only. Secret Discord webhook URLs are never returned.",
                "responses": {"200": {"description": "Configured webhook metadata.", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/Webhook"}}}}}}
            },
            "post": {
                "operationId": "createWebhook", "tags": ["webhooks"],
                "description": "Owner only.",
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/CreateWebhookRequest"}}}},
                "responses": {
                    "201": {"description": "Webhook created; its URL remains write-only.", "headers": {"ETag": {"$ref": "#/components/headers/ETag"}}, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Webhook"}}}},
                    "409": {"$ref": "#/components/responses/Conflict"}
                }
            }
        },
        "/webhooks/{id}": {
            "put": {
                "operationId": "updateWebhook", "tags": ["webhooks"],
                "description": "Owner only. Omitting url keeps the existing encrypted value.",
                "parameters": [
                    {"name": "id", "in": "path", "required": true, "schema": {"type": "string", "format": "uuid"}},
                    {"$ref": "#/components/parameters/StrongIfMatch"}
                ],
                "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/UpdateWebhookRequest"}}}},
                "responses": {
                    "200": {"description": "Webhook updated.", "headers": {"ETag": {"$ref": "#/components/headers/ETag"}}, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Webhook"}}}},
                    "409": {"$ref": "#/components/responses/Conflict"},
                    "428": {"$ref": "#/components/responses/PreconditionRequired"}
                }
            },
            "delete": {
                "operationId": "deleteWebhook", "tags": ["webhooks"],
                "description": "Owner only.",
                "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string", "format": "uuid"}}],
                "responses": {"204": {"description": "Webhook removed."}}
            }
        }
    })
}

fn core_schemas() -> Value {
    json!({
        "Problem": {
            "type": "object",
            "additionalProperties": false,
            "required": ["type", "title", "status", "trace_id"],
            "properties": {
                "type": {"type": "string", "default": "about:blank"},
                "title": {"type": "string"},
                "status": {"type": "integer", "minimum": 400, "maximum": 599},
                "trace_id": {"type": "string", "format": "uuid"},
                "code": {"type": "string"}
            }
        },
        "SuccessResponse": {
            "type": "object", "additionalProperties": false, "required": ["success"],
            "properties": {"success": {"type": "boolean"}, "message": {"type": "string"}}
        },
        "HealthResponse": {
            "type": "object", "additionalProperties": false,
            "required": ["status", "service", "version"],
            "properties": {
                "status": {"enum": ["ok", "unavailable"]},
                "service": {"const": "dmx-server-manager"},
                "version": {"type": "string"}
            }
        },
        "SetupStatus": {
            "type": "object", "additionalProperties": false, "required": ["needs_setup"],
            "properties": {"needs_setup": {"type": "boolean"}}
        },
        "LoginRequest": {
            "type": "object", "required": ["username", "password"],
            "properties": {"username": {"type": "string"}, "password": {"type": "string", "format": "password", "writeOnly": true}}
        },
        "SetupRequest": {
            "type": "object", "required": ["username", "password"],
            "properties": {
                "username": {"type": "string", "pattern": "^[A-Za-z0-9_-]{3,32}$"},
                "password": {"type": "string", "format": "password", "writeOnly": true, "minLength": 12, "maxLength": 256}
            }
        },
        "ChangePasswordRequest": {
            "type": "object", "required": ["current_password", "new_password"],
            "properties": {
                "current_password": {"type": "string", "format": "password", "writeOnly": true},
                "new_password": {"type": "string", "format": "password", "writeOnly": true, "minLength": 12, "maxLength": 256}
            }
        },
        "UserInfo": {
            "type": "object", "additionalProperties": false,
            "required": ["id", "username", "role", "permissions", "accent_color", "must_change_password"],
            "properties": {
                "id": {"type": "string", "format": "uuid"},
                "username": {"type": "string"},
                "role": {"type": "string"},
                "permissions": {"type": "array", "items": {"type": "string"}},
                "accent_color": {"type": "string", "pattern": "^#[0-9A-Fa-f]{6}$"},
                "must_change_password": {"type": "boolean"}
            }
        },
        "AuthResponse": {
            "type": "object", "additionalProperties": false, "required": ["user", "csrf_token"],
            "properties": {"user": {"$ref": "#/components/schemas/UserInfo"}, "csrf_token": {"type": "string", "minLength": 1}}
        },
        "AuditEvent": {
            "type": "object", "additionalProperties": false,
            "required": ["id", "actor_user_id", "actor_username", "action", "resource_type", "resource_id", "outcome", "metadata", "created_at"],
            "properties": {
                "id": {"type": "integer", "minimum": 1},
                "actor_user_id": {"type": ["string", "null"]},
                "actor_username": {"type": ["string", "null"]},
                "action": {"type": "string"},
                "resource_type": {"type": "string"},
                "resource_id": {"type": ["string", "null"]},
                "outcome": {"enum": ["success", "denied", "failure"]},
                "metadata": {"type": "object", "additionalProperties": true},
                "created_at": {"type": "string", "format": "date-time"}
            }
        },
        "AuditPage": {
            "type": "object", "additionalProperties": false, "required": ["items", "next_before_id"],
            "properties": {
                "items": {"type": "array", "items": {"$ref": "#/components/schemas/AuditEvent"}},
                "next_before_id": {"type": ["integer", "null"], "minimum": 1}
            }
        }
    })
}

fn profile_schemas() -> Value {
    json!({
        "SupportedPlatform": {"enum": ["linux-x64", "windows-x64"]},
        "PortProtocol": {"enum": ["tcp", "udp"]},
        "PortSpec": {
            "type": "object", "additionalProperties": false,
            "required": ["name", "protocol", "default", "adjacent_to"],
            "properties": {
                "name": {"type": "string"},
                "protocol": {"$ref": "#/components/schemas/PortProtocol"},
                "default": {"type": "integer", "minimum": 1, "maximum": 65535},
                "adjacent_to": {"type": ["string", "null"]}
            }
        },
        "PortSpecInput": {
            "type": "object", "additionalProperties": false,
            "required": ["name", "protocol", "default"],
            "properties": {
                "name": {"type": "string", "pattern": "^[a-z][a-z0-9_]{0,31}$"},
                "protocol": {"$ref": "#/components/schemas/PortProtocol"},
                "default": {"type": "integer", "minimum": 1, "maximum": 65535},
                "adjacent_to": {"type": ["string", "null"]}
            }
        },
        "StopStrategy": {
            "oneOf": [
                {"type": "object", "additionalProperties": false, "required": ["kind", "command", "timeout_seconds"], "properties": {"kind": {"const": "stdin"}, "command": {"type": "string", "minLength": 1, "maxLength": 256}, "timeout_seconds": {"type": "integer", "minimum": 1, "maximum": 300}}},
                {"type": "object", "additionalProperties": false, "required": ["kind", "timeout_seconds"], "properties": {"kind": {"const": "interrupt"}, "timeout_seconds": {"type": "integer", "minimum": 1, "maximum": 300}}},
                {"type": "object", "additionalProperties": false, "required": ["kind", "timeout_seconds"], "properties": {"kind": {"const": "terminate"}, "timeout_seconds": {"type": "integer", "minimum": 1, "maximum": 300}}}
            ],
            "discriminator": {"propertyName": "kind"}
        },
        "LifecycleSpec": {
            "type": "object", "additionalProperties": false, "required": ["stop", "ready_log_pattern"],
            "properties": {"stop": {"$ref": "#/components/schemas/StopStrategy"}, "ready_log_pattern": {"type": ["string", "null"]}}
        },
        "SteamExecutable": {
            "type": "object", "additionalProperties": false, "required": ["linux_x86_64", "windows_x86_64"],
            "properties": {"linux_x86_64": {"type": ["string", "null"], "maxLength": 512}, "windows_x86_64": {"type": ["string", "null"], "maxLength": 512}}
        },
        "SteamExecutableInput": {
            "type": "object", "additionalProperties": false, "minProperties": 1,
            "properties": {"linux_x86_64": {"type": ["string", "null"], "maxLength": 512}, "windows_x86_64": {"type": ["string", "null"], "maxLength": 512}}
        },
        "SteamProfile": {
            "type": "object", "additionalProperties": false,
            "required": ["app_id", "branch", "executable", "arguments", "ports", "save_paths", "ready_log_pattern", "stop_strategy"],
            "properties": {
                "app_id": {"type": "integer", "minimum": 1, "maximum": 4_294_967_295_u64},
                "branch": {"type": ["string", "null"], "pattern": "^[A-Za-z0-9._-]{1,64}$"},
                "executable": {"$ref": "#/components/schemas/SteamExecutable"},
                "arguments": {"type": "array", "maxItems": 128, "items": {"type": "string", "maxLength": 8192}},
                "ports": {"type": "array", "minItems": 1, "maxItems": 16, "items": {"$ref": "#/components/schemas/PortSpec"}},
                "save_paths": {"type": "array", "minItems": 1, "maxItems": 32, "items": {"type": "string", "maxLength": 512}},
                "ready_log_pattern": {"type": ["string", "null"], "maxLength": 256},
                "stop_strategy": {"$ref": "#/components/schemas/StopStrategy"}
            }
        },
        "SteamProfileDefinition": {
            "type": "object", "additionalProperties": false,
            "required": ["name", "description", "app_id", "executable", "ports", "save_paths", "stop_strategy"],
            "properties": {
                "name": {"type": "string", "minLength": 1, "maxLength": 80},
                "description": {"type": "string", "minLength": 1, "maxLength": 500},
                "app_id": {"type": "integer", "minimum": 1, "maximum": 4_294_967_295_u64},
                "branch": {"type": ["string", "null"], "pattern": "^[A-Za-z0-9._-]{1,64}$"},
                "executable": {"$ref": "#/components/schemas/SteamExecutableInput"},
                "arguments": {"type": "array", "maxItems": 128, "items": {"type": "string", "maxLength": 8192}, "default": []},
                "ports": {"type": "array", "minItems": 1, "maxItems": 16, "items": {"$ref": "#/components/schemas/PortSpecInput"}},
                "save_paths": {"type": "array", "minItems": 1, "maxItems": 32, "uniqueItems": true, "items": {"type": "string", "maxLength": 512}},
                "ready_log_pattern": {"type": ["string", "null"], "minLength": 1, "maxLength": 256},
                "stop_strategy": {"$ref": "#/components/schemas/StopStrategy"}
            }
        },
        "CreateSteamProfileRequest": {
            "type": "object", "additionalProperties": false, "required": ["id", "definition"],
            "properties": {
                "id": {"type": "string", "minLength": 7, "maxLength": 64, "pattern": "^steam-(?!custom$)[a-z0-9]+(?:-[a-z0-9]+)*$"},
                "definition": {"$ref": "#/components/schemas/SteamProfileDefinition"}
            }
        },
        "GameProfile": {
            "type": "object", "additionalProperties": false,
            "required": ["id", "revision", "name", "description", "kind", "platforms", "capabilities", "ports", "lifecycle", "settings_schema", "ui_schema"],
            "properties": {
                "id": {"type": "string"},
                "revision": {"type": "integer", "minimum": 1},
                "name": {"type": "string"},
                "description": {"type": "string"},
                "kind": {"enum": ["builtin", "steam_custom"]},
                "platforms": {"type": "array", "items": {"$ref": "#/components/schemas/SupportedPlatform"}},
                "capabilities": {"type": "array", "items": {"type": "string"}},
                "ports": {"type": "array", "items": {"$ref": "#/components/schemas/PortSpec"}},
                "lifecycle": {"$ref": "#/components/schemas/LifecycleSpec"},
                "settings_schema": {"type": "object", "additionalProperties": true},
                "ui_schema": {"type": "object", "additionalProperties": true},
                "steam_profile": {"$ref": "#/components/schemas/SteamProfile"}
            }
        },
        "ProfileVersionCatalog": {
            "type": "object", "additionalProperties": false,
            "required": ["profile_id", "game_versions", "selected_game_version", "loader_versions"],
            "properties": {
                "profile_id": {"type": "string", "minLength": 1, "maxLength": 64},
                "game_versions": {"type": "array", "maxItems": 512, "items": {"type": "string", "minLength": 1, "maxLength": 96, "pattern": "^[A-Za-z0-9._+-]+$"}},
                "selected_game_version": {"type": ["string", "null"], "minLength": 1, "maxLength": 96, "pattern": "^[A-Za-z0-9._+-]+$"},
                "loader_versions": {"type": "array", "maxItems": 512, "items": {"type": "string", "minLength": 1, "maxLength": 96, "pattern": "^[A-Za-z0-9._+-]+$"}}
            }
        }
    })
}

fn catalog_schemas() -> Value {
    json!({
        "CatalogFileDeclaration": {
            "type": "object", "additionalProperties": false,
            "required": ["path", "sha256", "size_bytes", "media_type"],
            "properties": {
                "path": {"type": "string", "minLength": 1, "maxLength": 256, "pattern": "^[a-z0-9._/-]+$"},
                "sha256": {"type": "string", "pattern": "^[0-9a-f]{64}$"},
                "size_bytes": {"type": "integer", "minimum": 1, "maximum": 4194304},
                "media_type": {"enum": ["application/json", "image/png"]}
            }
        },
        "CatalogContent": {
            "oneOf": [
                {
                    "type": "object", "additionalProperties": false,
                    "required": ["kind", "definition", "settings_schema", "ui_schema", "icon"],
                    "properties": {
                        "kind": {"const": "steam_profile"},
                        "definition": {"type": "string", "maxLength": 256},
                        "settings_schema": {"type": "string", "maxLength": 256},
                        "ui_schema": {"type": "string", "maxLength": 256},
                        "icon": {"type": ["string", "null"], "maxLength": 256}
                    }
                },
                {
                    "type": "object", "additionalProperties": false,
                    "required": ["kind", "tokens", "logo", "preview"],
                    "properties": {
                        "kind": {"const": "theme"},
                        "tokens": {"type": "string", "maxLength": 256},
                        "logo": {"type": ["string", "null"], "maxLength": 256},
                        "preview": {"type": ["string", "null"], "maxLength": 256}
                    }
                }
            ],
            "discriminator": {"propertyName": "kind"}
        },
        "CatalogManifest": {
            "type": "object", "additionalProperties": false,
            "required": ["format", "schema_version", "id", "revision", "name", "description", "content", "files"],
            "properties": {
                "format": {"const": "dmxpack"},
                "schema_version": {"const": 1},
                "id": {"type": "string", "minLength": 7, "maxLength": 64, "pattern": "^(?:steam|theme)-[a-z0-9]+(?:-[a-z0-9]+)*$"},
                "revision": {"type": "integer", "minimum": 1},
                "name": {"type": "string", "minLength": 1, "maxLength": 80},
                "description": {"type": "string", "minLength": 1, "maxLength": 500},
                "content": {"$ref": "#/components/schemas/CatalogContent"},
                "files": {"type": "array", "minItems": 1, "maxItems": 63, "items": {"$ref": "#/components/schemas/CatalogFileDeclaration"}, "description": "Strictly sorted by path; every extracted non-manifest file must be declared exactly once."}
            }
        },
        "ThemeTokens": {
            "type": "object", "additionalProperties": false,
            "required": ["accent", "bg_primary", "bg_secondary", "bg_tertiary", "bg_elevated", "border", "border_hover", "text_primary", "text_secondary", "text_muted", "success", "warning", "danger", "info"],
            "properties": {
                "accent": {"$ref": "#/components/schemas/ThemeColor"},
                "bg_primary": {"$ref": "#/components/schemas/ThemeColor"},
                "bg_secondary": {"$ref": "#/components/schemas/ThemeColor"},
                "bg_tertiary": {"$ref": "#/components/schemas/ThemeColor"},
                "bg_elevated": {"$ref": "#/components/schemas/ThemeColor"},
                "border": {"$ref": "#/components/schemas/ThemeColor"},
                "border_hover": {"$ref": "#/components/schemas/ThemeColor"},
                "text_primary": {"$ref": "#/components/schemas/ThemeColor"},
                "text_secondary": {"$ref": "#/components/schemas/ThemeColor"},
                "text_muted": {"$ref": "#/components/schemas/ThemeColor"},
                "success": {"$ref": "#/components/schemas/ThemeColor"},
                "warning": {"$ref": "#/components/schemas/ThemeColor"},
                "danger": {"$ref": "#/components/schemas/ThemeColor"},
                "info": {"$ref": "#/components/schemas/ThemeColor"}
            }
        },
        "ThemeSelection": {
            "oneOf": [
                {"type": "object", "additionalProperties": false, "required": ["kind"], "properties": {"kind": {"const": "default"}}},
                {
                    "type": "object", "additionalProperties": false,
                    "required": ["kind", "package_id", "revision"],
                    "properties": {
                        "kind": {"const": "catalog"},
                        "package_id": {"type": "string", "minLength": 7, "maxLength": 64, "pattern": "^theme-[a-z0-9]+(?:-[a-z0-9]+)*$"},
                        "revision": {"type": "integer", "minimum": 1}
                    }
                }
            ],
            "discriminator": {"propertyName": "kind"}
        },
        "ThemeAsset": {
            "type": "object", "additionalProperties": false,
            "required": ["url", "sha256", "media_type", "size_bytes"],
            "properties": {
                "url": {"type": "string", "pattern": "^/api/v1/catalog/theme/theme-[a-z0-9]+(?:-[a-z0-9]+)*/revisions/[1-9][0-9]*/assets/(?:logo|preview)$"},
                "sha256": {"type": "string", "pattern": "^[0-9a-f]{64}$"},
                "media_type": {"const": "image/png"},
                "size_bytes": {"type": "integer", "minimum": 1, "maximum": 2097152}
            }
        },
        "ThemeAssets": {
            "type": "object", "additionalProperties": false, "required": ["logo", "preview"],
            "properties": {
                "logo": {"oneOf": [{"$ref": "#/components/schemas/ThemeAsset"}, {"type": "null"}]},
                "preview": {"oneOf": [{"$ref": "#/components/schemas/ThemeAsset"}, {"type": "null"}]}
            }
        },
        "ActiveTheme": {
            "type": "object", "additionalProperties": false,
            "required": ["selection", "tokens", "assets", "version", "updated_at"],
            "properties": {
                "selection": {"$ref": "#/components/schemas/ThemeSelection"},
                "tokens": {"$ref": "#/components/schemas/ThemeTokens"},
                "assets": {"$ref": "#/components/schemas/ThemeAssets"},
                "version": {"type": "integer", "minimum": 1},
                "updated_at": {"type": "string", "format": "date-time"}
            }
        },
        "ThemeColor": {"type": "string", "pattern": "^#[0-9A-Fa-f]{6}$"},
        "CatalogFile": {
            "type": "object", "additionalProperties": false,
            "required": ["role", "path", "media_type", "sha256", "size_bytes"],
            "properties": {
                "role": {"enum": ["definition", "settings_schema", "ui_schema", "tokens", "icon", "logo", "preview"]},
                "path": {"type": "string", "maxLength": 256},
                "media_type": {"enum": ["application/json", "image/png"]},
                "sha256": {"type": "string", "pattern": "^[0-9a-f]{64}$"},
                "size_bytes": {"type": "integer", "minimum": 1, "maximum": 4194304}
            }
        },
        "CatalogPackage": {
            "type": "object", "additionalProperties": false,
            "required": ["id", "revision", "kind", "schema_version", "name", "description", "archive_sha256", "archive_size_bytes", "content_size_bytes", "manifest", "files", "theme_tokens", "compatibility_status", "created_at"],
            "properties": {
                "id": {"type": "string"},
                "revision": {"type": "integer", "minimum": 1},
                "kind": {"enum": ["steam_profile", "theme"]},
                "schema_version": {"const": 1},
                "name": {"type": "string"},
                "description": {"type": "string"},
                "archive_sha256": {"type": "string", "pattern": "^[0-9a-f]{64}$"},
                "archive_size_bytes": {"type": "integer", "minimum": 1, "maximum": 16777216},
                "content_size_bytes": {"type": "integer", "minimum": 1, "maximum": 33554432},
                "manifest": {"$ref": "#/components/schemas/CatalogManifest"},
                "files": {"type": "array", "maxItems": 63, "items": {"$ref": "#/components/schemas/CatalogFile"}},
                "theme_tokens": {"oneOf": [{"$ref": "#/components/schemas/ThemeTokens"}, {"type": "null"}]},
                "compatibility_status": {"const": "unverified", "description": "Generic anonymous SteamCMD support remains best effort until an OS/architecture smoke test succeeds."},
                "created_at": {"type": "string", "format": "date-time"}
            }
        }
    })
}

fn release_schemas() -> Value {
    json!({
        "ReleaseCheckState": {"enum": ["disabled", "never_checked", "checking", "up_to_date", "update_available", "check_failed"]},
        "ReleaseCheckErrorCode": {"enum": ["network", "response_too_large", "envelope_invalid", "signature_invalid", "manifest_invalid"]},
        "NativeReleaseTarget": {
            "type": "object", "additionalProperties": false,
            "required": ["kind", "platform", "archive_url", "archive_sha256", "installer_url", "installer_sha256", "upgrade_command"],
            "properties": {
                "kind": {"const": "native"}, "platform": {"enum": ["linux-amd64", "windows-amd64"]},
                "archive_url": {"type": "string", "format": "uri", "maxLength": 4096, "description": "Signed official HTTPS release URL."},
                "archive_sha256": {"type": "string", "pattern": "^[0-9a-f]{64}$"},
                "installer_url": {"type": "string", "format": "uri", "maxLength": 4096, "description": "Signed official HTTPS installer URL."},
                "installer_sha256": {"type": "string", "pattern": "^[0-9a-f]{64}$"},
                "upgrade_command": {"type": "string", "minLength": 1, "maxLength": 8192, "description": "Locally constructed, display-only command pinning both signed checksums. The panel never executes it."}
            }
        },
        "DockerReleaseTarget": {
            "type": "object", "additionalProperties": false,
            "required": ["kind", "image", "digest", "pull_command", "apply_command"],
            "properties": {
                "kind": {"const": "docker"}, "image": {"const": "ghcr.io/thefrcrazy/dmx-server-manager"},
                "digest": {"type": "string", "pattern": "^sha256:[0-9a-f]{64}$"},
                "pull_command": {"type": "string", "minLength": 1, "maxLength": 8192, "description": "Display-only docker pull command pinned by the signed image digest."},
                "apply_command": {"type": "string", "minLength": 1, "maxLength": 8192, "description": "Display-only Compose recreate command pinned by the signed image digest."}
            }
        },
        "ReleaseTarget": {
            "oneOf": [
                {"$ref": "#/components/schemas/NativeReleaseTarget"},
                {"$ref": "#/components/schemas/DockerReleaseTarget"}
            ],
            "discriminator": {
                "propertyName": "kind",
                "mapping": {
                    "native": "#/components/schemas/NativeReleaseTarget",
                    "docker": "#/components/schemas/DockerReleaseTarget"
                }
            }
        },
        "VerifiedPanelRelease": {
            "type": "object", "additionalProperties": false,
            "required": ["version", "published_at", "notes_url", "target"],
            "properties": {
                "version": {"type": "string", "minLength": 1, "maxLength": 64},
                "published_at": {"type": "string", "format": "date-time"},
                "notes_url": {"type": "string", "format": "uri", "maxLength": 4096},
                "target": {"$ref": "#/components/schemas/ReleaseTarget"}
            }
        },
        "ReleaseStatus": {
            "type": "object", "additionalProperties": false,
            "required": ["configured", "current_version", "deployment_mode", "state", "checked_at", "latest", "error_code"],
            "properties": {
                "configured": {"type": "boolean"}, "current_version": {"type": "string", "minLength": 1, "maxLength": 64},
                "deployment_mode": {"enum": ["native", "docker"]}, "state": {"$ref": "#/components/schemas/ReleaseCheckState"},
                "checked_at": {"type": ["string", "null"], "format": "date-time"},
                "latest": {"oneOf": [{"$ref": "#/components/schemas/VerifiedPanelRelease"}, {"type": "null"}]},
                "error_code": {"oneOf": [{"$ref": "#/components/schemas/ReleaseCheckErrorCode"}, {"type": "null"}]}
            }
        }
    })
}

fn administration_schemas() -> Value {
    json!({
        "PermissionId": {"enum": [
            "audit.read", "chat.read", "chat.write", "job.read", "mods.manage",
            "notifications.read", "profile.manage", "profile.read", "schedule.manage",
            "server.backup", "server.backup.read", "server.console.read",
            "server.console.write", "server.create", "server.delete", "server.files.read",
            "server.files.write", "server.kill", "server.read", "server.start", "server.stop",
            "server.update", "server.update_game", "user.create", "user.read", "user.update"
        ]},
        "InstancePermissionId": {"enum": [
            "job.read", "mods.manage", "schedule.manage", "server.backup", "server.backup.read",
            "server.console.read", "server.console.write", "server.files.read", "server.files.write",
            "server.kill", "server.read", "server.start", "server.stop", "server.update",
            "server.update_game"
        ]},
        "Permission": {
            "type": "object", "additionalProperties": false, "required": ["id", "high_risk", "instance_scoped"],
            "properties": {"id": {"$ref": "#/components/schemas/PermissionId"}, "high_risk": {"type": "boolean"}, "instance_scoped": {"type": "boolean"}}
        },
        "Role": {
            "type": "object", "additionalProperties": false,
            "required": ["id", "name", "permissions", "is_system", "created_at", "updated_at"],
            "properties": {
                "id": {"type": "string"}, "name": {"type": "string"},
                "permissions": {"type": "array", "items": {"oneOf": [{"$ref": "#/components/schemas/PermissionId"}, {"const": "*"}]}},
                "is_system": {"type": "boolean"},
                "created_at": {"type": "string", "format": "date-time"}, "updated_at": {"type": "string", "format": "date-time"}
            }
        },
        "CreateRoleRequest": {
            "type": "object", "additionalProperties": false, "required": ["name", "permissions"],
            "properties": {"name": {"type": "string", "minLength": 1, "maxLength": 64}, "permissions": {"type": "array", "uniqueItems": true, "maxItems": 26, "items": {"$ref": "#/components/schemas/PermissionId"}}}
        },
        "UpdateRoleRequest": {
            "type": "object", "additionalProperties": false, "minProperties": 1,
            "properties": {"name": {"type": ["string", "null"], "minLength": 1, "maxLength": 64}, "permissions": {"type": ["array", "null"], "uniqueItems": true, "maxItems": 26, "items": {"$ref": "#/components/schemas/PermissionId"}}}
        },
        "ManagedUser": {
            "type": "object", "additionalProperties": false,
            "required": ["id", "username", "role_id", "role_name", "is_active", "language", "accent_color", "must_change_password", "last_login_at", "created_at", "updated_at"],
            "properties": {
                "id": {"type": "string", "format": "uuid"}, "username": {"type": "string"},
                "role_id": {"type": "string"}, "role_name": {"type": "string"}, "is_active": {"type": "boolean"},
                "language": {"enum": ["fr", "en"]}, "accent_color": {"type": "string", "pattern": "^#[0-9A-Fa-f]{6}$"},
                "must_change_password": {"type": "boolean"}, "last_login_at": {"type": ["string", "null"], "format": "date-time"},
                "created_at": {"type": "string", "format": "date-time"}, "updated_at": {"type": "string", "format": "date-time"}
            }
        },
        "CreateUserRequest": {
            "type": "object", "additionalProperties": false, "required": ["username", "password", "role_id"],
            "properties": {
                "username": {"type": "string", "pattern": "^[A-Za-z0-9_-]{3,32}$"},
                "password": {"type": "string", "format": "password", "writeOnly": true, "minLength": 12, "maxLength": 256},
                "role_id": {"type": "string"}, "language": {"enum": ["fr", "en"], "default": "fr"}
            }
        },
        "UpdateUserRequest": {
            "type": "object", "additionalProperties": false, "minProperties": 1,
            "properties": {
                "role_id": {"type": ["string", "null"]}, "is_active": {"type": ["boolean", "null"]},
                "language": {"oneOf": [{"enum": ["fr", "en"]}, {"type": "null"}]},
                "accent_color": {"type": ["string", "null"], "pattern": "^#[0-9A-Fa-f]{6}$"},
                "password": {"type": ["string", "null"], "format": "password", "writeOnly": true, "minLength": 12, "maxLength": 256}
            }
        },
        "InstanceGrant": {
            "type": "object", "additionalProperties": false, "required": ["instance_id", "instance_name", "permissions", "created_at"],
            "properties": {
                "instance_id": {"type": "string", "format": "uuid"}, "instance_name": {"type": "string"},
                "permissions": {"type": "array", "items": {"$ref": "#/components/schemas/InstancePermissionId"}},
                "created_at": {"type": "string", "format": "date-time"}
            }
        },
        "SetGrantRequest": {
            "type": "object", "additionalProperties": false,
            "properties": {"permissions": {"type": "array", "uniqueItems": true, "maxItems": 15, "default": [], "items": {"$ref": "#/components/schemas/InstancePermissionId"}}}
        }
    })
}

fn server_schemas() -> Value {
    json!({
        "CreateInstanceRequest": {
            "type": "object", "additionalProperties": false, "required": ["name", "profile_id"],
            "properties": {
                "name": {"type": "string", "minLength": 1, "maxLength": 80},
                "profile_id": {"type": "string", "minLength": 1},
                "settings": {"type": "object", "additionalProperties": true, "default": {}},
                "secrets": {"type": "object", "default": {}, "additionalProperties": {"type": "string", "writeOnly": true, "maxLength": 16384}},
                "auto_start": {"type": "boolean", "default": false},
                "watchdog_enabled": {"type": "boolean", "default": true}
            }
        },
        "UpdateInstanceRequest": {
            "type": "object", "additionalProperties": false,
            "properties": {
                "name": {"type": ["string", "null"], "minLength": 1, "maxLength": 80},
                "settings": {"oneOf": [{"type": "object", "additionalProperties": true}, {"type": "null"}]},
                "auto_start": {"type": ["boolean", "null"]}, "watchdog_enabled": {"type": ["boolean", "null"]}
            }
        },
        "SetProfileRevisionRequest": {
            "type": "object", "additionalProperties": false, "required": ["revision"],
            "properties": {"revision": {"type": "integer", "minimum": 1, "maximum": 4_294_967_295_u64}}
        },
        "SetSecretRequest": {
            "type": "object", "additionalProperties": false, "required": ["value"],
            "properties": {"value": {"type": "string", "writeOnly": true, "minLength": 1, "maxLength": 16384}}
        },
        "SecretStatus": {
            "type": "object", "additionalProperties": false, "required": ["name", "configured"],
            "properties": {"name": {"type": "string"}, "configured": {"type": "boolean"}}
        },
        "SecretStatusList": {
            "type": "object", "additionalProperties": false, "required": ["items"],
            "properties": {"items": {"type": "array", "items": {"$ref": "#/components/schemas/SecretStatus"}}}
        },
        "Instance": {
            "type": "object", "additionalProperties": false,
            "required": ["id", "name", "profile_id", "profile_revision", "settings", "config_version", "installation_state", "installed_version", "installed_build", "desired_state", "runtime_state", "managed", "auto_start", "watchdog_enabled", "created_at", "updated_at"],
            "properties": {
                "id": {"type": "string", "format": "uuid"}, "name": {"type": "string"}, "profile_id": {"type": "string"},
                "profile_revision": {"type": "integer", "minimum": 1}, "settings": {"type": "object", "additionalProperties": true},
                "config_version": {"type": "integer", "minimum": 1},
                "installation_state": {"enum": ["not_installed", "installing", "installed", "updating", "failed"]},
                "installed_version": {"type": ["string", "null"]}, "installed_build": {"type": ["string", "null"]},
                "desired_state": {"enum": ["running", "stopped"]},
                "runtime_state": {"enum": ["stopped", "starting", "running", "stopping", "crashed", "unknown"]},
                "managed": {"type": "boolean"}, "auto_start": {"type": "boolean"}, "watchdog_enabled": {"type": "boolean"},
                "created_at": {"type": "string", "format": "date-time"}, "updated_at": {"type": "string", "format": "date-time"}
            }
        },
        "OauthDeviceJobInteraction": {
            "type": "object", "additionalProperties": false,
            "required": ["kind", "verification_uri", "user_code"],
            "properties": {
                "kind": {"const": "oauth_device"},
                "verification_uri": {"type": "string", "format": "uri", "maxLength": 4096, "description": "Validated HTTPS Hytale game-server or downloader device verification URL."},
                "user_code": {"type": ["string", "null"], "minLength": 4, "maxLength": 32, "pattern": "^[A-Za-z0-9-]+$"}
            }
        },
        "BedrockArchiveUploadJobInteraction": {
            "type": "object", "additionalProperties": false,
            "required": ["kind", "instance_id", "version", "method", "path", "required_sha256_header", "max_bytes"],
            "properties": {
                "kind": {"const": "bedrock_archive_upload"}, "instance_id": {"type": "string", "format": "uuid"},
                "version": {"type": ["string", "null"], "minLength": 1, "maxLength": 128},
                "method": {"const": "POST"},
                "path": {"type": "string", "pattern": "^/api/v1/servers/[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[1-8][0-9A-Fa-f]{3}-[89AaBb][0-9A-Fa-f]{3}-[0-9A-Fa-f]{12}/imports/zip$"},
                "required_sha256_header": {"const": "x-dmx-archive-sha256"},
                "max_bytes": {"const": 4_294_967_296_u64}
            }
        },
        "JobInteraction": {
            "oneOf": [
                {"$ref": "#/components/schemas/OauthDeviceJobInteraction"},
                {"$ref": "#/components/schemas/BedrockArchiveUploadJobInteraction"}
            ],
            "discriminator": {
                "propertyName": "kind",
                "mapping": {
                    "oauth_device": "#/components/schemas/OauthDeviceJobInteraction",
                    "bedrock_archive_upload": "#/components/schemas/BedrockArchiveUploadJobInteraction"
                }
            }
        },
        "Job": {
            "type": "object", "additionalProperties": false,
            "required": ["id", "instance_id", "kind", "state", "progress", "requested_by", "error_code", "error_message", "created_at", "started_at", "finished_at", "interaction"],
            "properties": {
                "id": {"type": "string", "format": "uuid"}, "instance_id": {"type": ["string", "null"], "format": "uuid"},
                "kind": {"type": "string"}, "state": {"enum": ["queued", "running", "waiting_for_user", "succeeded", "failed", "cancelled", "interrupted"]},
                "progress": {"type": "integer", "minimum": 0, "maximum": 100}, "requested_by": {"type": "string"},
                "error_code": {"type": ["string", "null"]}, "error_message": {"type": ["string", "null"]},
                "created_at": {"type": "string", "format": "date-time"}, "started_at": {"type": ["string", "null"], "format": "date-time"},
                "finished_at": {"type": ["string", "null"], "format": "date-time"},
                "interaction": {"oneOf": [{"$ref": "#/components/schemas/JobInteraction"}, {"type": "null"}]}
            }
        },
        "ConsoleRequest": {
            "type": "object", "additionalProperties": false, "required": ["command"],
            "properties": {"command": {"type": "string", "minLength": 1, "maxLength": 4096, "pattern": "^[^\\u0000\\r\\n]+$"}}
        },
        "ConsoleResponse": {
            "type": "object", "additionalProperties": false, "required": ["accepted"],
            "properties": {"accepted": {"type": "boolean"}}
        },
        "LogHistoryLine": {
            "type": "object", "additionalProperties": false, "required": ["stream", "message"],
            "properties": {
                "stream": {"enum": ["install", "install_error", "console", "console_error"]},
                "message": {"type": "string"}
            }
        },
        "LogHistoryResponse": {
            "type": "object", "additionalProperties": false, "required": ["source", "items"],
            "properties": {
                "source": {"enum": ["install", "console"]},
                "items": {"type": "array", "maxItems": 1000, "items": {"$ref": "#/components/schemas/LogHistoryLine"}}
            }
        },
        "ImportSourceRequest": {
            "type": "object", "additionalProperties": false, "required": ["source_path"],
            "properties": {"source_path": {"type": "string", "minLength": 1, "description": "Path inside a root declared by DMX_IMPORT_ROOTS."}}
        }
    })
}

fn operations_schemas() -> Value {
    json!({
        "EventEnvelope": {
            "type": "object", "additionalProperties": false,
            "required": ["type", "server_id", "payload", "created_at"],
            "properties": {
                "type": {"type": "string"},
                "server_id": {"type": ["string", "null"], "format": "uuid"},
                "payload": {},
                "created_at": {"type": "string", "format": "date-time"}
            }
        },
        "InstanceRequest": {
            "type": "object", "additionalProperties": false, "required": ["instance_id"],
            "properties": {"instance_id": {"type": "string", "format": "uuid"}}
        },
        "Backup": {
            "type": "object", "additionalProperties": false,
            "required": ["id", "instance_id", "kind", "status", "checksum_sha256", "size_bytes", "created_at", "completed_at"],
            "properties": {
                "id": {"type": "string", "format": "uuid"},
                "instance_id": {"type": "string", "format": "uuid"},
                "kind": {"enum": ["manual", "scheduled", "pre_restore", "pre_update"]},
                "status": {"enum": ["creating", "ready", "failed"]},
                "checksum_sha256": {"type": ["string", "null"], "pattern": "^[0-9a-f]{64}$"},
                "size_bytes": {"type": ["integer", "null"], "minimum": 0},
                "created_at": {"type": "string", "format": "date-time"},
                "completed_at": {"type": ["string", "null"], "format": "date-time"}
            }
        },
        "ManagedEntry": {
            "type": "object", "additionalProperties": false,
            "required": ["name", "path", "kind", "size_bytes", "modified_at"],
            "properties": {
                "name": {"type": "string"}, "path": {"type": "string", "maxLength": 1024},
                "kind": {"enum": ["file", "directory"]}, "size_bytes": {"type": "integer", "minimum": 0},
                "modified_at": {"type": ["string", "null"], "format": "date-time"}
            }
        },
        "FileList": {
            "type": "object", "additionalProperties": false, "required": ["items"],
            "properties": {"items": {"type": "array", "maxItems": 10000, "items": {"$ref": "#/components/schemas/ManagedEntry"}}}
        },
        "TextReadResponse": {
            "type": "object", "additionalProperties": false, "required": ["content"],
            "properties": {"content": {"type": "string", "description": "UTF-8 text, limited to 524288 encoded bytes."}}
        },
        "TextWriteRequest": {
            "type": "object", "additionalProperties": false, "required": ["content"],
            "properties": {"content": {"type": "string", "maxLength": 524288, "description": "No NUL or control characters except LF, CR and TAB; the encoded body is also limited to 524288 bytes."}}
        },
        "FileWriteResponse": {
            "type": "object", "additionalProperties": false, "required": ["bytes_written"],
            "properties": {"bytes_written": {"type": "integer", "minimum": 0}}
        },
        "InstalledMod": {
            "type": "object", "additionalProperties": false,
            "required": ["id", "instance_id", "source", "display_name", "checksum_sha256", "size_bytes", "provider_project_id", "provider_version_id", "enabled", "created_at"],
            "properties": {
                "id": {"type": "string", "format": "uuid"}, "instance_id": {"type": "string", "format": "uuid"},
                "source": {"enum": ["manual", "modrinth", "curseforge"]}, "display_name": {"type": "string"},
                "checksum_sha256": {"type": "string", "pattern": "^[0-9a-f]{64}$"},
                "size_bytes": {"type": "integer", "minimum": 1, "maximum": 536870912},
                "provider_project_id": {"type": ["string", "null"]}, "provider_version_id": {"type": ["string", "null"]},
                "enabled": {"type": "boolean"}, "created_at": {"type": "string", "format": "date-time"}
            }
        },
        "InstalledModList": {
            "type": "object", "additionalProperties": false, "required": ["items"],
            "properties": {"items": {"type": "array", "items": {"$ref": "#/components/schemas/InstalledMod"}}}
        },
        "ModProviderConfiguration": {
            "type": "object", "additionalProperties": false, "required": ["configured"],
            "properties": {"configured": {"type": "boolean"}}
        },
        "ModProviderStatus": {
            "type": "object", "additionalProperties": false, "required": ["modrinth", "curseforge"],
            "properties": {
                "modrinth": {"$ref": "#/components/schemas/ModProviderConfiguration"},
                "curseforge": {"$ref": "#/components/schemas/ModProviderConfiguration"}
            }
        },
        "ConfigureCurseForgeRequest": {
            "type": "object", "additionalProperties": false, "required": ["api_key"],
            "properties": {"api_key": {"type": "string", "format": "password", "writeOnly": true, "minLength": 16, "maxLength": 512, "description": "Printable ASCII without whitespace, quotes or backslashes."}}
        },
        "ProviderInstallRequest": {
            "oneOf": [
                {"type": "object", "additionalProperties": false, "required": ["provider", "project_id", "version_id"], "properties": {
                    "provider": {"const": "modrinth"}, "project_id": {"type": "string", "pattern": "^[A-Za-z0-9]{1,64}$"}, "version_id": {"type": "string", "pattern": "^[A-Za-z0-9]{1,64}$"}
                }},
                {"type": "object", "additionalProperties": false, "required": ["provider", "project_id", "version_id"], "properties": {
                    "provider": {"const": "curseforge"}, "project_id": {"type": "string", "pattern": "^[1-9][0-9]{0,9}$"}, "version_id": {"type": "string", "pattern": "^[1-9][0-9]{0,9}$"}
                }}
            ],
            "discriminator": {"propertyName": "provider"}
        },
        "MetricPoint": {
            "type": "object", "additionalProperties": false,
            "required": ["id", "cpu_usage", "memory_bytes", "disk_bytes", "uptime_seconds", "recorded_at"],
            "properties": {
                "id": {"type": "string", "format": "uuid"}, "cpu_usage": {"type": "number", "minimum": 0},
                "memory_bytes": {"type": "integer", "minimum": 0}, "disk_bytes": {"type": "integer", "minimum": 0},
                "uptime_seconds": {"type": "integer", "minimum": 0},
                "recorded_at": {"type": "string", "format": "date-time"}
            }
        },
        "MetricsHistory": {
            "type": "object", "additionalProperties": false, "required": ["server_id", "period", "points"],
            "properties": {
                "server_id": {"type": "string", "format": "uuid"}, "period": {"enum": ["1h", "6h", "1d", "7d"]},
                "points": {"type": "array", "maxItems": 10000, "items": {"$ref": "#/components/schemas/MetricPoint"}}
            }
        },
        "ScheduleTrigger": {
            "oneOf": [
                {"type": "object", "additionalProperties": false, "required": ["kind", "expression", "timezone"], "properties": {
                    "kind": {"const": "cron"}, "expression": {"type": "string", "minLength": 11, "maxLength": 454, "description": "Six- or seven-field cron expression; each field is limited to 64 characters."},
                    "timezone": {"type": "string", "minLength": 1, "description": "IANA time-zone identifier."}
                }},
                {"type": "object", "additionalProperties": false, "required": ["kind", "seconds"], "properties": {
                    "kind": {"const": "interval"}, "seconds": {"type": "integer", "minimum": 60, "maximum": 31536000}
                }}
            ],
            "discriminator": {"propertyName": "kind"}
        },
        "ScheduleAction": {
            "oneOf": [
                {"type": "object", "additionalProperties": false, "required": ["kind"], "properties": {"kind": {"const": "start"}}},
                {"type": "object", "additionalProperties": false, "required": ["kind"], "properties": {"kind": {"const": "stop"}}},
                {"type": "object", "additionalProperties": false, "required": ["kind"], "properties": {"kind": {"const": "restart"}}},
                {"type": "object", "additionalProperties": false, "required": ["kind"], "properties": {"kind": {"const": "backup"}}},
                {"type": "object", "additionalProperties": false, "required": ["kind"], "properties": {"kind": {"const": "update"}}},
                {"type": "object", "additionalProperties": false, "required": ["kind", "command"], "properties": {"kind": {"const": "console"}, "command": {"type": "string", "minLength": 1, "maxLength": 4096, "pattern": "^[^\\u0000\\r\\n]+$"}}}
            ],
            "discriminator": {"propertyName": "kind"}
        },
        "CreateScheduleRequest": {
            "type": "object", "additionalProperties": false, "required": ["instance_id", "name", "trigger", "action"],
            "properties": {
                "instance_id": {"type": "string", "format": "uuid"}, "name": {"type": "string", "minLength": 1, "maxLength": 80},
                "trigger": {"$ref": "#/components/schemas/ScheduleTrigger"}, "action": {"$ref": "#/components/schemas/ScheduleAction"},
                "enabled": {"type": "boolean", "default": true}
            }
        },
        "UpdateScheduleRequest": {
            "type": "object", "additionalProperties": false, "required": ["name", "trigger", "action", "enabled"],
            "properties": {
                "name": {"type": "string", "minLength": 1, "maxLength": 80},
                "trigger": {"$ref": "#/components/schemas/ScheduleTrigger"}, "action": {"$ref": "#/components/schemas/ScheduleAction"},
                "enabled": {"type": "boolean"}
            }
        },
        "Schedule": {
            "type": "object", "additionalProperties": false,
            "required": ["id", "instance_id", "name", "trigger", "action", "enabled", "next_run_at", "last_run_at", "last_job_id", "version", "created_by", "requested_by", "created_at", "updated_at"],
            "properties": {
                "id": {"type": "string", "format": "uuid"}, "instance_id": {"type": "string", "format": "uuid"}, "name": {"type": "string"},
                "trigger": {"$ref": "#/components/schemas/ScheduleTrigger"}, "action": {"$ref": "#/components/schemas/ScheduleAction"}, "enabled": {"type": "boolean"},
                "next_run_at": {"type": ["string", "null"], "format": "date-time"}, "last_run_at": {"type": ["string", "null"], "format": "date-time"},
                "last_job_id": {"type": ["string", "null"], "format": "uuid"}, "version": {"type": "integer", "minimum": 1},
                "created_by": {"type": "string"}, "requested_by": {"type": "string"},
                "created_at": {"type": "string", "format": "date-time"}, "updated_at": {"type": "string", "format": "date-time"}
            }
        },
        "CreateChatMessageRequest": {
            "type": "object", "additionalProperties": false, "required": ["body"],
            "properties": {"body": {"type": "string", "minLength": 1, "maxLength": 4000, "description": "Plain text; limited to 16384 encoded bytes, with LF and TAB as the only accepted control characters."}}
        },
        "ChatMessage": {
            "type": "object", "additionalProperties": false,
            "required": ["id", "author_user_id", "author_username", "body", "created_at", "deleted_at"],
            "properties": {
                "id": {"type": "string", "format": "uuid"}, "author_user_id": {"type": ["string", "null"], "format": "uuid"},
                "author_username": {"type": ["string", "null"]}, "body": {"type": ["string", "null"]},
                "created_at": {"type": "string", "format": "date-time"}, "deleted_at": {"type": ["string", "null"], "format": "date-time"}
            }
        },
        "ChatPage": {
            "type": "object", "additionalProperties": false, "required": ["items", "next_before_id"],
            "properties": {
                "items": {"type": "array", "maxItems": 100, "items": {"$ref": "#/components/schemas/ChatMessage"}},
                "next_before_id": {"type": ["string", "null"], "format": "uuid"}
            }
        },
        "Notification": {
            "type": "object", "additionalProperties": false,
            "required": ["id", "kind", "message_key", "data", "read_at", "created_at"],
            "properties": {
                "id": {"type": "string", "format": "uuid"}, "kind": {"type": "string", "minLength": 1, "maxLength": 64},
                "message_key": {"type": "string", "minLength": 1, "maxLength": 128}, "data": {"type": "object", "additionalProperties": true},
                "read_at": {"type": ["string", "null"], "format": "date-time"}, "created_at": {"type": "string", "format": "date-time"}
            }
        },
        "NotificationPage": {
            "type": "object", "additionalProperties": false, "required": ["items", "next_before_id", "unread_count"],
            "properties": {
                "items": {"type": "array", "maxItems": 100, "items": {"$ref": "#/components/schemas/Notification"}},
                "next_before_id": {"type": ["string", "null"], "format": "uuid"}, "unread_count": {"type": "integer", "minimum": 0}
            }
        },
        "WebhookEvent": {"enum": [
            "backup.created", "backup.restored", "job.failed", "server.crashed", "server.started", "server.stopped",
            "server.update_applied", "server.update_failed", "server.update_rolled_back"
        ]},
        "DiscordWebhookUrl": {
            "type": "string", "format": "uri", "writeOnly": true, "maxLength": 2048,
            "pattern": "^https://discord\\.com/api/webhooks/[0-9]+/[A-Za-z0-9_-]{32,256}$"
        },
        "CreateWebhookRequest": {
            "type": "object", "additionalProperties": false, "required": ["name", "url", "events"],
            "properties": {
                "name": {"type": "string", "minLength": 1, "maxLength": 64}, "url": {"$ref": "#/components/schemas/DiscordWebhookUrl"},
                "events": {"type": "array", "minItems": 1, "maxItems": 9, "items": {"$ref": "#/components/schemas/WebhookEvent"}},
                "enabled": {"type": "boolean", "default": true}
            }
        },
        "UpdateWebhookRequest": {
            "type": "object", "additionalProperties": false, "required": ["name", "events", "enabled"],
            "properties": {
                "name": {"type": "string", "minLength": 1, "maxLength": 64},
                "url": {"oneOf": [{"$ref": "#/components/schemas/DiscordWebhookUrl"}, {"type": "null"}]},
                "events": {"type": "array", "minItems": 1, "maxItems": 9, "items": {"$ref": "#/components/schemas/WebhookEvent"}},
                "enabled": {"type": "boolean"}
            }
        },
        "Webhook": {
            "type": "object", "additionalProperties": false,
            "required": ["id", "name", "events", "enabled", "configured", "version", "last_delivery_at", "last_error_code", "created_at", "updated_at"],
            "properties": {
                "id": {"type": "string", "format": "uuid"}, "name": {"type": "string"},
                "events": {"type": "array", "items": {"$ref": "#/components/schemas/WebhookEvent"}},
                "enabled": {"type": "boolean"}, "configured": {"type": "boolean"}, "version": {"type": "integer", "minimum": 1},
                "last_delivery_at": {"type": ["string", "null"], "format": "date-time"}, "last_error_code": {"type": ["string", "null"]},
                "created_at": {"type": "string", "format": "date-time"}, "updated_at": {"type": "string", "format": "date-time"}
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn document_exposes_every_mounted_v1_operation() {
        let document = document();
        assert_eq!(document["openapi"], "3.1.0");
        let actual = document["paths"]
            .as_object()
            .unwrap()
            .iter()
            .flat_map(|(path, item)| {
                item.as_object()
                    .unwrap()
                    .keys()
                    .filter(|method| {
                        matches!(method.as_str(), "get" | "post" | "put" | "patch" | "delete")
                    })
                    .map(move |method| format!("{} {path}", method.to_ascii_uppercase()))
            })
            .collect::<BTreeSet<_>>();
        let expected = EXPECTED_OPERATIONS
            .iter()
            .map(|value| (*value).to_string())
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, expected);
    }

    #[test]
    fn every_protected_mutation_declares_cookie_csrf_and_problem_json() {
        let document = document();
        for (path, item) in document["paths"].as_object().unwrap() {
            for (method, operation) in item.as_object().unwrap() {
                if !matches!(method.as_str(), "post" | "put" | "patch" | "delete")
                    || matches!(path.as_str(), "/auth/setup" | "/auth/login")
                {
                    continue;
                }
                assert_eq!(operation["security"], json!([{"cookieAuth": []}]));
                assert!(
                    operation["parameters"]
                        .as_array()
                        .is_some_and(|parameters| {
                            parameters.iter().any(|parameter| {
                                parameter["$ref"] == "#/components/parameters/CsrfToken"
                            })
                        })
                );
                assert_eq!(
                    operation["responses"]["default"]["$ref"],
                    "#/components/responses/Problem"
                );
            }
        }
    }

    #[test]
    fn committed_frontend_contract_matches_the_backend() {
        let committed: Value = serde_json::from_str(include_str!("../../../frontend/openapi.json"))
            .expect("frontend/openapi.json must contain valid JSON");
        assert_eq!(committed, document());
    }

    const EXPECTED_OPERATIONS: &[&str] = &[
        "GET /audit",
        "GET /auth/me",
        "GET /auth/status",
        "GET /backups",
        "GET /backups/{id}",
        "GET /backups/{id}/download",
        "GET /chat",
        "GET /catalog",
        "GET /catalog/theme",
        "GET /catalog/{kind}/{id}/revisions",
        "GET /catalog/{kind}/{id}/revisions/{revision}",
        "GET /catalog/{kind}/{id}/revisions/{revision}/assets/{asset}",
        "GET /events",
        "GET /files",
        "GET /files/content",
        "GET /files/text",
        "GET /game-profiles",
        "GET /game-profiles/{id}/revisions",
        "GET /game-profiles/{id}/version-catalog",
        "GET /health",
        "GET /jobs",
        "GET /jobs/{id}",
        "GET /mods/providers",
        "GET /notifications",
        "GET /openapi.json",
        "GET /permissions",
        "GET /releases/panel",
        "GET /roles",
        "GET /schedules",
        "GET /schedules/{id}",
        "GET /servers",
        "GET /servers/{id}",
        "GET /servers/{id}/logs",
        "GET /servers/{id}/metrics",
        "GET /servers/{id}/mods",
        "GET /servers/{id}/secrets",
        "GET /users",
        "GET /users/{id}/instances",
        "GET /webhooks",
        "PATCH /roles/{id}",
        "PATCH /servers/{id}",
        "PATCH /users/{id}",
        "POST /auth/login",
        "POST /auth/logout",
        "POST /auth/setup",
        "POST /backups",
        "POST /backups/{id}/restore",
        "POST /chat",
        "POST /catalog/import",
        "POST /files/directories",
        "POST /game-profiles/steam",
        "POST /jobs/{id}/cancel",
        "POST /notifications/read-all",
        "POST /releases/panel/check",
        "POST /roles",
        "POST /schedules",
        "POST /servers",
        "POST /servers/{id}/actions/install",
        "POST /servers/{id}/actions/kill",
        "POST /servers/{id}/actions/restart",
        "POST /servers/{id}/actions/start",
        "POST /servers/{id}/actions/stop",
        "POST /servers/{id}/console",
        "POST /servers/{id}/imports/attach",
        "POST /servers/{id}/imports/copy",
        "POST /servers/{id}/imports/zip",
        "POST /servers/{id}/mods/manual",
        "POST /servers/{id}/mods/provider",
        "POST /users",
        "POST /webhooks",
        "PUT /auth/password",
        "PUT /catalog/theme",
        "PUT /files/content",
        "PUT /files/text",
        "PUT /game-profiles/steam/{id}",
        "PUT /mods/providers/curseforge",
        "PUT /notifications/{id}/read",
        "PUT /schedules/{id}",
        "PUT /servers/{id}/profile-revision",
        "PUT /servers/{id}/secrets/{name}",
        "PUT /users/{user_id}/instances/{instance_id}",
        "PUT /webhooks/{id}",
        "DELETE /backups/{id}",
        "DELETE /chat/{id}",
        "DELETE /catalog/{kind}/{id}/revisions/{revision}",
        "DELETE /files",
        "DELETE /game-profiles/steam/{id}",
        "DELETE /mods/providers/curseforge",
        "DELETE /roles/{id}",
        "DELETE /schedules/{id}",
        "DELETE /servers/{id}",
        "DELETE /servers/{id}/mods/{mod_id}",
        "DELETE /servers/{id}/secrets/{name}",
        "DELETE /users/{user_id}/instances/{instance_id}",
        "DELETE /webhooks/{id}",
    ];
}
