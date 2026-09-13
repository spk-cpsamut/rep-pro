use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::resource::postgres_resource::{postgres_config::PostgresConfig, pull_batch::{PullBatchSize, PullBatchStrategy}};

// TODO: add push_strategy to handle edge cases such as pull and push checkpoints are different
// using the same checkpoint is good for now
pub struct PostgresConnection {
    pub resource_id: Uuid,
    pub pool: PgPool,
    pub table: String,
    pub pull_batch_size: PullBatchSize,
    pub sanitize_batch_size: PullBatchSize,
    pub pull_batch_strategy: PullBatchStrategy,
    pub config: PostgresConfig ,
}