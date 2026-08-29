use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use rust_decimal::Decimal;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use sqlx::{
    Column, PgPool, Pool, Postgres, QueryBuilder, Row, TypeInfo, ValueRef,
    postgres::{PgPoolOptions, PgRow},
};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::domain::{
    resource::{
        Config, ConnectionError, PullError, PushError, Record, ResourceConnection, Rules,
        SanitizeError, SanitizedRecord, errors::SyncUpConfigError,
    },
    sanitizer::{Sanitizer, in_memory_sanitizer::InMemorySanitizer},
};

use std::u32;

use constants::*;
use errors::*;

#[derive(Clone)]
pub struct PostgresConfig {
    resource_id: Uuid,
    host: Host,
    port: u16,
    username: Username,
    password: Password,
    database_name: DatabaseName,
    table: String,
    pull_batch_strategy: PullBatchStrategy,
}

impl PostgresConfig {
    pub fn new(
        resource_id: Uuid,
        host: Host,
        port: u16,
        username: Username,
        password: Password,
        database_name: DatabaseName,
        table: String,
        pull_batch_strategy: PullBatchStrategy,
    ) -> Self {
        Self {
            resource_id,
            host,
            port,
            username,
            password,
            database_name,
            table,
            pull_batch_strategy,
        }
    }
}

#[async_trait::async_trait]
impl Config<PostgresConfig, InMemorySanitizer> for PostgresConfig {
    type Connection = PostgresConnection;
    async fn connect(&mut self) -> Result<PostgresConnection, ConnectionError> {
        let database_url = format!(
            "postgres://{}:{}@{}:{}/{}",
            self.username.as_ref().expose_secret(),
            self.password.as_ref().expose_secret(),
            self.host.as_ref(),
            self.port,
            self.database_name.as_ref()
        );

        let pool = get_postgres_pool(&database_url)
            .await
            .map_err(|_| ConnectionError::FailedToConnect)?;
        Ok(PostgresConnection {
            resource_id: self.resource_id,
            pool,
            pull_batch_size: PullBatchSize::new(100000),
            sanitize_batch_size: PullBatchSize::new(100000),
            pull_batch_strategy: self.pull_batch_strategy.clone(),
            table: self.table.clone(),
            config: self.clone(),
        })
    }

    async fn sync_up_config(&self) -> Result<(), SyncUpConfigError> {
        Ok(())
    }
}

pub struct PostgresConfigJson {
    host: String,
    port: u16,
    username: String,
    password: String,
    database_name: String,
    table: String,
    pull_batch_strategy: String,
    pull_batch_field: String,
    pull_batch_checkpoint: Option<String>,
}

impl From<PostgresConfig> for PostgresConfigJson {
    fn from(value: PostgresConfig) -> Self {
        let (plan_strategy, field, checkpoint): (String, String, Option<String>) =
            match value.pull_batch_strategy {
                PullBatchStrategy::Cursor { field, pointer } => (CURSOR.to_owned(), field, pointer),
                PullBatchStrategy::LimitOffSet { field, offset } => {
                    let checkpoint = match offset {
                        Some(sql_offset) => Some(sql_offset.get().to_string()),
                        None => None,
                    };
                    (LIMIT_OFFSET.to_owned(), field, checkpoint)
                }
            };
        Self {
            host: value.host.as_ref().to_owned(),
            port: value.port,
            username: value.username.as_ref().expose_secret().to_string(),
            password: value.password.as_ref().expose_secret().to_string(),
            database_name: value.database_name.as_ref().to_owned(),
            table: value.table,
            pull_batch_strategy: plan_strategy,
            pull_batch_field: field,
            pull_batch_checkpoint: checkpoint,
        }
    }
}

#[derive(Clone)]
pub enum PullBatchStrategy {
    Cursor {
        field: String,
        pointer: Option<String>,
    },
    LimitOffSet {
        field: String,
        offset: Option<SqlOffset>,
    },
}

// TODO: add push_strategy to handle edge cases such as pull and push checkpoints are different
// using the same checkpoint is good for now
pub struct PostgresConnection {
    resource_id: Uuid,
    pool: PgPool,
    table: String,
    pull_batch_size: PullBatchSize,
    sanitize_batch_size: PullBatchSize,
    pull_batch_strategy: PullBatchStrategy,
    config: PostgresConfig,
}

#[derive(Clone)]
struct PullBatchSize(u32);

impl PullBatchSize {
    pub fn new(size: u32) -> Self {
        Self(size)
    }

