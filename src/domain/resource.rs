use std::{marker::PhantomData, ops::Deref};

use errors::*;
use serde_json::{Map, Value};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::domain::sanitizer::Sanitizer;

pub mod postgres_resource;

pub struct Resource<T, S>
where
    T: Config<T, S>,
    S: Sanitizer,
{
    id: Uuid,
    allow_upstream: bool,
    allow_downstream: bool,
    config: T,
    _marker: PhantomData<S>,
}

#[async_trait::async_trait]
pub trait Config<T: Config<T, S>, S: Sanitizer>: Clone {
    type Connection;
    async fn connect(&mut self) -> Result<Self::Connection, ConnectionError>
    where
        Self::Connection: ResourceConnection<T, S>;

    async fn sync_up_config(&self) -> Result<(), SyncUpConfigError>;
}

/// Flat JSON object: `{ "column": value, ... }`
/// TODO: We will get rid of Value and introduce normalized properties
/// e.g. Record { items: Vec<NormalizedProperties>} 
/// Enum NormalizedProperties { String(string), I64(i64), Decimal(points, value) etc..}
pub struct Record(Value);

impl Deref for Record {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Record {
    pub fn get_identifier_val(&self, rules: &Rules) -> Value {
        let field = rules
            .get_source_identifier()
            .get_field_identifer_as_string();
        self.get_field_value(&field).cloned().unwrap_or(Value::Null)
    }

    pub fn fields(&self) -> Map<String, Value> {
        self.as_object().cloned().unwrap_or_default()
    }

    pub fn get_field_value(&self, field_name: &str) -> Option<&Value> {
        self.get(field_name)
    }
}

#[derive(Clone)]
pub enum NamingConvention {
    CamelCase,
    PascalCase,
    SnakeCase,
    ScreamingCase,
    KebabCase,
    TrainCase,
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct FieldRecord {
    sanitize_type: SanitizeType,
    field_name: String,
}

impl FieldRecord {
    pub fn get_field_name(&self) -> &str {
        &self.field_name
    }

    pub fn get_sanitize_type(&self) -> &SanitizeType {
        &self.sanitize_type
    }
}
pub struct SanitizedRecord(Value);

impl SanitizedRecord {
    pub fn new(value: Value) -> Self {
        Self(value)
    }
}

impl Deref for SanitizedRecord {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum SanitizeType {
    Name,
    Email,
    PhoneNumber,
    Ssn,
}

#[derive(Clone, Hash, PartialEq, Eq)]
// support only String for now
pub enum FieldIdentifierKey {
    String(String),
}

impl FieldIdentifierKey {
    pub fn get_field_identifer_as_string(&self) -> String {
        match self {
            FieldIdentifierKey::String(val) => val.to_owned(),
        }
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct FieldIdentifierValue(String);

impl FieldIdentifierValue {
    pub fn new(rules: &Rules, record: &Record) -> Self {
        let value = record.get_identifier_val(rules);
        Self(Self::to_cache_key(value))
    }

    fn to_cache_key(value: Value) -> String {
        match value {
            Value::Null => String::new(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            Value::String(s) => s,
            other => other.to_string(),
        }
    }
}

#[derive(Clone)]
// We can turn this into Arc to avoid Clone
pub struct Rules {
    rule_id: Uuid,
    force_pull_from_start: bool,
    source_naming_convention: NamingConvention,
    target_naming_convention: NamingConvention,
    source_sanitize_fields: Vec<FieldRecord>,
    source_identifier: FieldIdentifierKey,
}

impl Rules {
    pub fn get_source_field(&self) -> &Vec<FieldRecord> {
        &self.source_sanitize_fields
    }
    pub fn get_rule_id(&self) -> &Uuid {
        &self.rule_id
    }

    pub fn get_source_identifier(&self) -> &FieldIdentifierKey {
        &self.source_identifier
    }
}

#[async_trait::async_trait]
pub trait ResourceConnection<T, S: Sanitizer> {
    async fn pull(&mut self, tx: mpsc::Sender<Vec<Record>>, rules: Rules) -> Result<T, PullError>;
    async fn sanitize(
        &mut self,
        tx: mpsc::Sender<Vec<SanitizedRecord>>,
        rx: mpsc::Receiver<Vec<Record>>,
        rules: Rules,
        sanitizer: S,
    ) -> Result<(), SanitizeError>;
    async fn push(
        &mut self,
        rules: Rules,
        rx: mpsc::Receiver<Vec<SanitizedRecord>>,
    ) -> Result<(), PushError>;
}

mod errors {
    pub enum PullError {
        FailedToGetPointer,
        FailedToExtractData,
        UnexpectedFailed,
    }
    pub enum PushError {
        UnexpectedFailed,
    }
    pub enum SanitizeError {
        SanitizerFailed,
        UnexpectedFailed,
    }

    pub enum ConnectionError {
        FailedToConnect,
    }

    pub enum SyncUpConfigError {}
}
