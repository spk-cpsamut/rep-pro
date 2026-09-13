use uuid::Uuid;
use errors::*;

use crate::domain::resource::postgres_resource::{
    DatabaseName, Password, PullBatchStrategy, Username,
};
#[derive(Clone)]
pub struct PostgresConfig {
    pub resource_id: Uuid,
    pub host: Host,
    pub port: u16,
    pub username: Username,
    pub password: Password,
    pub database_name: DatabaseName,
    pub table: String,
    pub pull_batch_strategy: PullBatchStrategy,
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

mod errors {
    pub enum HostError {
        UnexpectedDomain,
    }
}
