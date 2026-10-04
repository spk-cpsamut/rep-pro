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

            let column_type = column_type_name(&pg_pool, &table, &field).await?;
            let mut pointer = pointer;
            let mut first_run = true;
            loop {
                let mut records: Vec<Record> = Vec::new();
                let apply_pointer =
                    pointer.is_some() && !(rules.force_pull_from_start && first_run);
                let rows = {
                    let bound = if apply_pointer {
                        pointer.as_deref()
                    } else {
                        None
                    };
                    let mut qb = cursor_query(
                        &table,
                        &field,
                        &column_type,
                        bound,
                        batch_size.get() as i64,
                    );
                    qb.build()
                        .fetch_all(&pg_pool)
                        .await
                        .map_err(|_| PullError::FailedToExtractData)?
                };

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

pub(crate) async fn fetch_sample_row(
    pool: &Pool<Postgres>,
    table: &str,
    order_by: &str,
) -> Result<Option<(Vec<String>, Value)>, ()> {
    if !is_safe_ident(table) || !is_safe_ident(order_by) {
        return Err(());
    }

    let sql = format!("SELECT * FROM {table} ORDER BY {order_by} LIMIT 1");
    let row = sqlx::query(&sql)
        .fetch_optional(pool)
        .await
        .map_err(|_| ())?;
    let Some(row) = row else {
        return Ok(None);
    };

    let mut columns = Vec::new();
    let mut object = serde_json::Map::new();
    for column in row.columns() {
        let name = column.name().to_owned();
        let value =
            decode_pg_value(&row, column.ordinal(), column.type_info().name()).map_err(|_| ())?;
        columns.push(name.clone());
        object.insert(name, value);
    }
    Ok(Some((columns, Value::Object(object))))
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

fn cursor_query<'a>(
    table: &str,
    field: &str,
    column_type: &str,
    pointer: Option<&'a str>,
    limit: i64,
) -> QueryBuilder<'a, Postgres> {
    let mut qb = QueryBuilder::<Postgres>::new(format!("SELECT * FROM {table} WHERE 1=1"));
    if let Some(pointer) = pointer {
        qb.push(format!(" AND {field} > "));
        qb.push_bind(pointer);
        qb.push(format!("::{column_type}"));
    }
    qb.push(format!(" ORDER BY {field} LIMIT "));
    qb.push_bind(limit);
    qb
}

async fn column_type_name(
    pool: &Pool<Postgres>,
    table: &str,
    column: &str,
) -> Result<String, PullError> {
    let type_name: Option<String> = sqlx::query_scalar(
        r#"
        SELECT format_type(a.atttypid, a.atttypmod)
        FROM pg_catalog.pg_attribute a
        WHERE a.attrelid = $1::regclass
          AND a.attname = $2
          AND a.attnum > 0
          AND NOT a.attisdropped
        "#,
    )
    .bind(table)
    .bind(column)
    .fetch_optional(pool)
    .await
    .map_err(|_| PullError::FailedToExtractData)?;

    let type_name = type_name.ok_or(PullError::FailedToExtractData)?;
    if !is_safe_type_name(&type_name) {
        return Err(PullError::FailedToExtractData);
    }
    Ok(type_name)
}

fn is_safe_ident(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn is_safe_type_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | ' ' | '.' | '(' | ')' | ',' | '[' | ']')
        })
}

#[cfg(test)]
mod tests {
    use super::cursor_query;

    #[test]
    fn limit_placeholder_is_separated_from_the_keyword() {
        let query = cursor_query("users", "id", "uuid", None, 100);
        assert_eq!(
            query.sql(),
            "SELECT * FROM users WHERE 1=1 ORDER BY id LIMIT $1"
        );
    }

    #[test]
    fn checkpoint_is_cast_to_the_column_type() {
        let query = cursor_query(
            "users",
            "created_at",
            "timestamp with time zone",
            Some("2026-10-03T16:17:36.422975+00:00"),
            100,
        );
        assert_eq!(
            query.sql(),
            "SELECT * FROM users WHERE 1=1 AND created_at > $1::timestamp with time zone ORDER BY created_at LIMIT $2"
        );
    }
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
