pub mod pair_config;

pub mod dao;
mod handler_inner;
pub(crate) mod handlers;
mod models;

pub use pair_config::{PairConfig, PullBatchStrategy, SqlOffset};

use uuid::Uuid;

use crate::domain::resource::SyncUpConfigError;

pub struct ResourceMap {
    id: Uuid,
    resource_source: Uuid,
    resource_target: Uuid,
    table_source: String,
    table_target: String,
    pair_config: PairConfig,
}

impl ResourceMap {
    pub fn new(
        id: Uuid,
        resource_source: Uuid,
        resource_target: Uuid,
        table_source: String,
        table_target: String,
        pair_config: PairConfig,
    ) -> Self {
        Self {
            id,
            resource_source,
            resource_target,
            table_source,
            table_target,
            pair_config,
        }
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn resource_source(&self) -> Uuid {
        self.resource_source
    }

    pub fn resource_target(&self) -> Uuid {
        self.resource_target
    }

    pub fn table_source(&self) -> &str {
        &self.table_source
    }

    pub fn table_target(&self) -> &str {
        &self.table_target
    }

    pub fn pair_config(&self) -> &PairConfig {
        &self.pair_config
    }

    pub async fn sync_up_config<S: ResourceMapStore>(
        &mut self,
        store: &S,
        updated: PairConfig,
    ) -> Result<(), SyncUpConfigError> {
        store.update_pair_config(self.id, &updated).await?;
        self.pair_config = updated;
        Ok(())
    }
}

#[async_trait::async_trait]
pub trait ResourceMapStore: Send + Sync {
    async fn update_pair_config(
        &self,
        resource_map_id: Uuid,
        pair_config: &PairConfig,
    ) -> Result<(), SyncUpConfigError>;
}
