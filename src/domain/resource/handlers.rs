use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use serde::Deserialize;
use uuid::Uuid;

use crate::domain::http::bad_request;
use crate::domain::state::AppState;

use super::handler_inner;
use super::models::{
    ColumnMeta, CreatedPersistentKey, CreatedResource, PersistentKeySummary, ResourceSummary,
};

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/resources", get(list_resources).post(create_resource))
        .route("/resources/{id}/columns", get(list_columns))
        .route(
            "/resources/{id}/persistent-keys",
            get(list_persistent_keys).post(create_persistent_key),
        )
}

#[derive(Deserialize)]
struct ColumnsQuery {
    #[serde(default = "default_schema")]
    schema: String,
    table: String,
}

fn default_schema() -> String {
    "public".to_owned()
}

async fn list_resources(
    State(state): State<AppState>,
) -> Result<Json<Vec<ResourceSummary>>, (StatusCode, &'static str)> {
    handler_inner::list_resources(&state).await.map(Json)
}

async fn list_columns(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<ColumnsQuery>,
) -> Result<Json<Vec<ColumnMeta>>, (StatusCode, &'static str)> {
    handler_inner::list_columns(&state, id, &query.schema, &query.table)
        .await
        .map(Json)
}

async fn list_persistent_keys(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<PersistentKeySummary>>, (StatusCode, &'static str)> {
    handler_inner::list_persistent_keys(&state, id)
        .await
        .map(Json)
}

async fn create_persistent_key(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    body: Bytes,
) -> Result<(StatusCode, Json<CreatedPersistentKey>), (StatusCode, &'static str)> {
    let req = serde_json::from_slice(&body).map_err(|_| bad_request())?;
    let created = handler_inner::create_persistent_key(&state, id, req).await?;
    Ok((StatusCode::CREATED, Json(created)))
}

async fn create_resource(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<(StatusCode, Json<CreatedResource>), (StatusCode, &'static str)> {
    let req = serde_json::from_slice(&body).map_err(|_| bad_request())?;
    let created = handler_inner::create_resource(&state, req).await?;
    Ok((StatusCode::CREATED, Json(created)))
}
