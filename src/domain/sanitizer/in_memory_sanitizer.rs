use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use serde_json::{Value, json};
use uuid::Uuid;

use fake::Fake;
use fake::faker::internet::en::SafeEmail;
use fake::faker::name::en::Name;
use fake::faker::phone_number::en::PhoneNumber;

use crate::domain::{
    resource::{FieldIdentifierValue, FieldRecord, Record, Rules, SanitizeType, SanitizedRecord},
    sanitizer::{Sanitizer, errors::SanitizerError},
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

        let v = Self::generate_mock_data(col.get_sanitize_type());

        storage.map.insert(key, v.clone());

        v
    }

    fn generate_mock_data(sanitize_type: &SanitizeType) -> Value {
        match sanitize_type {
            SanitizeType::Name => json!(Name().fake::<String>()),
            SanitizeType::Email => json!(SafeEmail().fake::<String>()),
            SanitizeType::PhoneNumber => json!(PhoneNumber().fake::<String>()),
            SanitizeType::Ssn => {
                // no built-in SSN faker — roll a simple pattern
                let n: u32 = (100_000_000..999_999_999).fake();
                json!(format!(
                    "{:03}-{:02}-{:04}",
                    n / 1_000_000,
                    (n / 10_000) % 100,
                    n % 10_000
                ))
            }
        }
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
}
