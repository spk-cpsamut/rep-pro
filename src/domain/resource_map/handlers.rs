use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;

use crate::domain::http::bad_request;
use crate::domain::state::AppState;

use super::handler_inner;
use super::models::{CreatedResourceMap, ResourceMapSummary};

pub(crate) fn router() -> Router<AppState> {
    Router::new().route(
        "/resource-maps",
        get(list_resource_maps).post(create_resource_map),
    )
}

async fn list_resource_maps(
    State(state): State<AppState>,
) -> Result<Json<Vec<ResourceMapSummary>>, (StatusCode, &'static str)> {
    handler_inner::list_resource_maps(&state).await.map(Json)
}

async fn create_resource_map(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<(StatusCode, Json<CreatedResourceMap>), (StatusCode, &'static str)> {
    let req = serde_json::from_slice(&body).map_err(|_| bad_request())?;
    let created = handler_inner::create_resource_map(&state, req).await?;
    Ok((StatusCode::CREATED, Json(created)))
}
