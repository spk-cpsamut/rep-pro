use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use serde_json::{Value, json};
use sqlx::{PgPool, QueryBuilder};
use uuid::Uuid;

use crate::domain::{
    resource::{FieldIdentifierValue, Record, Rules, SanitizedRecord},
    sanitizer::{Sanitizer, SanitizerError, generate_mock_text},
};

#[derive(Clone)]
pub struct PersistentSanitizer {
    store: Arc<dyn ValueMappingStore>,
}

impl PersistentSanitizer {
    pub fn new(store: impl ValueMappingStore + 'static) -> Self {
        Self {
            store: Arc::new(store),
        }
    }

    pub async fn preview(
        &self,
        records: Vec<Record>,
        rules: Rules,
    ) -> Result<Vec<SanitizedRecord>, SanitizerError> {
        self.apply(records, &rules, false).await
    }

    async fn apply(
        &self,
        records: Vec<Record>,
        rules: &Rules,
        persist: bool,
    ) -> Result<Vec<SanitizedRecord>, SanitizerError> {
        let mut identities: Vec<String> = records
            .iter()
            .map(|record| FieldIdentifierValue::new(rules, record).as_str().to_owned())
            .collect();

        identities.sort();
        identities.dedup();

        let mut stored = self
            .store
            .load(*rules.get_persistent_key_id(), &identities)
            .await?;

        let mut missing = Vec::new();
        let mut seen = HashSet::new();
        for record in &records {
            let identity = FieldIdentifierValue::new(rules, record).as_str().to_owned();
            for field in rules.get_source_field() {
                let kind = field.get_sanitize_type().as_str();
                if stored.contains_key(&(identity.clone(), kind.to_owned())) {
                    continue;
                }
                if seen.insert((identity.clone(), kind.to_owned())) {
                    missing.push(MappingWrite {
                        source_identity: identity.clone(),
                        sanitize_type: kind.to_owned(),
                        target_value: generate_mock_text(field.get_sanitize_type()),
                    });
                }
            }
        }

        if persist && !missing.is_empty() {
            self.store
                .insert_missing(*rules.get_persistent_key_id(), &missing)
                .await?;
            stored = self
                .store
                .load(*rules.get_persistent_key_id(), &identities)
                .await?;
        } else {
            for row in missing {
                stored.insert((row.source_identity, row.sanitize_type), row.target_value);
            }
        }

        let mut sanitized = Vec::with_capacity(records.len());
        for record in records {
            let identity = FieldIdentifierValue::new(rules, &record)
                .as_str()
                .to_owned();
            let mut row = record.fields();
            let identifier = rules
                .get_source_identifier()
                .get_field_identifer_as_string();
            row.insert(identifier, record.get_identifier_val(rules));
            for field in rules.get_source_field() {
                let value = stored
                    .get(&(
                        identity.clone(),
                        field.get_sanitize_type().as_str().to_owned(),
                    ))
                    .ok_or(SanitizerError::StoreFailed)?;
                row.insert(field.get_field_name().to_owned(), json!(value));
            }
            sanitized.push(SanitizedRecord::new(Value::Object(row)));
        }
        Ok(sanitized)
    }
}

#[async_trait::async_trait]
impl Sanitizer for PersistentSanitizer {
    async fn sanitize(
        &self,
        records: Vec<Record>,
        rules: Rules,
    ) -> Result<Vec<SanitizedRecord>, SanitizerError> {
        self.apply(records, &rules, true).await
    }
}

pub struct MappingWrite {
    source_identity: String,
    sanitize_type: String,
    target_value: String,
}

#[async_trait::async_trait]
pub trait ValueMappingStore: Send + Sync {
    async fn load(
        &self,
        persistent_key_id: Uuid,
        identities: &[String],
    ) -> Result<HashMap<(String, String), String>, SanitizerError>;

    async fn insert_missing(
        &self,
        persistent_key_id: Uuid,
        rows: &[MappingWrite],
    ) -> Result<(), SanitizerError>;
}

#[derive(Clone, Default)]
pub struct MemoryValueMappingStore {
    inner: Arc<Mutex<HashMap<(Uuid, String, String), String>>>,
}

impl MemoryValueMappingStore {
    pub fn len(&self) -> usize {
        self.inner.lock().expect("mapping store lock").len()
    }
}

#[async_trait::async_trait]
impl ValueMappingStore for MemoryValueMappingStore {
    async fn load(
        &self,
        persistent_key_id: Uuid,
        identities: &[String],
    ) -> Result<HashMap<(String, String), String>, SanitizerError> {
        let guard = self.inner.lock().expect("mapping store lock");
        let mut found = HashMap::new();
        for ((key_id, identity, kind), value) in guard.iter() {
            if *key_id == persistent_key_id && identities.iter().any(|item| item == identity) {
                found.insert((identity.clone(), kind.clone()), value.clone());
            }
        }
        Ok(found)
    }

