use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use sqlx::{Column, PgPool, Pool, Postgres, QueryBuilder, Row, postgres::PgPoolOptions};
use tokio::sync::mpsc;

use crate::domain::resource::{
    Config, ConnectionError, PullError, PushError, Record, ResourceConnection, Rules,
    SanitizeError, SanitizedRecord,
};

use std::u32;

use errors::*;
pub struct PostgresConfig {
    host: Host,
    port: u16,
    username: Username,
    password: Password,
    database_name: DatabaseName,
    table: String,
    pull_batch_startegy: PullBatchStrategy,
}

impl PostgresConfig {
    pub fn new(
        host: Host,
        port: u16,
        username: Username,
        password: Password,
        database_name: DatabaseName,
        table: String,
        pull_batch_startegy: PullBatchStrategy,
    ) -> Self {
        Self {
            host,
            port,
            username,
            password,
            database_name,
            table,
            pull_batch_startegy,
        }
    }
}

#[async_trait::async_trait]
impl Config for PostgresConfig {
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
            pool,
            pull_batch_size: PullBatchSize::new(100000),
            sanitize_batch_size: PullBatchSize::new(100000),
            pull_batch_strategy: self.pull_batch_startegy.clone(),
            table: self.table.clone(),
        })
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

pub struct PostgresConnection {
    pool: PgPool,
    table: String,
    pull_batch_size: PullBatchSize,
    sanitize_batch_size: PullBatchSize,
    pull_batch_strategy: PullBatchStrategy,
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
}

#[async_trait::async_trait]
impl ResourceConnection for PostgresConnection {
    async fn pull(&mut self, tx: mpsc::Sender<Vec<Record>>, rules: Rules) -> Result<(), PullError> {
        let strategy = self.pull_batch_strategy.clone();
        let table = self.table.clone();
        let batch_size = self.pull_batch_size.clone();
        let pg_pool = self.pool.clone();

        let task = tokio::spawn(pull_task(strategy, table, batch_size, pg_pool, rules, tx));
        let _ = task.await.unwrap();
        Ok(())
    }
    async fn sanitize(
        &mut self,
        tx: mpsc::Sender<Vec<SanitizedRecord>>,
        rx: mpsc::Receiver<Vec<Record>>,
        rules: Rules,
    ) -> Result<(), SanitizeError> {
        todo!();
    }

    async fn push(&mut self, rx: mpsc::Receiver<Vec<SanitizedRecord>>) -> Result<(), PushError> {
        todo!();
    }
}

async fn pull_task(
    strategy: PullBatchStrategy,
    table: String,
    batch_size: PullBatchSize,
    pg_pool: Pool<Postgres>,
    rules: Rules,
    tx: mpsc::Sender<Vec<Record>>,
) -> Result<(), PullError> {
    let startegy = strategy;
    match startegy {
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
                qb.push_bind(batch_size.get().to_string());

                let rows = qb.build().fetch_all(&pg_pool).await.unwrap();

                if rows.is_empty() {
                    break;
                }

                for row in rows.iter() {
                    let new_ptr: String = row
                        .try_get(&field.as_str())
                        .map_err(|_| PullError::FailedToGetPointer)?;

                    pointer = Some(new_ptr);

                    let columns = row.columns();

                    let mut row_json = json!({ "items": []});

                    for column in columns.iter() {
                        let value: String = row
                            .try_get(column.name())
                            .map_err(|_| PullError::FailedToExtactData)?;

                        row_json["items"]
                            .as_array_mut()
                            .expect("row json to contains items")
                            .push(json!({"field": column.name(), "value": value}));
                    }
                    records.push(Record(row_json));
                }

                let got = rows.len();

                if got < batch_size.get() as usize {
                    break;
                }

                let _ = &tx.send(records).await.unwrap();

                first_run = false;
            }
        }
        PullBatchStrategy::LimitOffSet { field, offset } => {
            todo!("To support later")
        }
    }

    Ok::<(), PullError>(())
}

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
