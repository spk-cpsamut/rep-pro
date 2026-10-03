use aws_sdk_kms::primitives::Blob;
use axum::http::StatusCode;
use secrecy::ExposeSecret;
use serde_json::json;

use crate::domain::http::{bad_request, internal_error, is_safe_ident};
use crate::domain::state::AppState;

use super::dao::Dao;
use super::models::{CreateResourceRequest, CreatedResource, ResourceSummary};

const MAX_PLAINTEXT: usize = 4096;

pub(crate) async fn list_resources(
    state: &AppState,
) -> Result<Vec<ResourceSummary>, (StatusCode, &'static str)> {
    let rows = state.resources.list().await.map_err(|_| internal_error())?;
    Ok(rows.into_iter().map(ResourceSummary::from).collect())
}

pub(crate) async fn create_resource(
    state: &AppState,
    req: CreateResourceRequest,
) -> Result<CreatedResource, (StatusCode, &'static str)> {
    let config_bytes = serde_json::to_vec(&json!({
        "resource_type": "postgres",
        "host": req.config.host,
        "port": req.config.port,
        "database_name": req.config.database_name,
        "table": req.config.table,
    }))
    .map_err(|_| internal_error())?;

    if !valid(&req, &config_bytes) {
        return Err(bad_request());
    }

    let (username, password, config) = tokio::try_join!(
        encrypt(state, "username", req.username.as_bytes()),
        encrypt(state, "password", req.password.expose_secret().as_bytes()),
        encrypt(state, "config", &config_bytes),
    )
    .map_err(|_| internal_error())?;

    let resource_id = state
        .resources
        .insert(
            &username,
            &password,
            &config,
            req.allow_upstream,
            req.allow_downstream,
        )
        .await
        .map_err(|_| internal_error())?;

    Ok(CreatedResource { resource_id })
}

fn valid(req: &CreateResourceRequest, config_bytes: &[u8]) -> bool {
    !req.username.is_empty()
        && req.username.len() <= MAX_PLAINTEXT
        && !req.password.expose_secret().is_empty()
        && req.password.expose_secret().len() <= MAX_PLAINTEXT
        && config_bytes.len() <= MAX_PLAINTEXT
        && !req.config.host.is_empty()
        && is_safe_ident(&req.config.database_name)
        && is_safe_ident(&req.config.table)
}

async fn encrypt(state: &AppState, field: &str, plaintext: &[u8]) -> Result<Vec<u8>, ()> {
    let out = state
        .kms
        .encrypt()
        .key_id(&state.kms_key_id)
        .plaintext(Blob::new(plaintext.to_vec()))
        .encryption_context("field", field)
        .send()
        .await
        .map_err(|_| ())?;
    out.ciphertext_blob()
        .map(|blob| blob.as_ref().to_vec())
        .ok_or(())
}
