use std::{marker::PhantomData, ops::Deref};

use serde_json::{Map, Value};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::domain::{resource_map::PullBatchStrategy, sanitizer::Sanitizer};

pub mod postgres_resource;

pub mod dao;
mod handler_inner;
pub(crate) mod handlers;
mod models;

pub use errors::{ConnectionError, PullError, PushError, SanitizeError, SyncUpConfigError};
pub(crate) use handler_inner::{connect_resource, load_postgres_resource};

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

impl<T, S> Resource<T, S>
where
    T: Config<T, S>,
    S: Sanitizer,
{
    pub fn new(id: Uuid, allow_upstream: bool, allow_downstream: bool, config: T) -> Self {
        Self {
            id,
            allow_upstream,
            allow_downstream,
            config,
            _marker: PhantomData,
        }
    }

    pub fn allow_upstream(&self) -> bool {
        self.allow_upstream
    }

    pub fn allow_downstream(&self) -> bool {
        self.allow_downstream
    }

    pub async fn connect(&mut self) -> Result<T::Connection, ConnectionError>
    where
        T::Connection: ResourceConnection<T, S>,
    {
        self.config.connect().await
    }
}

impl<T, S> Clone for Resource<T, S>
where
    T: Config<T, S>,
    S: Sanitizer,
{
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            allow_upstream: self.allow_upstream,
            allow_downstream: self.allow_downstream,
            config: self.config.clone(),
            _marker: PhantomData,
        }
    }
}

#[async_trait::async_trait]
pub trait Config<T: Config<T, S>, S: Sanitizer>: Clone {
    type Connection;
    async fn connect(&mut self) -> Result<Self::Connection, ConnectionError>
    where
        Self::Connection: ResourceConnection<T, S>;
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
    pub(crate) fn from_value(value: Value) -> Self {
        Self(value)
    }

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

impl NamingConvention {
    pub(crate) fn parse(value: &str) -> Result<Self, ()> {
        match value {
            "camel_case" => Ok(Self::CamelCase),
            "pascal_case" => Ok(Self::PascalCase),
            "snake_case" => Ok(Self::SnakeCase),
            "screaming_case" => Ok(Self::ScreamingCase),
            "kebab_case" => Ok(Self::KebabCase),
            "train_case" => Ok(Self::TrainCase),
            _ => Err(()),
        }
    }
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

impl SanitizeType {
    pub(crate) fn parse(value: &str) -> Result<Self, ()> {
        match value {
            "name" => Ok(Self::Name),
            "email" => Ok(Self::Email),
            "phone_number" => Ok(Self::PhoneNumber),
            "ssn" => Ok(Self::Ssn),
            _ => Err(()),
        }
    }
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
    pub(crate) fn from_stored(rule_id: Uuid, value: &Value) -> Result<Self, ()> {
        let force_pull_from_start = value
            .get("force_pull_from_start")
            .and_then(Value::as_bool)
            .ok_or(())?;
        let source_naming_convention = NamingConvention::parse(
            value
                .get("source_naming_convention")
                .and_then(Value::as_str)
                .ok_or(())?,
        )?;
        let target_naming_convention = NamingConvention::parse(
            value
                .get("target_naming_convention")
                .and_then(Value::as_str)
                .ok_or(())?,
        )?;
        let identifier = value
            .get("source_identifier")
            .and_then(Value::as_str)
            .ok_or(())?;
        let fields = value
            .get("source_sanitize_fields")
            .and_then(Value::as_array)
            .ok_or(())?;
        let mut source_sanitize_fields = Vec::with_capacity(fields.len());
        for field in fields {
            let field_name = field
                .get("field_name")
                .and_then(Value::as_str)
                .ok_or(())?
                .to_owned();
            let sanitize_type = SanitizeType::parse(
                field
                    .get("sanitize_type")
                    .and_then(Value::as_str)
                    .ok_or(())?,
            )?;
            source_sanitize_fields.push(FieldRecord {
                sanitize_type,
                field_name,
            });
        }
        Ok(Self {
            rule_id,
            force_pull_from_start,
            source_naming_convention,
            target_naming_convention,
            source_sanitize_fields,
            source_identifier: FieldIdentifierKey::String(identifier.to_owned()),
        })
    }

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
    async fn pull(
        &self,
        tx: mpsc::Sender<Vec<Record>>,
        rules: Rules,
        strategy: PullBatchStrategy,
    ) -> Result<PullBatchStrategy, PullError>;
    async fn sanitize(
        &self,
        tx: mpsc::Sender<Vec<SanitizedRecord>>,
        rx: mpsc::Receiver<Vec<Record>>,
        rules: Rules,
        sanitizer: S,
    ) -> Result<(), SanitizeError>;
    async fn push(
        &self,
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

    pub enum SyncUpConfigError {
        PersistFailed,
        MapNotFound,
    }
}
