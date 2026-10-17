use tokio::sync::mpsc;

use crate::domain::{
    resource::{Config, Record, Resource, ResourceConnection, Rules, SanitizedRecord},
    resource_map::{PairConfig, PullBatchStrategy, ResourceMap, ResourceMapStore},
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

#[derive(Debug)]
pub enum OrchestratorError {
    DownstreamNotAllowed,
    UpstreamNotAllowed,
    SourceConnectionFailed,
    TargetConnectionFailed,
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

    #[tracing::instrument(
        skip(self, rules, sanitizer),
        fields(
            resource_map_id = %self.resource_map.id(),
            resource_source = %self.resource_map.resource_source(),
            resource_target = %self.resource_map.resource_target(),
            table_source = self.resource_map.table_source(),
            table_target = self.resource_map.table_target(),
            checkpoint_field = tracing::field::Empty,
        )
    )]
    pub async fn run(&mut self, rules: Rules, sanitizer: S1) -> Result<(), OrchestratorError>
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

        let source_conn = self.source.connect().await.map_err(|err| {
            tracing::error!(error = ?err, "source connection failed");
            OrchestratorError::SourceConnectionFailed
        })?;
        tracing::info!("source connected");
        let target_conn = self.target.connect().await.map_err(|err| {
            tracing::error!(error = ?err, "target connection failed");
            OrchestratorError::TargetConnectionFailed
        })?;
        tracing::info!("target connected");

        let (raw_tx, raw_rx) = mpsc::channel::<Vec<Record>>(8);
        let (clean_tx, clean_rx) = mpsc::channel::<Vec<SanitizedRecord>>(8);

        let strategy = self
            .resource_map
            .pair_config()
            .pull_batch_strategy()
            .clone();
        let field_name = checkpoint_field(&strategy).to_owned();
        tracing::Span::current().record("checkpoint_field", field_name.as_str());

        let (updated_strategy, _, _) = tokio::try_join!(
            async {
                tracing::info!(checkpoint_field = %field_name, "pull started");
                let updated = source_conn
                    .pull(raw_tx, rules.clone(), strategy)
                    .await
                    .map_err(|err| {
                        tracing::error!(error = ?err, "pull failed");
                        OrchestratorError::PullFailed
                    })?;
                tracing::info!("pull finished");
                Ok(updated)
            },
            async {
                tracing::info!("sanitize started");
                source_conn
                    .sanitize(clean_tx, raw_rx, rules.clone(), sanitizer)
                    .await
                    .map_err(|err| {
                        tracing::error!(error = ?err, "sanitize failed");
                        OrchestratorError::SanitizeFailed
                    })?;
                tracing::info!("sanitize finished");
                Ok(())
            },
            async {
                tracing::info!("push started");
                target_conn
                    .push(rules.clone(), clean_rx)
                    .await
                    .map_err(|err| {
                        tracing::error!(error = ?err, "push failed");
                        OrchestratorError::PushFailed
                    })?;
                tracing::info!("push finished");
                Ok(())
            },
        )?;

        self.resource_map
            .sync_up_config(&self.store, PairConfig::new(updated_strategy.clone()))
            .await
            .map_err(|err| {
                tracing::error!(error = ?err, "checkpoint sync failed");
                OrchestratorError::SyncUpConfigFailed
            })?;
        tracing::info!(
            checkpoint_field = %checkpoint_field(&updated_strategy),
            "checkpoint synced"
        );

        Ok(())
    }
}

