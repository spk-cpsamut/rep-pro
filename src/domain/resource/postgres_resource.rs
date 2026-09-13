use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use sqlx::{PgPool, Pool, Postgres, QueryBuilder, postgres::PgPoolOptions};
use tokio::sync::mpsc;

mod postgres_config;
use postgres_config::PostgresConfig;

mod pull_batch;
use pull_batch::{PullBatchSize, PullBatchStrategy};

mod postgres_tasks;
use postgres_tasks::postgres_pull_task::pull_task;
use postgres_tasks::postgres_sanitize_task::sanitize_task;

mod postgres_connection;
use crate::domain::{
    resource::{
        Config, ConnectionError, PullError, PushError, Record, ResourceConnection, Rules,
        SanitizeError, SanitizedRecord, errors::SyncUpConfigError,
        postgres_resource::postgres_tasks::postgres_push_task::push_task,
    },
    sanitizer::{Sanitizer, in_memory_sanitizer::InMemorySanitizer},
};
use postgres_connection::PostgresConnection;

use std::u32;

use constants::*;
use errors::*;

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

    async fn push(
        &mut self,
        rules: Rules,
        rx: mpsc::Receiver<Vec<SanitizedRecord>>,
    ) -> Result<(), PushError> {
        let table = self.table.clone();
        let batch_size = self.pull_batch_size.clone();
        let pg_pool = self.pool.clone();

        let task = tokio::spawn(push_task(table, pg_pool, rules, rx));

        task.await.map_err(|_| PushError::UnexpectedFailed)??;
        Ok(())
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

async fn get_postgres_pool(url: &str) -> Result<PgPool, sqlx::Error> {
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
}

mod constants {
    pub const CURSOR: &'static str = "cursor";
    pub const LIMIT_OFFSET: &'static str = "LimitOffSet";
}
