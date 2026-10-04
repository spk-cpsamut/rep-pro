use std::time::Duration;

use aws_sdk_kms::primitives::Blob;
use axum::http::StatusCode;
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use uuid::Uuid;

use crate::domain::http::{MAX_IDENT, bad_request, internal_error, is_safe_ident};
use crate::domain::state::AppState;

use super::dao::Dao;
use super::models::{ColumnMeta, CreateResourceRequest, CreatedResource, ResourceSummary};

const MAX_PLAINTEXT: usize = 4096;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

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

    probe_resource(&req).await?;

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

pub(crate) async fn list_columns(
    state: &AppState,
    id: Uuid,
    schema: &str,
    table: &str,
) -> Result<Vec<ColumnMeta>, (StatusCode, &'static str)> {
    if schema.len() > MAX_IDENT || !is_safe_ident(schema) {
        return Err((StatusCode::BAD_REQUEST, "invalid schema"));
    }
    if table.len() > MAX_IDENT || !is_safe_ident(table) {
        return Err((StatusCode::BAD_REQUEST, "invalid table"));
    }

    let row = state
        .resources
        .load_ciphertext(id)
        .await
        .map_err(|_| internal_error())?
        .ok_or((StatusCode::BAD_REQUEST, "resource not found"))?;

    let (username, password, config) = tokio::try_join!(
        decrypt(state, "username", &row.username),
        decrypt(state, "password", &row.password),
        decrypt(state, "config", &row.config),
    )
    .map_err(|_| internal_error())?;

    let username = String::from_utf8(username).map_err(|_| internal_error())?;
    let password = String::from_utf8(password).map_err(|_| internal_error())?;
    let config: StoredResourceConfig =
        serde_json::from_slice(&config).map_err(|_| internal_error())?;

    let options = PgConnectOptions::new()
        .host(&config.host)
        .port(config.port)
        .username(&username)
        .password(&password)
        .database(&config.database_name);

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(CONNECT_TIMEOUT)
        .connect_with(options)
        .await
        .map_err(|_| cannot_connect())?;

    let columns = query_columns(&pool, schema, table).await;
    pool.close().await;
    columns
}

#[derive(Deserialize)]
struct StoredResourceConfig {
    host: String,
    port: u16,
    database_name: String,
}

async fn query_columns(
    pool: &PgPool,
    schema: &str,
    table: &str,
) -> Result<Vec<ColumnMeta>, (StatusCode, &'static str)> {
    sqlx::query_as(
        r#"
        SELECT
            c.column_name::text AS column_name,
            c.data_type::text AS data_type,
            CASE
                WHEN tc.constraint_type = 'PRIMARY KEY' THEN true
                ELSE false
            END AS is_primary_key
        FROM information_schema.columns c
        LEFT JOIN information_schema.key_column_usage kcu
            ON c.table_schema = kcu.table_schema
            AND c.table_name = kcu.table_name
            AND c.column_name = kcu.column_name
        LEFT JOIN information_schema.table_constraints tc
            ON kcu.constraint_name = tc.constraint_name
            AND kcu.table_schema = tc.table_schema
            AND kcu.table_name = tc.table_name
        WHERE c.table_schema = $1
          AND c.table_name = $2
        ORDER BY c.ordinal_position
        "#,
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(|_| cannot_query())
}

async fn probe_resource(req: &CreateResourceRequest) -> Result<(), (StatusCode, &'static str)> {
    let options = PgConnectOptions::new()
        .host(&req.config.host)
        .port(req.config.port)
        .username(&req.username)
        .password(req.password.expose_secret())
        .database(&req.config.database_name);

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(CONNECT_TIMEOUT)
        .connect_with(options)
        .await
        .map_err(|_| cannot_connect())?;

    let sql = format!("SELECT * FROM {} LIMIT 1", req.config.table);
    let queried = sqlx::query(&sql).fetch_optional(&pool).await;
    pool.close().await;
    queried.map_err(|_| cannot_query())?;
    Ok(())
}

fn cannot_connect() -> (StatusCode, &'static str) {
    (StatusCode::BAD_REQUEST, "cannot connect to database")
}

fn cannot_query() -> (StatusCode, &'static str) {
    (StatusCode::BAD_REQUEST, "cannot query table")
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

async fn decrypt(state: &AppState, field: &str, ciphertext: &[u8]) -> Result<Vec<u8>, ()> {
    let out = state
        .kms
        .decrypt()
        .key_id(&state.kms_key_id)
        .ciphertext_blob(Blob::new(ciphertext.to_vec()))
        .encryption_context("field", field)
        .send()
        .await
        .map_err(|_| ())?;
    out.plaintext().map(|blob| blob.as_ref().to_vec()).ok_or(())
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