fn checkpoint_field(strategy: &PullBatchStrategy) -> &str {
    match strategy {
        PullBatchStrategy::Cursor { field, .. } | PullBatchStrategy::LimitOffSet { field, .. } => {
            field
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use serde_json::{Value, json};
    use tokio::sync::mpsc;
    use uuid::Uuid;

    use super::{OrchestratorError, ParallelOrchestrator};
    use crate::domain::resource::{
        Config, ConnectionError, PullError, PushError, Record, Resource, ResourceConnection, Rules,
        SanitizeError, SanitizedRecord, SyncUpConfigError,
    };
    use crate::domain::resource_map::{
        PairConfig, PullBatchStrategy, ResourceMap, ResourceMapStore,
    };
    use crate::domain::sanitizer::in_memory_sanitizer::{InMemorySanitizer, SanitizeStorage};

    #[derive(Clone)]
    struct FakeConfig {
        fail_connect: bool,
        pull_err: bool,
        sanitize_err: bool,
        push_err: bool,
        updated: PullBatchStrategy,
    }

    #[derive(Clone)]
    struct FakeConn {
        pull_err: bool,
        sanitize_err: bool,
        push_err: bool,
        updated: PullBatchStrategy,
    }

    #[async_trait::async_trait]
    impl Config<FakeConfig, InMemorySanitizer> for FakeConfig {
        type Connection = FakeConn;

        async fn connect(&mut self) -> Result<FakeConn, ConnectionError> {
            if self.fail_connect {
                return Err(ConnectionError::FailedToConnect);
            }
            Ok(FakeConn {
                pull_err: self.pull_err,
                sanitize_err: self.sanitize_err,
                push_err: self.push_err,
                updated: self.updated.clone(),
            })
        }
    }

    #[async_trait::async_trait]
    impl ResourceConnection<FakeConfig, InMemorySanitizer> for FakeConn {
        async fn pull(
            &self,
            _tx: mpsc::Sender<Vec<Record>>,
            _rules: Rules,
            _strategy: PullBatchStrategy,
        ) -> Result<PullBatchStrategy, PullError> {
            if self.pull_err {
                Err(PullError::UnexpectedFailed)
            } else {
                Ok(self.updated.clone())
            }
        }

        async fn sanitize(
            &self,
            _tx: mpsc::Sender<Vec<SanitizedRecord>>,
            _rx: mpsc::Receiver<Vec<Record>>,
            _rules: Rules,
            _sanitizer: InMemorySanitizer,
        ) -> Result<(), SanitizeError> {
            if self.sanitize_err {
                Err(SanitizeError::SanitizerFailed)
            } else {
                Ok(())
            }
        }

        async fn push(
            &self,
            _rules: Rules,
            _rx: mpsc::Receiver<Vec<SanitizedRecord>>,
        ) -> Result<(), PushError> {
            if self.push_err {
                Err(PushError::UnexpectedFailed)
            } else {
                Ok(())
            }
        }
    }

    #[derive(Clone, Copy)]
    enum StoreFail {
        Persist,
    }

    #[derive(Clone)]
    struct FakeStore {
        fail: Option<StoreFail>,
        saved: Arc<Mutex<Option<Value>>>,
    }

    #[async_trait::async_trait]
    impl ResourceMapStore for FakeStore {
        async fn update_pair_config(
            &self,
            _resource_map_id: Uuid,
            pair_config: &PairConfig,
        ) -> Result<(), SyncUpConfigError> {
            match self.fail {
                Some(StoreFail::Persist) => Err(SyncUpConfigError::PersistFailed),
                None => {
                    *self.saved.lock().expect("saved config") = Some(pair_config.to_json());
                    Ok(())
                }
            }
        }
    }

    struct Case {
        allow_down: bool,
        allow_up: bool,
        source_connect_fails: bool,
        target_connect_fails: bool,
        pull_fails: bool,
        sanitize_fails: bool,
        push_fails: bool,
        store_fails: bool,
    }

    impl Default for Case {
        fn default() -> Self {
            Self {
                allow_down: true,
                allow_up: true,
                source_connect_fails: false,
                target_connect_fails: false,
                pull_fails: false,
                sanitize_fails: false,
                push_fails: false,
                store_fails: false,
            }
        }
    }

    fn rules() -> Rules {
        Rules::from_stored(
            Uuid::from_u128(1),
            Uuid::from_u128(2),
            &json!({
                "force_pull_from_start": false,
                "source_naming_convention": "snake_case",
                "target_naming_convention": "snake_case",
                "source_identifier": "id",
                "source_sanitize_fields": []
            }),
        )
        .unwrap()
    }

    fn updated_strategy() -> PullBatchStrategy {
        PullBatchStrategy::Cursor {
            field: "id".into(),
            pointer: Some("7".into()),
        }
    }

    async fn run_case(case: Case) -> Result<Value, OrchestratorError> {
        let updated = updated_strategy();
        let source = Resource::new(
            Uuid::nil(),
            true,
            case.allow_down,
            FakeConfig {
                fail_connect: case.source_connect_fails,
                pull_err: case.pull_fails,
                sanitize_err: case.sanitize_fails,
                push_err: false,
                updated: updated.clone(),
            },
        );
        let target = Resource::new(
            Uuid::from_u128(1),
            case.allow_up,
            true,
            FakeConfig {
                fail_connect: case.target_connect_fails,
                pull_err: false,
                sanitize_err: false,
                push_err: case.push_fails,
                updated,
            },
        );
        let resource_map = ResourceMap::new(
            Uuid::from_u128(3),
            Uuid::nil(),
            Uuid::from_u128(1),
            "users".into(),
            "users_out".into(),
            PairConfig::new(PullBatchStrategy::Cursor {
                field: "id".into(),
                pointer: None,
            }),
        );
        let saved = Arc::new(Mutex::new(None));
        let store = FakeStore {
            fail: case.store_fails.then_some(StoreFail::Persist),
            saved: saved.clone(),
        };
        let sanitizer = InMemorySanitizer::new(Mutex::new(SanitizeStorage {
            map: HashMap::new(),
        }));
        ParallelOrchestrator::new(source, target, resource_map, store)
            .run(rules(), sanitizer)
            .await?;
        Ok(saved.lock().expect("saved config").clone().unwrap())
    }

    #[tokio::test]
    async fn each_failure_is_reported_before_later_steps() {
        let mut case = Case::default();
        case.allow_down = false;
        assert!(matches!(
            run_case(case).await.unwrap_err(),
            OrchestratorError::DownstreamNotAllowed
        ));

        let mut case = Case::default();
        case.allow_up = false;
        assert!(matches!(
            run_case(case).await.unwrap_err(),
            OrchestratorError::UpstreamNotAllowed
        ));

        let mut case = Case::default();
        case.source_connect_fails = true;
        assert!(matches!(
            run_case(case).await.unwrap_err(),
            OrchestratorError::SourceConnectionFailed
        ));

        let mut case = Case::default();
        case.target_connect_fails = true;
        assert!(matches!(
            run_case(case).await.unwrap_err(),
            OrchestratorError::TargetConnectionFailed
        ));

        let mut case = Case::default();
        case.pull_fails = true;
        assert!(matches!(
            run_case(case).await.unwrap_err(),
            OrchestratorError::PullFailed
        ));

        let mut case = Case::default();
        case.sanitize_fails = true;
        assert!(matches!(
            run_case(case).await.unwrap_err(),
            OrchestratorError::SanitizeFailed
        ));

        let mut case = Case::default();
        case.push_fails = true;
        assert!(matches!(
            run_case(case).await.unwrap_err(),
            OrchestratorError::PushFailed
        ));

        let mut case = Case::default();
        case.store_fails = true;
        assert!(matches!(
            run_case(case).await.unwrap_err(),
            OrchestratorError::SyncUpConfigFailed
        ));
    }

    #[tokio::test]
    async fn a_successful_run_stores_the_strategy_returned_by_pull() {
        let saved = run_case(Case::default()).await.unwrap();
        assert_eq!(saved["pull_batch_strategy"], "cursor");
        assert_eq!(saved["pull_batch_field"], "id");
        assert_eq!(saved["pull_batch_checkpoint"], "7");
    }
}