    fn get(&self) -> u32 {
        self.0
    }
}

#[derive(Clone)]
struct SqlOffset(u32);

impl SqlOffset {
    pub fn new(size: u32) -> Self {
        Self(size)
    }

    pub fn get(&self) -> u32 {
        self.0
    }
}

#[async_trait::async_trait]
impl ResourceConnection<PostgresConfig, InMemorySanitizer> for PostgresConnection {
    async fn pull(
        &mut self,
        tx: mpsc::Sender<Vec<Record>>,
        rules: Rules,
    ) -> Result<PostgresConfig, PullError> {
        let strategy = self.pull_batch_strategy.clone();
        let table = self.table.clone();
        let batch_size = self.pull_batch_size.clone();
        let pg_pool = self.pool.clone();

        let task = tokio::spawn(pull_task(
            self.resource_id,
            strategy,
            table,
            batch_size,
            pg_pool,
            rules,
            tx,
        ));
        let _ = task.await.map_err(|_| PullError::UnexpectedFailed)?;

        Ok(self.config.clone())
    }
    async fn sanitize(
        &mut self,
        tx: mpsc::Sender<Vec<SanitizedRecord>>,
        rx: mpsc::Receiver<Vec<Record>>,
        rules: Rules,
        sanitizer: InMemorySanitizer,
    ) -> Result<(), SanitizeError> {
        let sanitizer: Box<dyn Sanitizer> = Box::new(sanitizer);
        let task = tokio::spawn(sanitize_task(tx, rx, rules, sanitizer));

        let _ = task.await.map_err(|_| SanitizeError::UnexpectedFailed)?;

        Ok(())
    }

    async fn push(&mut self, rx: mpsc::Receiver<Vec<SanitizedRecord>>) -> Result<(), PushError> {
        todo!();

        Ok(())
    }
}

async fn pull_task(
    resource_id: Uuid,
    strategy: PullBatchStrategy,
    table: String,
    batch_size: PullBatchSize,
    pg_pool: Pool<Postgres>,
    rules: Rules,
    tx: mpsc::Sender<Vec<Record>>,
) -> Result<PullBatchStrategy, PullError> {
    let mut updated_strategy = strategy.clone();
    match strategy {
        PullBatchStrategy::Cursor { field, pointer } => {
            let mut pointer = pointer;
            let mut first_run = true;
            loop {
                let mut records: Vec<Record> = Vec::new();

                let mut qb =
                    QueryBuilder::<Postgres>::new(format!("SELECT * FROM {} WHERE 1=1", table));

                if let Some(p) = &pointer
                    && !(rules.force_pull_from_start && first_run)
                {
                    qb.push(format!("AND {} >", &field));
                    qb.push_bind(p);
                }

                qb.push(format!(" ORDER BY {} LIMIT", &field));
                qb.push_bind(batch_size.get() as i64);

                let rows = qb
                    .build()
                    .fetch_all(&pg_pool)
                    .await
                    .map_err(|_| PullError::FailedToExtractData)?;

                if rows.is_empty() {
                    break;
                }

                for row in rows.iter() {
                    let mut row_json = json!({ "items": [] });
                    let items = row_json["items"]
                        .as_array_mut()
                        .expect("row json to contains items");

                    for column in row.columns() {
                        let value = decode_pg_value(row, column.ordinal(), column.type_info().name())
                            .map_err(|_| PullError::FailedToExtractData)?;

                        if column.name() == field.as_str() {
                            pointer = Some(json_value_to_checkpoint(&value));
                        }

                        items.push(json!({
                            "field": column.name(),
                            "value": value,
                        }));
                    }

                    if pointer.is_none() {
                        return Err(PullError::FailedToGetPointer);
                    }

                    records.push(Record(row_json));
                }

                let _ = &tx.send(records).await.unwrap();

                let got = rows.len();

                if got < batch_size.get() as usize {
                    break;
                }

                first_run = false;
            }
            updated_strategy = PullBatchStrategy::Cursor { field, pointer }
        }
        PullBatchStrategy::LimitOffSet { field, offset } => {
            todo!("To support later")
        }
    }

    Ok::<PullBatchStrategy, PullError>(updated_strategy)
}

