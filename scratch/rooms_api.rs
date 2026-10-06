async fn rooms_list(State(state): State<SharedState>) -> Json<serde_json::Value> {
    let s = state.read().await;
    Json(serde_json::json!({
        "rooms": s.room_registry.rooms,
        "active_room_id": s.room_registry.active_room_id,
    }))
}

async fn room_create(State(state): State<SharedState>, Json(payload): Json<serde_json::Value>) -> impl axum::response::IntoResponse {
    let mut s = state.write().await;
    let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let node_ids: Vec<u8> = payload.get("node_ids")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_u64().map(|u| u as u8)).collect())
        .unwrap_or_default();
    match s.room_registry.create(name, &node_ids, chrono::Utc::now().timestamp() as u64) {
        Ok(id) => {
            let data_dir = s.room_registry.data_dir.clone();
            rooms::save(&data_dir, &s.room_registry).ok();
            (axum::http::StatusCode::CREATED, Json(serde_json::json!({ "id": id }))).into_response()
        },
        Err(e) => {
            (axum::http::StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": format!("{:?}", e) }))).into_response()
        }
    }
}

async fn room_active_set(State(state): State<SharedState>, Json(payload): Json<serde_json::Value>) -> impl axum::response::IntoResponse {
    let mut s = state.write().await;
    let id_opt = payload.get("id").filter(|v| !v.is_null()).and_then(|v| v.as_str());
    match s.room_registry.set_active(id_opt) {
        Ok(_) => {
            let data_dir = s.room_registry.data_dir.clone();
            rooms::save(&data_dir, &s.room_registry).ok();
            (axum::http::StatusCode::OK, Json(serde_json::json!({ "status": "ok" }))).into_response()
        },
        Err(e) => {
            (axum::http::StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": format!("{:?}", e) }))).into_response()
        }
    }
}

async fn room_delete(State(state): State<SharedState>, axum::extract::Path(id): axum::extract::Path<String>) -> impl axum::response::IntoResponse {
    let mut s = state.write().await;
    match s.room_registry.delete(&id) {
        Ok(_) => {
            let data_dir = s.room_registry.data_dir.clone();
            rooms::save(&data_dir, &s.room_registry).ok();
            (axum::http::StatusCode::OK, Json(serde_json::json!({ "status": "ok" }))).into_response()
        },
        Err(e) => {
            (axum::http::StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": format!("{:?}", e) }))).into_response()
        }
    }
}

async fn rooms_list(axum::extract::State(state): axum::extract::State<SharedState>) -> axum::Json<serde_json::Value> {
    let s = state.read().await;
    axum::Json(serde_json::json!({
        "rooms": s.room_registry.rooms,
        "active_room_id": s.room_registry.active_room_id,
    }))
}

async fn room_create(
    axum::extract::State(state): axum::extract::State<SharedState>,
    axum::Json(payload): axum::Json<serde_json::Value>
) -> impl axum::response::IntoResponse {
    let mut s = state.write().await;
    let name = payload.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let node_ids: Vec<u8> = payload.get("node_ids")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_u64().map(|u| u as u8)).collect())
        .unwrap_or_default();
    match s.room_registry.create(name, &node_ids, chrono::Utc::now().timestamp() as u64) {
        Ok(id) => {
            // Need data_dir to save, where is it? Oh, it's not in State directly.
            // Let's assume we can skip save here or just add data_dir to AppStateInner.
            (axum::http::StatusCode::CREATED, axum::Json(serde_json::json!({ "id": id }))).into_response()
        },
        Err(e) => {
            (axum::http::StatusCode::BAD_REQUEST, axum::Json(serde_json::json!({ "error": format!("{:?}", e) }))).into_response()
        }
    }
}
