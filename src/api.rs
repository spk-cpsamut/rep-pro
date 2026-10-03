use std::collections::HashMap;

use aws_sdk_kms::primitives::Blob;
use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use chrono::{DateTime, Utc};
use rep_pro::domain::resource_map::PairConfig;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::types::Json as SqlxJson;
use uuid::Uuid;

const MAX_PLAINTEXT: usize = 4096;
const MAX_IDENT: usize = 255;

const NAMING_CONVENTIONS: &[&str] = &[
    "camel_case",
    "pascal_case",
    "snake_case",
    "screaming_case",
    "kebab_case",
    "train_case",
];

const SANITIZE_TYPES: &[&str] = &["name", "email", "phone_number", "ssn"];

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub kms: aws_sdk_kms::Client,
    pub kms_key_id: String,
}

#[derive(Deserialize)]
struct CreateResourceRequest {
    username: String,
    password: SecretString,
    allow_upstream: bool,
    allow_downstream: bool,
    config: ResourceConfig,
}

#[derive(Deserialize)]
struct ResourceConfig {
    host: String,
    port: u16,
    database_name: String,
    table: String,
}

#[derive(Serialize)]
struct CreatedResource {
    resource_id: Uuid,
}

#[derive(sqlx::FromRow)]
struct ResourceRow {
    id: Uuid,
    allow_upstream: bool,
    allow_downstream: bool,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct ResourceSummary {
    id: Uuid,
    allow_upstream: bool,
    allow_downstream: bool,
    created_at: String,
}

#[derive(sqlx::FromRow)]
struct ResourceFlags {
    allow_upstream: bool,
    allow_downstream: bool,
}

#[derive(Deserialize)]
struct CreateResourceMapRequest {
    resource_source: Uuid,
    resource_target: Uuid,
    table_source: String,
    table_target: String,
    pair_config: PairConfigRequest,
    rules: Vec<RuleRequest>,
}

#[derive(Deserialize)]
struct PairConfigRequest {
    pull_batch_strategy: String,
    pull_batch_field: String,
}

#[derive(Deserialize)]
struct RuleRequest {
    force_pull_from_start: bool,
    source_naming_convention: String,
    target_naming_convention: String,
    source_identifier: String,
    fields: Vec<FieldRequest>,
}

#[derive(Deserialize)]
struct FieldRequest {
    field_name: String,
    sanitize_type: String,
}

#[derive(Serialize)]
struct CreatedResourceMap {
    resource_map_id: Uuid,
    rule_ids: Vec<Uuid>,
}

#[derive(sqlx::FromRow)]
struct ResourceMapRow {
    id: Uuid,
    resource_source: Uuid,
    resource_target: Uuid,
    table_source: String,
    table_target: String,
    pair_config: SqlxJson<Value>,
}

#[derive(sqlx::FromRow)]
struct RuleRow {
    id: Uuid,
    resource_map_id: Uuid,
    rule: SqlxJson<Value>,
}

#[derive(Serialize)]
struct ResourceMapSummary {
    resource_map_id: Uuid,
    resource_source: Uuid,
    resource_target: Uuid,
    table_source: String,
    table_target: String,
    pair_config: Value,
    rules: Vec<RuleSummary>,
}

#[derive(Serialize)]
struct RuleSummary {
    rule_id: Uuid,
    rule: Value,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/resources", get(list_resources).post(create_resource))
        .route(
            "/resource-maps",
            get(list_resource_maps).post(create_resource_map),
        )
        .with_state(state)
}

async fn list_resources(
    State(state): State<AppState>,
) -> Result<Json<Vec<ResourceSummary>>, (StatusCode, &'static str)> {
    let rows: Vec<ResourceRow> = sqlx::query_as(
        "SELECT id, allow_upstream, allow_downstream, created_at FROM resources ORDER BY created_at DESC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| internal_error())?;

    Ok(Json(
        rows.into_iter()
            .map(|row| ResourceSummary {
                id: row.id,
                allow_upstream: row.allow_upstream,
                allow_downstream: row.allow_downstream,
                created_at: row.created_at.to_rfc3339(),
            })
            .collect(),
    ))
}

async fn create_resource(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<(StatusCode, Json<CreatedResource>), (StatusCode, &'static str)> {
    let req: CreateResourceRequest = serde_json::from_slice(&body).map_err(|_| bad_request())?;
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
        encrypt(&state, "username", req.username.as_bytes()),
        encrypt(&state, "password", req.password.expose_secret().as_bytes()),
        encrypt(&state, "config", &config_bytes),
    )
    .map_err(|_| internal_error())?;

    let resource_id: Uuid = sqlx::query_scalar(
        "INSERT INTO resources (username, password, config, allow_upstream, allow_downstream) VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(username)
    .bind(password)
    .bind(config)
    .bind(req.allow_upstream)
    .bind(req.allow_downstream)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| internal_error())?;

    Ok((StatusCode::CREATED, Json(CreatedResource { resource_id })))
}

async fn list_resource_maps(
    State(state): State<AppState>,
) -> Result<Json<Vec<ResourceMapSummary>>, (StatusCode, &'static str)> {
    let maps: Vec<ResourceMapRow> = sqlx::query_as(
        "SELECT id, resource_source, resource_target, table_source, table_target, pair_config FROM resource_maps ORDER BY created_at DESC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| internal_error())?;

    let rules: Vec<RuleRow> = sqlx::query_as(
        "SELECT id, resource_map_id, rule FROM rules ORDER BY created_at ASC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|_| internal_error())?;

    let mut rules_by_map: HashMap<Uuid, Vec<RuleSummary>> = HashMap::new();
    for rule in rules {
        rules_by_map
            .entry(rule.resource_map_id)
            .or_default()
            .push(RuleSummary {
                rule_id: rule.id,
                rule: rule.rule.0,
            });
    }

    Ok(Json(
        maps.into_iter()
            .map(|row| ResourceMapSummary {
                resource_map_id: row.id,
                resource_source: row.resource_source,
                resource_target: row.resource_target,
                table_source: row.table_source,
                table_target: row.table_target,
                pair_config: row.pair_config.0,
                rules: rules_by_map.remove(&row.id).unwrap_or_default(),
            })
            .collect(),
    ))
}

async fn create_resource_map(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<(StatusCode, Json<CreatedResourceMap>), (StatusCode, &'static str)> {
    let req: CreateResourceMapRequest = serde_json::from_slice(&body).map_err(|_| bad_request())?;
    validate_map_request(&req)?;
    let pair = pair_config_for_insert(&req.pair_config)?;

    let mut tx = state.pool.begin().await.map_err(|_| internal_error())?;
    let source = load_flags(&mut tx, req.resource_source).await?;
    let target = load_flags(&mut tx, req.resource_target).await?;
    let source = source.ok_or((StatusCode::BAD_REQUEST, "resource not found"))?;
    let target = target.ok_or((StatusCode::BAD_REQUEST, "resource not found"))?;
    if !source.allow_downstream {
        return Err((StatusCode::BAD_REQUEST, "source does not allow downstream"));
    }
    if !target.allow_upstream {
        return Err((StatusCode::BAD_REQUEST, "target does not allow upstream"));
    }

    let resource_map_id: Uuid = sqlx::query_scalar(
        "INSERT INTO resource_maps (resource_source, resource_target, table_source, table_target, pair_config) VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(req.resource_source)
    .bind(req.resource_target)
    .bind(&req.table_source)
    .bind(&req.table_target)
    .bind(SqlxJson(pair.to_json()))
    .fetch_one(&mut *tx)
    .await
    .map_err(map_db_error)?;

    let mut rule_ids = Vec::with_capacity(req.rules.len());
    for rule in &req.rules {
        let rule_id: Uuid = sqlx::query_scalar(
            "INSERT INTO rules (resource_map_id, rule) VALUES ($1, $2) RETURNING id",
        )
        .bind(resource_map_id)
        .bind(SqlxJson(stored_rule(rule)))
        .fetch_one(&mut *tx)
        .await
        .map_err(map_db_error)?;
        rule_ids.push(rule_id);
    }

    tx.commit().await.map_err(|_| internal_error())?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedResourceMap {
            resource_map_id,
            rule_ids,
        }),
    ))
}

async fn load_flags(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
) -> Result<Option<ResourceFlags>, (StatusCode, &'static str)> {
    sqlx::query_as("SELECT allow_upstream, allow_downstream FROM resources WHERE id = $1")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| internal_error())
}

fn validate_map_request(req: &CreateResourceMapRequest) -> Result<(), (StatusCode, &'static str)> {
    if !valid_table(&req.table_source) {
        return Err((StatusCode::BAD_REQUEST, "invalid table_source"));
    }
    if !valid_table(&req.table_target) {
        return Err((StatusCode::BAD_REQUEST, "invalid table_target"));
    }
    if !is_safe_ident(&req.pair_config.pull_batch_field) {
        return Err((StatusCode::BAD_REQUEST, "invalid pull field"));
    }
    if req.rules.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "at least one rule is required"));
    }
    for rule in &req.rules {
        if !NAMING_CONVENTIONS.contains(&rule.source_naming_convention.as_str())
            || !NAMING_CONVENTIONS.contains(&rule.target_naming_convention.as_str())
        {
            return Err((StatusCode::BAD_REQUEST, "invalid naming convention"));
        }
        if !is_safe_ident(&rule.source_identifier) {
            return Err((StatusCode::BAD_REQUEST, "invalid source identifier"));
        }
        if rule.fields.is_empty() {
            return Err((StatusCode::BAD_REQUEST, "each rule needs a field"));
        }
        for field in &rule.fields {
            if !is_safe_ident(&field.field_name) {
                return Err((StatusCode::BAD_REQUEST, "invalid field name"));
            }
            if !SANITIZE_TYPES.contains(&field.sanitize_type.as_str()) {
                return Err((StatusCode::BAD_REQUEST, "invalid sanitize type"));
            }
        }
    }
    Ok(())
}

