use std::{collections::HashMap, sync::Mutex};

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
    storage: Mutex<SanitizeStorage>,
}

impl InMemorySanitizer {
    pub fn new(storage: Mutex<SanitizeStorage>) -> Self {
        Self { storage }
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
                let mut merged = record.fields();
                for col in rules.get_source_field() {
                    merged.insert(
                        col.get_field_name().to_owned(),
                        self.get_mock_data(rules, col, &record),
                    );
                }
                SanitizedRecord::new(Value::Object(merged))
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
