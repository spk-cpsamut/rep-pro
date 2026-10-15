use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use serde_json::{Value, json};
use uuid::Uuid;

use crate::domain::{
    resource::{FieldIdentifierValue, FieldRecord, Record, Rules, SanitizedRecord},
    sanitizer::{Sanitizer, errors::SanitizerError, generate_mock_text},
};

pub struct SanitizeStorage {
    pub map: HashMap<SanitizeStorageMapKey, Value>,
}

pub struct InMemorySanitizer {
    storage: Arc<Mutex<SanitizeStorage>>,
}

impl InMemorySanitizer {
    pub fn new(storage: Mutex<SanitizeStorage>) -> Self {
        Self {
            storage: Arc::new(storage),
        }
    }
}

impl Clone for InMemorySanitizer {
    fn clone(&self) -> Self {
        Self {
            storage: Arc::clone(&self.storage),
        }
    }
}

#[derive(Hash, PartialEq, Eq)]
pub struct SanitizeStorageMapKey(Uuid, FieldIdentifierValue, FieldRecord);

#[async_trait::async_trait]
impl Sanitizer for InMemorySanitizer {
    async fn sanitize(
        &self,
        records: Vec<Record>,
        rules: Rules,
    ) -> Result<Vec<SanitizedRecord>, SanitizerError> {
        Ok(self.sanitize_sync(records, &rules))
    }
}

impl InMemorySanitizer {
    fn sanitize_sync(&self, records: Vec<Record>, rules: &Rules) -> Vec<SanitizedRecord> {
        records
            .into_iter()
            .map(|record| {
                let mut row = record.fields();
                let identifier = rules
                    .get_source_identifier()
                    .get_field_identifer_as_string();
                row.insert(identifier, record.get_identifier_val(rules));
                for col in rules.get_source_field() {
                    row.insert(
                        col.get_field_name().to_owned(),
                        self.get_mock_data(rules, col, &record),
                    );
                }
                SanitizedRecord::new(Value::Object(row))
            })
            .collect()
    }

    fn get_mock_data(&self, rules: &Rules, col: &FieldRecord, record: &Record) -> Value {
        let mut storage = self.storage.lock().unwrap();
        let key = SanitizeStorageMapKey(
            rules.get_rule_id().clone(),
            FieldIdentifierValue::new(rules, record),
            col.clone(),
        );

        if let Some(val) = storage.map.get(&key) {
            return val.clone();
        }

        let v = json!(generate_mock_text(col.get_sanitize_type()));

        storage.map.insert(key, v.clone());

        v
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use serde_json::json;
    use uuid::Uuid;

    use super::{InMemorySanitizer, SanitizeStorage};
    use crate::domain::resource::{Record, Rules};
    use crate::domain::sanitizer::Sanitizer;

    #[tokio::test]
    async fn unsanitized_columns_are_copied() {
        let sanitizer = InMemorySanitizer::new(Mutex::new(SanitizeStorage {
            map: HashMap::new(),
        }));
        let rules = Rules::from_stored(
            Uuid::from_u128(1),
            Uuid::from_u128(2),
            &json!({
                "force_pull_from_start": false,
                "source_naming_convention": "snake_case",
                "target_naming_convention": "snake_case",
                "source_identifier": "id",
                "source_sanitize_fields": [
                    { "field_name": "name", "sanitize_type": "name" },
                    { "field_name": "email", "sanitize_type": "email" }
                ]
            }),
        )
        .unwrap();
        let source = json!({
            "id": "42",
            "name": "Ava Martinez",
            "email": "ava@example.com",
            "age": 24,
            "salary": "43000.00",
            "department": "HR",
            "is_active": true
        });

        let Ok(sanitized) = sanitizer
            .sanitize(vec![Record::from_value(source)], rules)
            .await
        else {
            panic!("sanitize failed");
        };
        let row = &sanitized[0];

        assert_eq!(row["id"], "42");
        assert_eq!(row["age"], 24);
        assert_eq!(row["salary"], "43000.00");
        assert_eq!(row["department"], "HR");
        assert_eq!(row["is_active"], true);
        assert_ne!(row["name"], "Ava Martinez");
        assert_ne!(row["email"], "ava@example.com");
    }

    fn rules(fields: serde_json::Value) -> Rules {
        Rules::from_stored(
            Uuid::from_u128(1),
            Uuid::from_u128(2),
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

    fn sanitizer() -> InMemorySanitizer {
        InMemorySanitizer::new(Mutex::new(SanitizeStorage {
            map: HashMap::new(),
        }))
    }

    #[tokio::test]
    async fn repeated_identity_reuses_the_cached_value() {
        let sanitizer = sanitizer();
        let rules = rules(json!([{ "field_name": "name", "sanitize_type": "name" }]));
        let once = sanitizer
            .sanitize(
                vec![Record::from_value(json!({"id": "1", "name": "Ava"}))],
                rules.clone(),
            )
            .await
            .unwrap();
        let twice = sanitizer
            .sanitize(
                vec![Record::from_value(json!({"id": "1", "name": "Ava"}))],
                rules,
            )
            .await
            .unwrap();
        assert_eq!(once[0]["name"], twice[0]["name"]);
        assert_ne!(once[0]["name"], "Ava");
    }

    #[tokio::test]
    async fn rows_that_share_an_identity_share_a_value() {
        let sanitizer = sanitizer();
        let rules = rules(json!([{ "field_name": "name", "sanitize_type": "name" }]));
        let sanitized = sanitizer
            .sanitize(
                vec![
                    Record::from_value(json!({"id": "1", "name": "Ava"})),
                    Record::from_value(json!({"id": "1", "name": "Other"})),
                ],
                rules,
            )
            .await
            .unwrap();
        assert_eq!(sanitized[0]["name"], sanitized[1]["name"]);
        assert_ne!(sanitized[0]["name"], "Ava");
    }

    #[tokio::test]
    async fn empty_batch_returns_no_rows() {
        let sanitized = sanitizer()
            .sanitize(
                vec![],
                rules(json!([{ "field_name": "name", "sanitize_type": "name" }])),
            )
            .await
            .unwrap();
        assert!(sanitized.is_empty());
    }

    #[tokio::test]
    async fn a_rule_with_no_sanitize_fields_copies_the_row() {
        let sanitized = sanitizer()
            .sanitize(
                vec![Record::from_value(json!({"id": "7", "age": 3}))],
                rules(json!([])),
            )
            .await
            .unwrap();
        assert_eq!(sanitized[0]["id"], "7");
        assert_eq!(sanitized[0]["age"], 3);
    }

    #[tokio::test]
    async fn a_missing_identifier_is_stored_as_null() {
        let sanitized = sanitizer()
            .sanitize(
                vec![Record::from_value(json!({"name": "Ava"}))],
                rules(json!([{ "field_name": "name", "sanitize_type": "name" }])),
            )
            .await
            .unwrap();
        assert_eq!(sanitized[0]["id"], serde_json::Value::Null);
        assert_ne!(sanitized[0]["name"], "Ava");
    }
}
