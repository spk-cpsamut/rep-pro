use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const CURSOR: &str = "cursor";
pub const LIMIT_OFFSET: &str = "LimitOffSet";

#[derive(Clone)]
pub enum PullBatchStrategy {
    Cursor {
        field: String,
        pointer: Option<String>,
    },
    LimitOffSet {
        field: String,
        offset: Option<SqlOffset>,
    },
}

#[derive(Clone)]
pub struct SqlOffset(u32);

impl SqlOffset {
    pub fn new(size: u32) -> Self {
        Self(size)
    }

    pub fn get(&self) -> u32 {
        self.0
    }
}

#[derive(Clone)]
pub struct PairConfig {
    pull_batch_strategy: PullBatchStrategy,
}

impl PairConfig {
    pub fn new(pull_batch_strategy: PullBatchStrategy) -> Self {
        Self {
            pull_batch_strategy,
        }
    }

    pub fn pull_batch_strategy(&self) -> &PullBatchStrategy {
        &self.pull_batch_strategy
    }

    pub fn set_pull_strategy(&mut self, strategy: PullBatchStrategy) {
        self.pull_batch_strategy = strategy;
    }

    pub fn to_json(&self) -> Value {
        serde_json::to_value(PairConfigJson::from(self))
            .expect("pair_config json encoding is infallible")
    }

    pub fn from_json(value: Value) -> Result<Self, PairConfigJsonError> {
        let json: PairConfigJson =
            serde_json::from_value(value).map_err(|_| PairConfigJsonError::InvalidJson)?;
        Self::try_from(json)
    }
}

#[derive(Serialize, Deserialize)]
pub struct PairConfigJson {
    pull_batch_strategy: String,
    pull_batch_field: String,
    pull_batch_checkpoint: Option<String>,
}

impl From<&PairConfig> for PairConfigJson {
    fn from(value: &PairConfig) -> Self {
        let (plan_strategy, field, checkpoint) = match &value.pull_batch_strategy {
            PullBatchStrategy::Cursor { field, pointer } => {
                (CURSOR.to_owned(), field.clone(), pointer.clone())
            }
            PullBatchStrategy::LimitOffSet { field, offset } => {
                let checkpoint = offset
                    .as_ref()
                    .map(|sql_offset| sql_offset.get().to_string());
                (LIMIT_OFFSET.to_owned(), field.clone(), checkpoint)
            }
        };
        Self {
            pull_batch_strategy: plan_strategy,
            pull_batch_field: field,
            pull_batch_checkpoint: checkpoint,
        }
    }
}

impl TryFrom<PairConfigJson> for PairConfig {
    type Error = PairConfigJsonError;

    fn try_from(value: PairConfigJson) -> Result<Self, Self::Error> {
        let strategy = match value.pull_batch_strategy.as_str() {
            CURSOR => PullBatchStrategy::Cursor {
                field: value.pull_batch_field,
                pointer: value.pull_batch_checkpoint,
            },
            LIMIT_OFFSET => {
                let offset = match value.pull_batch_checkpoint {
                    Some(raw) => {
                        let parsed = raw
                            .parse::<u32>()
                            .map_err(|_| PairConfigJsonError::InvalidCheckpoint)?;
                        Some(SqlOffset::new(parsed))
                    }
                    None => None,
                };
                PullBatchStrategy::LimitOffSet {
                    field: value.pull_batch_field,
                    offset,
                }
            }
            _ => return Err(PairConfigJsonError::UnknownStrategy),
        };
        Ok(PairConfig::new(strategy))
    }
}

#[derive(Debug)]
pub enum PairConfigJsonError {
    InvalidJson,
    UnknownStrategy,
    InvalidCheckpoint,
}

impl std::fmt::Display for PairConfigJsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson => f.write_str("invalid pair config"),
            Self::UnknownStrategy => f.write_str("unknown pull batch strategy"),
            Self::InvalidCheckpoint => f.write_str("invalid pull batch checkpoint"),
        }
    }
}

impl std::error::Error for PairConfigJsonError {}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{PairConfig, PairConfigJsonError, PullBatchStrategy};

    #[test]
    fn rejects_json_that_is_not_a_pair_config() {
        assert!(matches!(
            PairConfig::from_json(json!("cursor")),
            Err(PairConfigJsonError::InvalidJson)
        ));
        assert!(matches!(
            PairConfig::from_json(json!({})),
            Err(PairConfigJsonError::InvalidJson)
        ));
    }

    #[test]
    fn round_trips_cursor_with_and_without_a_checkpoint() {
        for checkpoint in [json!(null), json!("42")] {
            let value = json!({
                "pull_batch_strategy": "cursor",
                "pull_batch_field": "id",
                "pull_batch_checkpoint": checkpoint,
            });
            let Ok(pair) = PairConfig::from_json(value.clone()) else {
                panic!("cursor pair config");
            };
            assert_eq!(pair.to_json(), value);
            match pair.pull_batch_strategy() {
                PullBatchStrategy::Cursor { field, pointer } => {
                    assert_eq!(field, "id");
                    assert_eq!(pointer.as_deref(), checkpoint.as_str());
                }
                PullBatchStrategy::LimitOffSet { .. } => panic!("expected cursor"),
            }
        }
    }

    #[test]
    fn round_trips_limit_offset_with_and_without_a_checkpoint() {
        let open = json!({
            "pull_batch_strategy": "LimitOffSet",
            "pull_batch_field": "id",
            "pull_batch_checkpoint": null,
        });
        let Ok(pair) = PairConfig::from_json(open.clone()) else {
            panic!("open limit offset");
        };
        assert_eq!(pair.to_json(), open);
        match pair.pull_batch_strategy() {
            PullBatchStrategy::LimitOffSet { field, offset } => {
                assert_eq!(field, "id");
                assert!(offset.is_none());
            }
            PullBatchStrategy::Cursor { .. } => panic!("expected limit offset"),
        }

        let saved = json!({
            "pull_batch_strategy": "LimitOffSet",
            "pull_batch_field": "id",
            "pull_batch_checkpoint": "12",
        });
        let Ok(pair) = PairConfig::from_json(saved.clone()) else {
            panic!("saved limit offset");
        };
        assert_eq!(pair.to_json(), saved);
        match pair.pull_batch_strategy() {
            PullBatchStrategy::LimitOffSet { field, offset } => {
                assert_eq!(field, "id");
                assert_eq!(offset.as_ref().unwrap().get(), 12);
            }
            PullBatchStrategy::Cursor { .. } => panic!("expected limit offset"),
        }
    }

    #[test]
    fn rejects_an_unknown_strategy_and_a_bad_checkpoint() {
        let unknown = json!({
            "pull_batch_strategy": "offset",
            "pull_batch_field": "id",
            "pull_batch_checkpoint": null,
        });
        assert!(matches!(
            PairConfig::from_json(unknown),
            Err(PairConfigJsonError::UnknownStrategy)
        ));

        let bad_checkpoint = json!({
            "pull_batch_strategy": "LimitOffSet",
            "pull_batch_field": "id",
            "pull_batch_checkpoint": "nope",
        });
        assert!(matches!(
            PairConfig::from_json(bad_checkpoint),
            Err(PairConfigJsonError::InvalidCheckpoint)
        ));
    }
}
