use tokio::sync::mpsc;

use crate::domain::{
    resource::{Config, Record, Resource, ResourceConnection, Rules, SanitizedRecord},
    resource_map::{PairConfig, ResourceMap, ResourceMapStore},
    sanitizer::Sanitizer,
};

pub struct ParallelOrchestrator<T1, S1, T2, S2, Store>
where
    T1: Config<T1, S1>,
    S1: Sanitizer,
    T2: Config<T2, S2>,
    S2: Sanitizer,
    Store: ResourceMapStore,
{
    source: Resource<T1, S1>,
    target: Resource<T2, S2>,
    resource_map: ResourceMap,
    store: Store,
}

pub enum OrchestratorError {
    DownstreamNotAllowed,
    UpstreamNotAllowed,
    ConnectionFailed,
    PullFailed,
    SanitizeFailed,
    PushFailed,
    SyncUpConfigFailed,
}

impl<T1, S1, T2, S2, Store> ParallelOrchestrator<T1, S1, T2, S2, Store>
where
    T1: Config<T1, S1>,
    S1: Sanitizer,
    T2: Config<T2, S2>,
    S2: Sanitizer,
    Store: ResourceMapStore,
{
    pub fn new(
        source: Resource<T1, S1>,
        target: Resource<T2, S2>,
        resource_map: ResourceMap,
        store: Store,
    ) -> Self {
        Self {
            source,
            target,
            resource_map,
            store,
        }
    }

    pub async fn run(
        &mut self,
        rules: Rules,
        sanitizer: S1,
    ) -> Result<(), OrchestratorError>
    where
        T1::Connection: ResourceConnection<T1, S1>,
        T2::Connection: ResourceConnection<T2, S2>,
    {
        if !self.source.allow_downstream() {
            return Err(OrchestratorError::DownstreamNotAllowed);
        }
        if !self.target.allow_upstream() {
            return Err(OrchestratorError::UpstreamNotAllowed);
        }

        let source_conn = self
            .source
            .connect()
            .await
            .map_err(|_| OrchestratorError::ConnectionFailed)?;
        let target_conn = self
            .target
            .connect()
            .await
            .map_err(|_| OrchestratorError::ConnectionFailed)?;

        let (raw_tx, raw_rx) = mpsc::channel::<Vec<Record>>(8);
        let (clean_tx, clean_rx) = mpsc::channel::<Vec<SanitizedRecord>>(8);

        let strategy = self.resource_map.pair_config().pull_batch_strategy().clone();

        let (updated_strategy, _, _) = tokio::try_join!(
            async {
                source_conn
                    .pull(raw_tx, rules.clone(), strategy)
                    .await
                    .map_err(|_| OrchestratorError::PullFailed)
            },
            async {
                source_conn
                    .sanitize(clean_tx, raw_rx, rules.clone(), sanitizer)
                    .await
                    .map_err(|_| OrchestratorError::SanitizeFailed)
            },
            async {
                target_conn
                    .push(rules.clone(), clean_rx)
                    .await
                    .map_err(|_| OrchestratorError::PushFailed)
            },
        )?;

        let mut updated_pair_config = self.resource_map.pair_config().clone();
        updated_pair_config.set_pull_strategy(updated_strategy);

        self.resource_map
            .sync_up_config(&self.store, updated_pair_config.clone())
            .await
            .map_err(|_| OrchestratorError::SyncUpConfigFailed)?;

        Ok(())
    }
}
