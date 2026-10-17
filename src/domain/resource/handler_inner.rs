use std::time::Duration;

use aws_sdk_kms::primitives::Blob;
use axum::http::StatusCode;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use uuid::Uuid;

use crate::domain::http::{
    MAX_IDENT, bad_request, fail_internal, fail_internal_msg, is_safe_ident,
};
use crate::domain::state::AppState;

use super::Resource;
use super::dao::Dao;
use super::models::{
    ColumnMeta, CreatePersistentKeyRequest, CreateResourceRequest, CreatedPersistentKey,
    CreatedResource, PersistentKeySummary, ResourceSummary,
};
use super::postgres_resource::{DatabaseName, Host, Password, PostgresConfig, Username};
use crate::domain::sanitizer::persistent_sanitizer::PersistentSanitizer;

const MAX_PLAINTEXT: usize = 4096;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[tracing::instrument(skip(state))]
pub(crate) async fn list_resources(
    state: &AppState,
) -> Result<Vec<ResourceSummary>, (StatusCode, &'static str)> {
    let rows = state
        .resources
        .list()
        .await
        .map_err(|err| fail_internal(&err))?;
    Ok(rows.into_iter().map(ResourceSummary::from).collect())
}

#[tracing::instrument(
    skip(state, req),
    fields(
        host = %req.config.host,
        port = req.config.port,
        database = %req.config.database_name,
        table = %req.config.table,
    )
)]
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
    .map_err(|err| fail_internal(&err))?;

    if !valid(&req, &config_bytes) {
        return Err(bad_request());
    }

    probe_resource(&req).await?;

    let (username, password, config) = tokio::try_join!(
        encrypt(state, "username", req.username.as_bytes()),
        encrypt(state, "password", req.password.expose_secret().as_bytes()),
        encrypt(state, "config", &config_bytes),
    )
    .map_err(|err| fail_internal(err.as_ref()))?;

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
        .map_err(|err| fail_internal(&err))?;

    tracing::info!(%resource_id, "resource created");
    Ok(CreatedResource { resource_id })
}

#[tracing::instrument(skip(state), fields(resource_id = %id, schema, table))]
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

    let pool = connect_resource(state, id).await?;
    let columns = query_columns(&pool, schema, table).await;
    pool.close().await;
    columns
}

pub(crate) async fn connect_resource(
    state: &AppState,
    id: Uuid,
) -> Result<PgPool, (StatusCode, &'static str)> {
    let (username, password, config) = decrypted_resource(state, id).await?;
    let options = PgConnectOptions::new()
        .host(&config.host)
        .port(config.port)
        .username(&username)
        .password(&password)
        .database(&config.database_name);

    PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(CONNECT_TIMEOUT)
        .connect_with(options)
        .await
        .map_err(|err| {
            tracing::warn!(error = %err, "cannot connect to database");
            cannot_connect()
        })
}

pub(crate) async fn load_postgres_resource(
    state: &AppState,
    id: Uuid,
    table: &str,
) -> Result<Resource<PostgresConfig, PersistentSanitizer>, (StatusCode, &'static str)> {
    if table.len() > MAX_IDENT || !is_safe_ident(table) {
        return Err((StatusCode::BAD_REQUEST, "invalid table"));
    }
    let rows = state
        .resources
        .list()
        .await
        .map_err(|err| fail_internal(&err))?;
    let flags = rows
        .into_iter()
        .find(|row| row.id == id)
        .ok_or((StatusCode::BAD_REQUEST, "resource not found"))?;
    let (username, password, config) = decrypted_resource(state, id).await?;
    let postgres = PostgresConfig::new(
        id,
        Host::new(config.host).map_err(|_| fail_internal_msg("invalid host"))?,
        config.port,
        Username::parse(SecretString::from(username))
            .map_err(|_| fail_internal_msg("invalid username"))?,
        Password::parse(SecretString::from(password))
            .map_err(|_| fail_internal_msg("invalid password"))?,
        DatabaseName::parse(config.database_name)
            .map_err(|_| fail_internal_msg("invalid database"))?,
        table.to_owned(),
    );
    Ok(Resource::new(
        id,
        flags.allow_upstream,
        flags.allow_downstream,
        postgres,
    ))
}

