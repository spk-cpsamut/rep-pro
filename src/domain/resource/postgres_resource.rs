use secrecy::{ExposeSecret, SecretString};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tokio::sync::mpsc;

mod postgres_config;
use postgres_config::PostgresConfig;

mod pull_batch;
use pull_batch::PullBatchSize;

pub use crate::domain::resource_map::PullBatchStrategy;

mod postgres_tasks;
use postgres_tasks::postgres_pull_task::pull_task;
use postgres_tasks::postgres_sanitize_task::sanitize_task;

mod postgres_connection;
use crate::domain::{
    resource::{
        Config, ConnectionError, PullError, PushError, Record, ResourceConnection, Rules,
        SanitizeError, SanitizedRecord,
        postgres_resource::postgres_tasks::postgres_push_task::push_task,
    },
    sanitizer::{Sanitizer, in_memory_sanitizer::InMemorySanitizer},
};
use postgres_connection::PostgresConnection;

pub(crate) use postgres_tasks::postgres_pull_task::fetch_sample_row;

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
            table: self.table.clone(),
            config: self.clone(),
        })
    }
}

#[async_trait::async_trait]
impl ResourceConnection<PostgresConfig, InMemorySanitizer> for PostgresConnection {
    async fn pull(
        &self,
        tx: mpsc::Sender<Vec<Record>>,
        rules: Rules,
        strategy: PullBatchStrategy,
    ) -> Result<PullBatchStrategy, PullError> {
        pull_task(
            self.resource_id,
            strategy,
            self.table.clone(),
            self.pull_batch_size.clone(),
            self.pool.clone(),
            rules,
            tx,
        )
        .await
    }
    async fn sanitize(
        &self,
        tx: mpsc::Sender<Vec<SanitizedRecord>>,
        rx: mpsc::Receiver<Vec<Record>>,
        rules: Rules,
        sanitizer: InMemorySanitizer,
    ) -> Result<(), SanitizeError> {
        let sanitizer: Box<dyn Sanitizer> = Box::new(sanitizer);
        sanitize_task(tx, rx, rules, sanitizer).await
    }

    async fn push(
        &self,
        rules: Rules,
        rx: mpsc::Receiver<Vec<SanitizedRecord>>,
    ) -> Result<(), PushError> {
        push_task(self.table.clone(), self.pool.clone(), rules, rx).await
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
