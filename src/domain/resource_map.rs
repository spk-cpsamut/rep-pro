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

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{PairConfig, PullBatchStrategy, ResourceMap, ResourceMapStore};
    use crate::domain::resource::SyncUpConfigError;

    struct FakeStore {
        err: Option<SyncUpConfigError>,
    }

    #[async_trait::async_trait]
    impl ResourceMapStore for FakeStore {
        async fn update_pair_config(
            &self,
            _resource_map_id: Uuid,
            _pair_config: &PairConfig,
        ) -> Result<(), SyncUpConfigError> {
            match &self.err {
                Some(SyncUpConfigError::PersistFailed) => Err(SyncUpConfigError::PersistFailed),
                Some(SyncUpConfigError::MapNotFound) => Err(SyncUpConfigError::MapNotFound),
                None => Ok(()),
            }
        }
    }

    fn map(pointer: Option<String>) -> ResourceMap {
        ResourceMap::new(
            Uuid::from_u128(1),
            Uuid::nil(),
            Uuid::from_u128(2),
            "users".into(),
            "users_out".into(),
            PairConfig::new(PullBatchStrategy::Cursor {
                field: "id".into(),
                pointer,
            }),
        )
    }

    fn updated() -> PairConfig {
        PairConfig::new(PullBatchStrategy::Cursor {
            field: "id".into(),
            pointer: Some("9".into()),
        })
    }

    #[tokio::test]
    async fn sync_up_config_replaces_the_strategy_when_the_store_accepts_it() {
        let mut resource_map = map(None);
        assert!(
            resource_map
                .sync_up_config(&FakeStore { err: None }, updated())
                .await
                .is_ok()
        );
        assert_eq!(
            resource_map.pair_config().to_json()["pull_batch_checkpoint"],
            "9"
        );
    }

    #[tokio::test]
    async fn sync_up_config_keeps_the_strategy_when_the_store_fails() {
        for fail in [Fail::Persist, Fail::Missing] {
            let mut resource_map = map(None);
            let failed = resource_map
                .sync_up_config(
                    &FakeStore {
                        err: Some(match fail {
                            Fail::Persist => SyncUpConfigError::PersistFailed,
                            Fail::Missing => SyncUpConfigError::MapNotFound,
                        }),
                    },
                    updated(),
                )
                .await;
            match fail {
                Fail::Persist => {
                    assert!(matches!(failed, Err(SyncUpConfigError::PersistFailed)))
                }
                Fail::Missing => {
                    assert!(matches!(failed, Err(SyncUpConfigError::MapNotFound)))
                }
            }
            assert!(resource_map.pair_config().to_json()["pull_batch_checkpoint"].is_null());
        }
    }

    #[derive(Clone, Copy)]
    enum Fail {
        Persist,
        Missing,
    }
}
