use errors::*;
use serde_json::Value;
use tokio::sync::mpsc;

pub mod postgres_resource;

pub struct Resource<T>
where
    T: Config,
{
    allow_upstream: bool,
    allow_downstream: bool,
    config: T,
}

pub enum ConnectionError {
    FailedToConnect,
}

#[async_trait::async_trait]
trait Config {
    type Connection;
    async fn connect(&mut self) -> Result<Self::Connection, ConnectionError>
    where
        Self::Connection: ResourceConnection;
}

// we can replace Value with vec of field & value
pub struct Record(Value);

pub enum NamingConvention {
    CamelCase,
    PascalCase,
    SnakeCase,
    ScreamingCase,
    KebabCase,
    TrainCase,
}

pub struct FieldRecord {
    sanitize_type: SanitizeType,
    field_name: String,
    identifier: String,
}
pub struct SanitizedRecord {}

pub enum SanitizeType {}

pub struct Rules {
    force_pull_from_start: bool,
    source_naming_convention: NamingConvention,
    target_naming_convention: NamingConvention,
    source_sanitize_fields: Vec<FieldRecord>,
}

#[async_trait::async_trait]
trait ResourceConnection {
    async fn pull(&mut self, tx: mpsc::Sender<Vec<Record>>, rules: Rules) -> Result<(), PullError>;
    async fn sanitize(
        &mut self,
        tx: mpsc::Sender<Vec<SanitizedRecord>>,
        rx: mpsc::Receiver<Vec<Record>>,
        rules: Rules,
    ) -> Result<(), SanitizeError>;
    async fn push(&mut self, rx: mpsc::Receiver<Vec<SanitizedRecord>>) -> Result<(), PushError>;
}

mod errors {
    pub enum PullError {
        FailedToGetPointer,
        FailedToExtactData,
    }
    pub enum PushError {}
    pub enum SanitizeError {}
}