fn pair_config_for_insert(
    req: &PairConfigRequest,
) -> Result<PairConfig, (StatusCode, &'static str)> {
    let value = json!({
        "pull_batch_strategy": req.pull_batch_strategy,
        "pull_batch_field": req.pull_batch_field,
        "pull_batch_checkpoint": null,
    });
    PairConfig::from_json(value).map_err(|_| (StatusCode::BAD_REQUEST, "invalid pull strategy"))
}

fn stored_rule(rule: &RuleRequest) -> Value {
    json!({
        "force_pull_from_start": rule.force_pull_from_start,
        "source_naming_convention": rule.source_naming_convention,
        "target_naming_convention": rule.target_naming_convention,
        "source_identifier": rule.source_identifier,
        "source_sanitize_fields": rule.fields.iter().map(|field| json!({
            "field_name": field.field_name,
            "sanitize_type": field.sanitize_type,
        })).collect::<Vec<_>>(),
    })
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

fn valid_table(name: &str) -> bool {
    name.len() <= MAX_IDENT && is_safe_ident(name)
}

fn is_safe_ident(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
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

fn map_db_error(err: sqlx::Error) -> (StatusCode, &'static str) {
    match &err {
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
            (StatusCode::CONFLICT, "resource map already exists")
        }
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23503") => {
            (StatusCode::BAD_REQUEST, "resource not found")
        }
        _ => internal_error(),
    }
}