async fn decrypted_resource(
    state: &AppState,
    id: Uuid,
) -> Result<(String, String, StoredResourceConfig), (StatusCode, &'static str)> {
    let row = state
        .resources
        .load_ciphertext(id)
        .await
        .map_err(|err| fail_internal(&err))?
        .ok_or((StatusCode::BAD_REQUEST, "resource not found"))?;

    let (username, password, config) = tokio::try_join!(
        decrypt(state, "username", &row.username),
        decrypt(state, "password", &row.password),
        decrypt(state, "config", &row.config),
    )
    .map_err(|err| fail_internal(err.as_ref()))?;

    let username = String::from_utf8(username).map_err(|err| fail_internal(&err))?;
    let password = String::from_utf8(password).map_err(|err| fail_internal(&err))?;
    let config = serde_json::from_slice(&config).map_err(|err| fail_internal(&err))?;
    Ok((username, password, config))
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
    .map_err(|err| {
        tracing::warn!(error = %err, "cannot query table");
        cannot_query()
    })
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
        .map_err(|err| {
            tracing::warn!(error = %err, "cannot connect to database");
            cannot_connect()
        })?;

    let sql = format!("SELECT * FROM {} LIMIT 1", req.config.table);
    let queried = sqlx::query(&sql).fetch_optional(&pool).await;
    pool.close().await;
    queried.map_err(|err| {
        tracing::warn!(error = %err, "cannot query table");
        cannot_query()
    })?;
    Ok(())
}

#[tracing::instrument(skip(state), fields(%resource_id))]
pub(crate) async fn list_persistent_keys(
    state: &AppState,
    resource_id: Uuid,
) -> Result<Vec<PersistentKeySummary>, (StatusCode, &'static str)> {
    if !resource_exists(state, resource_id).await? {
        return Err((StatusCode::BAD_REQUEST, "resource not found"));
    }
    let rows = state
        .resources
        .list_persistent_keys(resource_id)
        .await
        .map_err(|err| fail_internal(&err))?;
    Ok(rows
        .into_iter()
        .map(|row| PersistentKeySummary {
            id: row.id,
            name: row.name,
        })
        .collect())
}

#[tracing::instrument(skip(state, req), fields(%resource_id))]
pub(crate) async fn create_persistent_key(
    state: &AppState,
    resource_id: Uuid,
    req: CreatePersistentKeyRequest,
) -> Result<CreatedPersistentKey, (StatusCode, &'static str)> {
    if req.name.len() > MAX_IDENT || !is_safe_ident(&req.name) {
        return Err((StatusCode::BAD_REQUEST, "invalid persistent key"));
    }
    if !resource_exists(state, resource_id).await? {
        return Err((StatusCode::BAD_REQUEST, "resource not found"));
    }
    let row = state
        .resources
        .insert_persistent_key(resource_id, &req.name)
        .await
        .map_err(map_key_error)?;
    tracing::info!(%resource_id, id = %row.id, "persistent key created");
    Ok(CreatedPersistentKey {
        id: row.id,
        name: row.name,
        resource_id,
    })
}

async fn resource_exists(
    state: &AppState,
    resource_id: Uuid,
) -> Result<bool, (StatusCode, &'static str)> {
    state
        .resources
        .resource_exists(resource_id)
        .await
        .map_err(|err| fail_internal(&err))
}

fn map_key_error(err: sqlx::Error) -> (StatusCode, &'static str) {
    match &err {
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
            tracing::warn!("persistent key already exists");
            (StatusCode::CONFLICT, "persistent key already exists")
        }
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23503") => {
            tracing::warn!("resource not found");
            (StatusCode::BAD_REQUEST, "resource not found")
        }
        _ => fail_internal(&err),
    }
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

async fn decrypt(state: &AppState, field: &str, ciphertext: &[u8]) -> Result<Vec<u8>, KmsFailure> {
    let out = state
        .kms
        .decrypt()
        .key_id(&state.kms_key_id)
        .ciphertext_blob(Blob::new(ciphertext.to_vec()))
        .encryption_context("field", field)
        .send()
        .await
        .map_err(|err| Box::new(err) as KmsFailure)?;
    out.plaintext()
        .map(|blob| blob.as_ref().to_vec())
        .ok_or_else(|| missing_kms("kms decrypt returned no plaintext"))
}

