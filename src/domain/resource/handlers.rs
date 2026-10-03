use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;

use crate::domain::http::bad_request;
use crate::domain::state::AppState;

use super::handler_inner;
use super::models::{CreatedResource, ResourceSummary};

pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/resources", get(list_resources).post(create_resource))
}

async fn list_resources(
    State(state): State<AppState>,
) -> Result<Json<Vec<ResourceSummary>>, (StatusCode, &'static str)> {
    handler_inner::list_resources(&state).await.map(Json)
}

async fn create_resource(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<(StatusCode, Json<CreatedResource>), (StatusCode, &'static str)> {
    let req = serde_json::from_slice(&body).map_err(|_| bad_request())?;
    let created = handler_inner::create_resource(&state, req).await?;
    Ok((StatusCode::CREATED, Json(created)))
}
