use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use sqlx::{Column, PgPool, Postgres, QueryBuilder, Row, postgres::PgPoolOptions};
use tokio::sync::mpsc;

use crate::domain::resource::{
    Config, ConnectionError, PullError, PushError, Record, ResourceConnection, Rules,
    SanitizeError, SanitizedRecord,
};
use std::u32;

pub struct PostgresConfig {
    host: Host,
    port: Port,
    username: Username,
    password: Password,
    database_name: DatabaseName,
    table: String,
    pull_batch_startegy: PullBatchStartegy,
}

impl PostgresConfig {
    pub fn new(
        host: Host,
        port: Port,
        username: Username,
        password: Password,
        database_name: DatabaseName,
        table: String,
        pull_batch_startegy: PullBatchStartegy,
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
            self.port.0,
            self.database_name.as_ref()
        );

        let pool = get_postgres_pool(&database_url)
            .await
            .map_err(|_| ConnectionError::FailedToConnect)?;
        Ok(PostgresConnection {
            pool,
            pull_batch_size: PullBatchSize::parse(100000),
            sanitize_batch_size: PullBatchSize::parse(100000),
            pull_batch_startegy: self.pull_batch_startegy.clone(),
            table: self.table.clone(),
        })
    }
}

#[derive(Clone)]
pub enum PullBatchStartegy {
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
    pull_batch_startegy: PullBatchStartegy,
}

#[derive(Clone)]
struct PullBatchSize(String);
enum PullBatchSizeError {
    CannotConvertBackToU32,
}

impl PullBatchSize {
    pub fn parse(size: u32) -> Self {
        Self(size.to_string())
    }

    pub fn to_u32(&self) -> u32 {
        self.as_ref()
            .parse::<u32>()
            .expect("it should convert to u32 properly as the original value is u32")
    }
}

impl AsRef<str> for PullBatchSize {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[derive(Clone)]
struct SqlOffset(String);

impl SqlOffset {
    pub fn parse(size: u32) -> Self {
        Self(size.to_string())
    }
}

impl AsRef<str> for SqlOffset {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[async_trait::async_trait]
impl ResourceConnection for PostgresConnection {
    async fn pull(&mut self, tx: mpsc::Sender<Vec<Record>>, rules: Rules) -> Result<(), PullError> {
        let startegy = self.pull_batch_startegy.clone();
        let table = self.table.clone();
        let batch_size = self.pull_batch_size.clone();
        let pg_pool = self.pool.clone();

        let task = tokio::spawn(async move {
            let mut startegy = startegy;
            match startegy {
                PullBatchStartegy::Cursor { field, pointer } => {
                    let mut pointer = pointer;
                    let mut first_run = true;
                    let mut records: Vec<Record> = Vec::new();
                    loop {
                        let mut qb = QueryBuilder::<Postgres>::new(format!(
                            "SELECT * FROM {} WHERE 1=1",
                            table
                        ));

                        if let Some(p) = &pointer
                            && !(rules.force_pull_from_start && first_run)
                        {
                            qb.push(format!("AND {} >", &field));
                            qb.push_bind(p);
                        }

                        qb.push(format!(" ORDER BY {} LIMIT", &field));
                        qb.push_bind(batch_size.as_ref());

                        let rows = qb.build().fetch_all(&pg_pool).await.unwrap();

                        if rows.is_empty() {
                            todo!("write pointer back to database");
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
                                    .unwrap()
                                    .push(json!({"field": column.name(), "value": value}));
                            }
                            records.push(Record(row_json));
                        }

                        let got = rows.len();

                        if got < batch_size.to_u32() as usize {
                            todo!("write pointer back to database");
                            break;
                        }

                        first_run = false;
                    }
                }
                PullBatchStartegy::LimitOffSet { field, offset } => {
                    todo!("To support later")
                }
            }

            Ok::<(), PullError>(())
        });
        task.await.unwrap();
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
enum HostError {
    UnexpectedDomain,
}

pub struct Host(String);

impl Host {
    pub fn parse(host: String) -> Result<Self, HostError> {
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

pub struct Port(u16);

impl Port {
    pub fn parse(port: u16) -> Self {
        Port(port)
    }
}

enum UsernameError {}

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
enum PasswordError {}
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

enum DatabaseNameError {}
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
