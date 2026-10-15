use axum::http::StatusCode;
use serde_json::Value;
use sqlx::PgPool;
use sqlx::types::Json as SqlxJson;
use uuid::Uuid;

use super::PairConfig;
use crate::domain::http::internal_error;

#[async_trait::async_trait]
pub(crate) trait Dao: Send + Sync {
    async fn list_maps(&self) -> Result<Vec<ResourceMapRow>, sqlx::Error>;

    async fn list_rules(&self) -> Result<Vec<RuleRow>, sqlx::Error>;

    async fn begin(&self) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, sqlx::Error>;

    async fn load_flags(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: Uuid,
    ) -> Result<Option<ResourceFlags>, sqlx::Error>;

    async fn insert_map(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        resource_source: Uuid,
        resource_target: Uuid,
        table_source: &str,
        table_target: &str,
        pair: &PairConfig,
    ) -> Result<Uuid, (StatusCode, &'static str)>;

    async fn find_persistent_key(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        resource_id: Uuid,
        name: &str,
    ) -> Result<Option<Uuid>, sqlx::Error>;

    async fn insert_rule(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        resource_map_id: Uuid,
        persistent_key_id: Uuid,
        rule: Value,
    ) -> Result<Uuid, (StatusCode, &'static str)>;

    async fn commit(&self, tx: sqlx::Transaction<'_, sqlx::Postgres>) -> Result<(), sqlx::Error>;
}

#[derive(Clone)]
pub struct ResourceMapDao {
    pool: PgPool,
}

impl ResourceMapDao {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[async_trait::async_trait]
impl Dao for ResourceMapDao {
    async fn list_maps(&self) -> Result<Vec<ResourceMapRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, resource_source, resource_target, table_source, table_target, pair_config FROM resource_maps ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
    }

    async fn list_rules(&self) -> Result<Vec<RuleRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, resource_map_id, persistent_key_id, rule FROM rules ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await
    }

    async fn begin(&self) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, sqlx::Error> {
        self.pool.begin().await
    }

    async fn load_flags(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: Uuid,
    ) -> Result<Option<ResourceFlags>, sqlx::Error> {
        sqlx::query_as("SELECT allow_upstream, allow_downstream FROM resources WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
    }

    async fn insert_map(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        resource_source: Uuid,
        resource_target: Uuid,
        table_source: &str,
        table_target: &str,
        pair: &PairConfig,
    ) -> Result<Uuid, (StatusCode, &'static str)> {
        sqlx::query_scalar(
            "INSERT INTO resource_maps (resource_source, resource_target, table_source, table_target, pair_config) VALUES ($1, $2, $3, $4, $5) RETURNING id",
        )
        .bind(resource_source)
        .bind(resource_target)
        .bind(table_source)
        .bind(table_target)
        .bind(SqlxJson(pair.to_json()))
        .fetch_one(&mut **tx)
        .await
        .map_err(map_db_error)
    }

    async fn find_persistent_key(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        resource_id: Uuid,
        name: &str,
    ) -> Result<Option<Uuid>, sqlx::Error> {
        sqlx::query_scalar("SELECT id FROM persistent_keys WHERE resource_id = $1 AND name = $2")
            .bind(resource_id)
            .bind(name)
            .fetch_optional(&mut **tx)
            .await
    }

    async fn insert_rule(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        resource_map_id: Uuid,
        persistent_key_id: Uuid,
        rule: Value,
    ) -> Result<Uuid, (StatusCode, &'static str)> {
        sqlx::query_scalar(
            "INSERT INTO rules (resource_map_id, persistent_key_id, rule) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(resource_map_id)
        .bind(persistent_key_id)
        .bind(SqlxJson(rule))
        .fetch_one(&mut **tx)
        .await
        .map_err(map_db_error)
    }

    async fn commit(&self, tx: sqlx::Transaction<'_, sqlx::Postgres>) -> Result<(), sqlx::Error> {
        tx.commit().await
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct ResourceMapRow {
    pub id: Uuid,
    pub resource_source: Uuid,
    pub resource_target: Uuid,
    pub table_source: String,
    pub table_target: String,
    pub pair_config: SqlxJson<Value>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct RuleRow {
    pub id: Uuid,
    pub resource_map_id: Uuid,
    pub persistent_key_id: Uuid,
    pub rule: SqlxJson<Value>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct ResourceFlags {
    pub allow_upstream: bool,
    pub allow_downstream: bool,
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

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::map_db_error;

    #[test]
    fn a_non_database_error_is_internal() {
        let (status, message) = map_db_error(sqlx::Error::PoolClosed);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(message, "internal server error");
    }
}
