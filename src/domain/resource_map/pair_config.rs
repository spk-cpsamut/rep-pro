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

pub enum PairConfigJsonError {
    InvalidJson,
    UnknownStrategy,
    InvalidCheckpoint,
}
