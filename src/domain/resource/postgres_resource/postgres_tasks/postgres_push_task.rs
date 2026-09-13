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
                bind_json_value(&mut b, value);
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

fn bind_json_value(b: &mut sqlx::query_builder::Separated<'_, '_, Postgres, &str>, value: &Value) {
    match value {
        Value::Null => {
            b.push("NULL");
        }
        Value::Bool(v) => {
            b.push_bind(*v);
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
        }
        Value::String(s) => {
            b.push_bind(s.clone());
        }
        Value::Array(_) | Value::Object(_) => {
            b.push_bind(sqlx::types::Json(value.clone()));
        }
    }
}
