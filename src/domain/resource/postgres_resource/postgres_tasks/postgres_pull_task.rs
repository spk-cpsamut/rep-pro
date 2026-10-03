use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use rust_decimal::Decimal;
use tokio::sync::mpsc;

use serde_json::{Value, json};
use sqlx::{Column, Pool, Postgres, QueryBuilder, Row, TypeInfo, ValueRef, postgres::PgRow};
use uuid::Uuid;

use crate::domain::{
    resource::{Record, Rules, errors::PullError, postgres_resource::pull_batch::PullBatchSize},
    resource_map::PullBatchStrategy,
};

pub async fn pull_task(
    _resource_id: Uuid,
    strategy: PullBatchStrategy,
    table: String,
    batch_size: PullBatchSize,
    pg_pool: Pool<Postgres>,
    rules: Rules,
    tx: mpsc::Sender<Vec<Record>>,
) -> Result<PullBatchStrategy, PullError> {
    if !is_safe_ident(&table) {
        return Err(PullError::FailedToExtractData);
    }

    let mut updated_strategy = strategy.clone();
    match strategy {
        PullBatchStrategy::Cursor { field, pointer } => {
            if !is_safe_ident(&field) {
                return Err(PullError::FailedToExtractData);
            }

            let mut pointer = pointer;
            let mut first_run = true;
            loop {
                let mut records: Vec<Record> = Vec::new();

                let mut qb =
                    QueryBuilder::<Postgres>::new(format!("SELECT * FROM {} WHERE 1=1", table));

                if let Some(p) = &pointer
                    && !(rules.force_pull_from_start && first_run)
                {
                    qb.push(format!(" AND {} >", &field));
                    qb.push_bind(p);
                }

                qb.push(format!(" ORDER BY {} LIMIT", &field));
                qb.push_bind(batch_size.get() as i64);

                let rows = qb
                    .build()
                    .fetch_all(&pg_pool)
                    .await
                    .map_err(|_| PullError::FailedToExtractData)?;

                if rows.is_empty() {
                    break;
                }

                for row in rows.iter() {
                    let mut row_json = serde_json::Map::new();

                    for column in row.columns() {
                        let value =
                            decode_pg_value(row, column.ordinal(), column.type_info().name())
                                .map_err(|_| PullError::FailedToExtractData)?;

                        if column.name() == field.as_str() {
                            pointer = Some(json_value_to_checkpoint(&value));
                        }

                        row_json.insert(column.name().to_owned(), value);
                    }

                    if pointer.is_none() {
                        return Err(PullError::FailedToGetPointer);
                    }

                    records.push(Record(Value::Object(row_json)));
                }

                tx.send(records)
                    .await
                    .map_err(|_| PullError::UnexpectedFailed)?;

                let got = rows.len();

                if got < batch_size.get() as usize {
                    break;
                }

                first_run = false;
            }
            updated_strategy = PullBatchStrategy::Cursor { field, pointer }
        }
        PullBatchStrategy::LimitOffSet { .. } => {
            return Err(PullError::UnexpectedFailed);
        }
    }

    Ok::<PullBatchStrategy, PullError>(updated_strategy)
}

fn decode_pg_value(row: &PgRow, ordinal: usize, type_name: &str) -> Result<Value, sqlx::Error> {
    let raw = row.try_get_raw(ordinal)?;
    if raw.is_null() {
        return Ok(Value::Null);
    }

    match type_name {
        "BOOL" => Ok(Value::Bool(row.try_get::<bool, _>(ordinal)?)),
        "INT2" => Ok(json!(row.try_get::<i16, _>(ordinal)?)),
        "INT4" => Ok(json!(row.try_get::<i32, _>(ordinal)?)),
        "INT8" => Ok(json!(row.try_get::<i64, _>(ordinal)?)),
        "FLOAT4" => Ok(json!(row.try_get::<f32, _>(ordinal)?)),
        "FLOAT8" => Ok(json!(row.try_get::<f64, _>(ordinal)?)),
        // Keep decimal precision — JSON numbers are not safe for NUMERIC.
        "NUMERIC" => Ok(Value::String(
            row.try_get::<Decimal, _>(ordinal)?.to_string(),
        )),
        "TEXT" | "VARCHAR" | "BPCHAR" | "NAME" | "CITEXT" => {
            Ok(Value::String(row.try_get::<String, _>(ordinal)?))
        }
        "UUID" => Ok(Value::String(row.try_get::<Uuid, _>(ordinal)?.to_string())),
        "DATE" => Ok(Value::String(
            row.try_get::<NaiveDate, _>(ordinal)?.to_string(),
        )),
        "TIME" => Ok(Value::String(
            row.try_get::<NaiveTime, _>(ordinal)?.to_string(),
        )),
        "TIMESTAMP" => Ok(Value::String(
            row.try_get::<NaiveDateTime, _>(ordinal)?.to_string(),
        )),
        "TIMESTAMPTZ" => Ok(Value::String(
            row.try_get::<DateTime<Utc>, _>(ordinal)?.to_rfc3339(),
        )),
        "BYTEA" => Ok(Value::String(
            BASE64.encode(row.try_get::<Vec<u8>, _>(ordinal)?),
        )),
        "JSON" | "JSONB" => row.try_get::<Value, _>(ordinal),
        // Last resort for uncommon/extension types that are text-compatible.
        _ => match row.try_get::<String, _>(ordinal) {
            Ok(s) => Ok(Value::String(s)),
            Err(e) => Err(e),
        },
    }
}

fn is_safe_ident(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn json_value_to_checkpoint(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
