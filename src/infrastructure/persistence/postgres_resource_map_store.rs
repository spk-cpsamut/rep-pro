use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{
    resource::SyncUpConfigError,
    resource_map::{PairConfig, ResourceMapStore},
};

pub struct PostgresResourceMapStore {
    pool: PgPool,
}

impl PostgresResourceMapStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl ResourceMapStore for PostgresResourceMapStore {
    async fn update_pair_config(
        &self,
        resource_map_id: Uuid,
        pair_config: &PairConfig,
    ) -> Result<(), SyncUpConfigError> {
        let result = sqlx::query(
            "UPDATE resource_maps SET pair_config = $1, updated_at = NOW() WHERE id = $2",
        )
        .bind(sqlx::types::Json(pair_config.to_json()))
        .bind(resource_map_id)
        .execute(&self.pool)
        .await
        .map_err(|_| SyncUpConfigError::PersistFailed)?;

        if result.rows_affected() == 0 {
            return Err(SyncUpConfigError::MapNotFound);
        }

        Ok(())
    }
}