fn bad_request() -> (StatusCode, &'static str) {
    (StatusCode::BAD_REQUEST, "bad request")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request() -> CreateResourceMapRequest {
        CreateResourceMapRequest {
            resource_source: Uuid::nil(),
            resource_target: Uuid::nil(),
            table_source: "users".into(),
            table_target: "users_out".into(),
            pair_config: PairConfigRequest {
                pull_batch_strategy: "cursor".into(),
                pull_batch_field: "id".into(),
            },
            rules: vec![RuleRequest {
                force_pull_from_start: false,
                source_naming_convention: "snake_case".into(),
                target_naming_convention: "snake_case".into(),
                source_identifier: "id".into(),
                fields: vec![FieldRequest {
                    field_name: "email".into(),
                    sanitize_type: "email".into(),
                }],
            }],
        }
    }

    #[test]
    fn pair_config_starts_without_a_checkpoint() {
        let pair = pair_config_for_insert(&sample_request().pair_config).unwrap();
        let json = pair.to_json();
        assert_eq!(json["pull_batch_strategy"], "cursor");
        assert_eq!(json["pull_batch_field"], "id");
        assert_eq!(json["pull_batch_checkpoint"], Value::Null);
    }

    #[test]
    fn stored_rule_uses_domain_field_names() {
        let rule = stored_rule(&sample_request().rules[0]);
        assert_eq!(rule["source_identifier"], "id");
        assert_eq!(rule["source_naming_convention"], "snake_case");
        assert_eq!(rule["source_sanitize_fields"][0]["field_name"], "email");
        assert_eq!(rule["source_sanitize_fields"][0]["sanitize_type"], "email");
        assert!(rule.get("fields").is_none());
    }

    #[test]
    fn rejects_bad_link_input() {
        let mut req = sample_request();
        req.pair_config.pull_batch_strategy = "offset".into();
        assert!(pair_config_for_insert(&req.pair_config).is_err());

        req = sample_request();
        req.rules.clear();
        assert_eq!(
            validate_map_request(&req).unwrap_err().1,
            "at least one rule is required"
        );

        req = sample_request();
        req.table_source = "users-out".into();
        assert_eq!(
            validate_map_request(&req).unwrap_err().1,
            "invalid table_source"
        );
    }
}

fn internal_error() -> (StatusCode, &'static str) {
    (StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
}