async fn encrypt(state: &AppState, field: &str, plaintext: &[u8]) -> Result<Vec<u8>, KmsFailure> {
    let out = state
        .kms
        .encrypt()
        .key_id(&state.kms_key_id)
        .plaintext(Blob::new(plaintext.to_vec()))
        .encryption_context("field", field)
        .send()
        .await
        .map_err(|err| Box::new(err) as KmsFailure)?;
    out.ciphertext_blob()
        .map(|blob| blob.as_ref().to_vec())
        .ok_or_else(|| missing_kms("kms encrypt returned no ciphertext"))
}

type KmsFailure = Box<dyn std::error::Error + Send + Sync>;

fn missing_kms(message: &'static str) -> KmsFailure {
    Box::new(std::io::Error::other(message))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use secrecy::SecretString;
    use uuid::Uuid;

    use super::super::models::ResourceConfig;
    use super::{
        CreatePersistentKeyRequest, CreateResourceRequest, MAX_IDENT, MAX_PLAINTEXT,
        create_persistent_key, list_columns, load_postgres_resource, map_key_error, valid,
    };
    use crate::domain::state::idle_state;

    fn err_message<T>(result: Result<T, (StatusCode, &'static str)>) -> &'static str {
        match result {
            Err((_, message)) => message,
            Ok(_) => panic!("expected a request error"),
        }
    }

    fn request() -> CreateResourceRequest {
        CreateResourceRequest {
            username: "user".into(),
            password: SecretString::from("secret"),
            allow_upstream: true,
            allow_downstream: true,
            config: ResourceConfig {
                host: "localhost".into(),
                port: 5432,
                database_name: "app".into(),
                table: "users".into(),
            },
        }
    }

    #[test]
    fn valid_rejects_each_failed_conjunct() {
        assert!(valid(&request(), b"config"));

        let mut req = request();
        req.username.clear();
        assert!(!valid(&req, b"config"));

        let mut req = request();
        req.username = "a".repeat(MAX_PLAINTEXT + 1);
        assert!(!valid(&req, b"config"));

        let mut req = request();
        req.password = SecretString::from("");
        assert!(!valid(&req, b"config"));

        let mut req = request();
        req.password = SecretString::from("a".repeat(MAX_PLAINTEXT + 1));
        assert!(!valid(&req, b"config"));

        assert!(!valid(&request(), &vec![b'a'; MAX_PLAINTEXT + 1]));

        let mut req = request();
        req.config.host.clear();
        assert!(!valid(&req, b"config"));

        let mut req = request();
        req.config.database_name = "bad-name".into();
        assert!(!valid(&req, b"config"));

        let mut req = request();
        req.config.table = "bad-name".into();
        assert!(!valid(&req, b"config"));
    }

    #[tokio::test]
    async fn column_and_key_guards_reject_unsafe_names_before_io() {
        let state = idle_state();
        let id = Uuid::nil();
        let long = "a".repeat(MAX_IDENT + 1);

        assert_eq!(
            err_message(list_columns(&state, id, &long, "users").await),
            "invalid schema"
        );
        assert_eq!(
            err_message(list_columns(&state, id, "bad-schema", "users").await),
            "invalid schema"
        );
        assert_eq!(
            err_message(list_columns(&state, id, "public", &long).await),
            "invalid table"
        );
        assert_eq!(
            err_message(list_columns(&state, id, "public", "bad-table").await),
            "invalid table"
        );

        assert_eq!(
            err_message(load_postgres_resource(&state, id, &long).await),
            "invalid table"
        );
        assert_eq!(
            err_message(load_postgres_resource(&state, id, "bad-table").await),
            "invalid table"
        );

        assert_eq!(
            err_message(
                create_persistent_key(
                    &state,
                    id,
                    CreatePersistentKeyRequest { name: long.clone() },
                )
                .await,
            ),
            "invalid persistent key"
        );
        assert_eq!(
            err_message(
                create_persistent_key(
                    &state,
                    id,
                    CreatePersistentKeyRequest {
                        name: "bad key".into(),
                    },
                )
                .await,
            ),
            "invalid persistent key"
        );
    }

    #[test]
    fn a_non_database_key_error_is_internal() {
        let (status, message) = map_key_error(sqlx::Error::PoolClosed);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(message, "internal server error");
    }
}