async fn sanitize_task(
    tx: mpsc::Sender<Vec<SanitizedRecord>>,
    mut rx: mpsc::Receiver<Vec<Record>>,
    rules: Rules,
    sanitizer: Box<dyn Sanitizer>,
) -> Result<(), SanitizeError> {
    while let Some(records) = rx.recv().await {
        let sanitized_records = sanitizer
            .sanitize(records, rules.clone())
            .await
            .map_err(|_| SanitizeError::SanitizerFailed)?;

        tx.send(sanitized_records)
            .await
            .map_err(|_| SanitizeError::SanitizerFailed)?;
    }

    Ok(())
}
#[derive(Clone)]
pub struct Host(String);

impl Host {
    pub fn new(host: String) -> Result<Self, HostError> {
        if !host.ends_with(".com") {
            return Err(HostError::UnexpectedDomain);
        }

        Ok(Host(host))
    }
}

impl AsRef<str> for Host {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[derive(Clone)]
pub struct Username(SecretString);

impl Username {
    pub fn parse(username: SecretString) -> Result<Self, UsernameError> {
        Ok(Username(username))
    }
}

impl AsRef<SecretString> for Username {
    fn as_ref(&self) -> &SecretString {
        &self.0
    }
}

#[derive(Clone)]
pub struct Password(SecretString);

impl Password {
    pub fn parse(password: SecretString) -> Result<Self, PasswordError> {
        Ok(Password(password))
    }
}

impl AsRef<SecretString> for Password {
    fn as_ref(&self) -> &SecretString {
        &self.0
    }
}

#[derive(Clone)]
pub struct DatabaseName(String);

impl DatabaseName {
    pub fn parse(database_name: String) -> Result<Self, DatabaseNameError> {
        Ok(DatabaseName(database_name))
    }
}

impl AsRef<str> for DatabaseName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

pub async fn get_postgres_pool(url: &str) -> Result<PgPool, sqlx::Error> {
    // Create a new PostgreSQL connection pool
    PgPoolOptions::new().max_connections(5).connect(url).await
}

fn json_value_to_checkpoint(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn decode_pg_value(row: &PgRow, ordinal: usize, type_name: &str) -> Result<Value, sqlx::Error> {
    let raw = row.try_get_raw(ordinal)?;
    if raw.is_null() {
        return Ok(Value::Null);
    }

    match type_name {
        "BOOL" => Ok(Value::Bool(row.try_get::<bool, _>(ordinal)?)),
        "INT2" => Ok(json!(row.try_get::<i16, _>(ordinal)?)),
        "INT4" => Ok(json!(row.try_get::<i32, _>(ordinal)?)),
        "INT8" => Ok(json!(row.try_get::<i64, _>(ordinal)?)),
        "FLOAT4" => Ok(json!(row.try_get::<f32, _>(ordinal)?)),
        "FLOAT8" => Ok(json!(row.try_get::<f64, _>(ordinal)?)),
        // Keep decimal precision — JSON numbers are not safe for NUMERIC.
        "NUMERIC" => Ok(Value::String(row.try_get::<Decimal, _>(ordinal)?.to_string())),
        "TEXT" | "VARCHAR" | "BPCHAR" | "NAME" | "CITEXT" => {
            Ok(Value::String(row.try_get::<String, _>(ordinal)?))
        }
        "UUID" => Ok(Value::String(row.try_get::<Uuid, _>(ordinal)?.to_string())),
        "DATE" => Ok(Value::String(row.try_get::<NaiveDate, _>(ordinal)?.to_string())),
        "TIME" => Ok(Value::String(row.try_get::<NaiveTime, _>(ordinal)?.to_string())),
        "TIMESTAMP" => Ok(Value::String(
            row.try_get::<NaiveDateTime, _>(ordinal)?.to_string(),
        )),
        "TIMESTAMPTZ" => Ok(Value::String(
            row.try_get::<DateTime<Utc>, _>(ordinal)?.to_rfc3339(),
        )),
        "BYTEA" => Ok(Value::String(BASE64.encode(row.try_get::<Vec<u8>, _>(ordinal)?))),
        "JSON" | "JSONB" => row.try_get::<Value, _>(ordinal),
        // Last resort for uncommon/extension types that are text-compatible.
        _ => match row.try_get::<String, _>(ordinal) {
            Ok(s) => Ok(Value::String(s)),
            Err(e) => Err(e),
        },
    }
}

mod errors {
    pub enum DatabaseNameError {}
    pub enum PasswordError {}

    pub enum UsernameError {}

    pub enum PullBatchSizeError {
        CannotConvertBackToU32,
    }

    pub enum HostError {
        UnexpectedDomain,
    }
}

mod constants {
    pub const CURSOR: &'static str = "cursor";
    pub const LIMIT_OFFSET: &'static str = "LimitOffSet";
}
