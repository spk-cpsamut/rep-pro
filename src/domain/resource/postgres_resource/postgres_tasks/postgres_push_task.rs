use std::collections::HashMap;

use serde_json::Value;
use sqlx::{Pool, Postgres, QueryBuilder};
use tokio::sync::mpsc;

use crate::domain::resource::{Rules, SanitizedRecord, errors::PushError};

pub async fn push_task(
    table: String,
    pg_pool: Pool<Postgres>,
    rules: Rules,
    mut rx: mpsc::Receiver<Vec<SanitizedRecord>>,
) -> Result<(), PushError> {
    let conflict_field = rules
        .get_source_identifier()
        .get_field_identifer_as_string();

    if !is_safe_ident(&table) || !is_safe_ident(&conflict_field) {
        return Err(PushError::UnexpectedFailed);
    }

    let column_types = column_types(&pg_pool, &table).await?;

    while let Some(sanitized_records) = rx.recv().await {
        if sanitized_records.is_empty() {
            continue;
        }

        let columns = sanitized_records[0]
            .as_object()
            .ok_or(PushError::UnexpectedFailed)?;

        if !columns.keys().any(|c| c == &conflict_field) {
            return Err(PushError::UnexpectedFailed);
        }
        if columns
            .keys()
            .any(|column| !column_types.contains_key(column))
        {
            return Err(PushError::UnexpectedFailed);
        }

        let mut qb = QueryBuilder::<Postgres>::new("INSERT INTO ");
        qb.push(&table);
        qb.push(" (");
        for (i, col) in columns.keys().enumerate() {
            if i > 0 {
                qb.push(", ");
            }
            qb.push(col.as_str());
        }
        qb.push(") ");

        qb.push_values(&sanitized_records, |mut b, record| {
            let obj = record.as_object();
            for col in columns.keys() {
                let value = obj.and_then(|m| m.get(col)).unwrap_or(&Value::Null);
                let column_type = column_types
                    .get(col)
                    .map(String::as_str)
                    .unwrap_or_default();
                bind_json_value(&mut b, value, column_type);
            }
        });

        let update_columns: Vec<&String> = columns
            .keys()
            .filter(|col| *col != &conflict_field)
            .collect();

        qb.push(" ON CONFLICT (");
        qb.push(conflict_field.as_str());
        qb.push(") ");

        if update_columns.is_empty() {
            qb.push("DO NOTHING");
        } else {
            qb.push("DO UPDATE SET ");
            for (i, col) in update_columns.iter().enumerate() {
                if i > 0 {
                    qb.push(", ");
                }
                qb.push(col.as_str());
                qb.push(" = EXCLUDED.");
                qb.push(col.as_str());
            }
        }

        qb.build()
            .execute(&pg_pool)
            .await
            .map_err(|_| PushError::UnexpectedFailed)?;
    }

    Ok(())
}

fn is_safe_ident(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn bind_json_value(
    b: &mut sqlx::query_builder::Separated<'_, '_, Postgres, &str>,
    value: &Value,
    column_type: &str,
) {
    match value {
        Value::Null => {
            b.push("NULL");
        }
        Value::Bool(v) => {
            b.push_bind(*v);
            b.push_unseparated(format!("::{column_type}"));
        }
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                b.push_bind(i);
            } else if let Some(u) = n.as_u64() {
                b.push_bind(i64::try_from(u).unwrap_or(i64::MAX));
            } else if let Some(f) = n.as_f64() {
                b.push_bind(f);
            } else {
                b.push_bind(n.to_string());
            }
            b.push_unseparated(format!("::{column_type}"));
        }
        Value::String(s) => {
            b.push_bind(s.clone());
            b.push_unseparated(format!("::{column_type}"));
        }
        Value::Array(_) | Value::Object(_) => {
            b.push_bind(sqlx::types::Json(value.clone()));
            b.push_unseparated(format!("::{column_type}"));
        }
    }
}

async fn column_types(
    pool: &Pool<Postgres>,
    table: &str,
) -> Result<HashMap<String, String>, PushError> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        r#"
        SELECT a.attname::text, format_type(a.atttypid, a.atttypmod)
        FROM pg_catalog.pg_attribute a
        WHERE a.attrelid = $1::regclass
          AND a.attnum > 0
          AND NOT a.attisdropped
        "#,
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(|_| PushError::UnexpectedFailed)?;

    let mut types = HashMap::new();
    for (name, type_name) in rows {
        if !is_safe_type_name(&type_name) {
            return Err(PushError::UnexpectedFailed);
        }
        types.insert(name, type_name);
    }
    Ok(types)
}

fn is_safe_type_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | ' ' | '.' | '(' | ')' | ',' | '[' | ']')
        })
}

#[cfg(test)]
mod tests {
    use super::bind_json_value;
    use serde_json::json;
    use sqlx::{Postgres, QueryBuilder};

    #[test]
    fn text_values_are_cast_to_the_target_column_type() {
        let row = json!({
            "id": "00ffcfde-f578-4b43-b1e9-6c07f340b154",
            "name": "Ava"
        });
        let columns = row.as_object().unwrap();
        let mut qb = QueryBuilder::<Postgres>::new("INSERT INTO users (id, name) ");
        qb.push_values(std::iter::once(&row), |mut b, record| {
            let obj = record.as_object().unwrap();
            for col in columns.keys() {
                let column_type = if col == "id" {
                    "uuid"
                } else {
                    "character varying"
                };
                bind_json_value(&mut b, obj.get(col).unwrap(), column_type);
            }
        });
        assert_eq!(
            qb.sql(),
            "INSERT INTO users (id, name) VALUES ($1::uuid, $2::character varying)"
        );
    }
}
