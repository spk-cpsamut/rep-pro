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

#[cfg(test)]
mod tests {
    use axum::body::Bytes;
    use axum::extract::{Path, State};
    use uuid::Uuid;

    use super::{ColumnsQuery, bad_request, create_persistent_key, create_resource};
    use crate::domain::state::idle_state;

    #[tokio::test]
    async fn a_body_that_is_not_json_is_a_bad_request() {
        let state = idle_state();
        let body = Bytes::from_static(b"not json");
        let created = create_resource(State(state.clone()), body.clone()).await;
        assert!(matches!(created, Err(got) if got == bad_request()));
        let key = create_persistent_key(State(state), Path(Uuid::nil()), body).await;
        assert!(matches!(key, Err(got) if got == bad_request()));
    }

    #[test]
    fn columns_query_defaults_the_schema_to_public() {
        let query: ColumnsQuery = serde_json::from_str(r#"{"table":"users"}"#).unwrap();
        assert_eq!(query.schema, "public");
    }
}