    async fn insert_missing(
        &self,
        persistent_key_id: Uuid,
        rows: &[MappingWrite],
    ) -> Result<(), SanitizerError> {
        let mut guard = self.inner.lock().expect("mapping store lock");
        for row in rows {
            guard
                .entry((
                    persistent_key_id,
                    row.source_identity.clone(),
                    row.sanitize_type.clone(),
                ))
                .or_insert_with(|| row.target_value.clone());
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct PostgresValueMappingStore {
    pool: PgPool,
}

impl PostgresValueMappingStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct MappingRow {
    source_identity: String,
    sanitize_type: String,
    target_value: String,
}

#[async_trait::async_trait]
impl ValueMappingStore for PostgresValueMappingStore {
    async fn load(
        &self,
        persistent_key_id: Uuid,
        identities: &[String],
    ) -> Result<HashMap<(String, String), String>, SanitizerError> {
        if identities.is_empty() {
            return Ok(HashMap::new());
        }
        let rows: Vec<MappingRow> = sqlx::query_as(
            "SELECT source_identity, sanitize_type, target_value FROM value_mappings WHERE persistent_key_id = $1 AND source_identity = ANY($2)",
        )
        .bind(persistent_key_id)
        .bind(identities)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| SanitizerError::StoreFailed)?;
        Ok(rows
            .into_iter()
            .map(|row| ((row.source_identity, row.sanitize_type), row.target_value))
            .collect())
    }

    async fn insert_missing(
        &self,
        persistent_key_id: Uuid,
        rows: &[MappingWrite],
    ) -> Result<(), SanitizerError> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut query = QueryBuilder::new(
            "INSERT INTO value_mappings (persistent_key_id, source_identity, sanitize_type, target_value) ",
        );
        query.push_values(rows, |mut builder, row| {
            builder
                .push_bind(persistent_key_id)
                .push_bind(&row.source_identity)
                .push_bind(&row.sanitize_type)
                .push_bind(&row.target_value);
        });
        query.push(" ON CONFLICT (persistent_key_id, source_identity, sanitize_type) DO NOTHING");
        query
            .build()
            .execute(&self.pool)
            .await
            .map_err(|_| SanitizerError::StoreFailed)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::{MemoryValueMappingStore, PersistentSanitizer};
    use crate::domain::resource::{Record, Rules};
    use crate::domain::sanitizer::Sanitizer;

    fn rules(rule_id: u128, key_id: Uuid, fields: serde_json::Value) -> Rules {
        Rules::from_stored(
            Uuid::from_u128(rule_id),
            key_id,
            &json!({
                "force_pull_from_start": false,
                "source_naming_convention": "snake_case",
                "target_naming_convention": "snake_case",
                "source_identifier": "id",
                "source_sanitize_fields": fields
            }),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn linked_rules_reuse_the_same_row_identity() {
        let store = MemoryValueMappingStore::default();
        let sanitizer = PersistentSanitizer::new(store.clone());
        let key_id = Uuid::from_u128(9);
        let users = rules(
            1,
            key_id,
            json!([
                { "field_name": "name", "sanitize_type": "name" },
                { "field_name": "email", "sanitize_type": "email" }
            ]),
        );
        let profiles = rules(
            2,
            key_id,
            json!([
                { "field_name": "full_name", "sanitize_type": "name" },
                { "field_name": "contact", "sanitize_type": "email" }
            ]),
        );
        let user = Record::from_value(json!({
            "id": "42",
            "name": "Ava Martinez",
            "email": "ava@example.com"
        }));
        let profile = Record::from_value(json!({
            "id": "42",
            "full_name": "Ava Martinez",
            "contact": "ava@example.com"
        }));

        let first = sanitizer
            .sanitize(vec![user], users)
            .await
            .expect("sanitize users");
        let second = sanitizer
            .sanitize(vec![profile], profiles)
            .await
            .expect("sanitize profiles");

        assert_ne!(first[0]["name"], "Ava Martinez");
        assert_eq!(first[0]["name"], second[0]["full_name"]);
        assert_eq!(first[0]["email"], second[0]["contact"]);
        assert_eq!(store.len(), 2);
    }

    #[tokio::test]
    async fn preview_does_not_store_a_new_identity() {
        let store = MemoryValueMappingStore::default();
        let sanitizer = PersistentSanitizer::new(store.clone());
        let key_id = Uuid::from_u128(9);
        let users = rules(
            1,
            key_id,
            json!([{ "field_name": "name", "sanitize_type": "name" }]),
        );
        let row = Record::from_value(json!({ "id": "42", "name": "Ava Martinez" }));

        sanitizer
            .preview(vec![row], users.clone())
            .await
            .expect("preview");
        assert_eq!(store.len(), 0);

        let stored = sanitizer
            .sanitize(
                vec![Record::from_value(
                    json!({ "id": "42", "name": "Ava Martinez" }),
                )],
                users.clone(),
            )
            .await
            .expect("sanitize");
        let again = sanitizer
            .sanitize(
                vec![Record::from_value(
                    json!({ "id": "42", "name": "Ava Martinez" }),
                )],
                users,
            )
            .await
            .expect("sanitize again");
        assert_eq!(stored[0]["name"], again[0]["name"]);
        assert_eq!(store.len(), 1);
    }
}
