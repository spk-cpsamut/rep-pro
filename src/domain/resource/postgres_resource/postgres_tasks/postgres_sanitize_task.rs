use tokio::sync::mpsc;

use crate::domain::{
    resource::{Record, Rules, SanitizedRecord, errors::SanitizeError},
    sanitizer::Sanitizer,
};

pub async fn sanitize_task(
    tx: mpsc::Sender<Vec<SanitizedRecord>>,
    mut rx: mpsc::Receiver<Vec<Record>>,
    rules: Rules,
    sanitizer: Box<dyn Sanitizer>,
) -> Result<(), SanitizeError> {
    while let Some(records) = rx.recv().await {
        let sanitized_records = sanitizer
            .sanitize(records, rules.clone())
            .await
            .map_err(|_| SanitizeError::SanitizerFailed)?;

        tx.send(sanitized_records)
            .await
            .map_err(|_| SanitizeError::SanitizerFailed)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use serde_json::json;
    use tokio::sync::mpsc;
    use uuid::Uuid;

    use super::sanitize_task;
    use crate::domain::resource::{Record, Rules, SanitizeError, SanitizedRecord};
    use crate::domain::sanitizer::{Sanitizer, SanitizerError};

    fn rules() -> Rules {
        Rules::from_stored(
            Uuid::from_u128(1),
            Uuid::from_u128(2),
            &json!({
                "force_pull_from_start": false,
                "source_naming_convention": "snake_case",
                "target_naming_convention": "snake_case",
                "source_identifier": "id",
                "source_sanitize_fields": []
            }),
        )
        .unwrap()
    }

    struct Echo;

    #[async_trait::async_trait]
    impl Sanitizer for Echo {
        async fn sanitize(
            &self,
            records: Vec<Record>,
            _rules: Rules,
        ) -> Result<Vec<SanitizedRecord>, SanitizerError> {
            Ok(records
                .into_iter()
                .map(|record| SanitizedRecord::new(record.deref().clone()))
                .collect())
        }
    }

    struct Boom;

    #[async_trait::async_trait]
    impl Sanitizer for Boom {
        async fn sanitize(
            &self,
            _records: Vec<Record>,
            _rules: Rules,
        ) -> Result<Vec<SanitizedRecord>, SanitizerError> {
            Err(SanitizerError::StoreFailed)
        }
    }

    #[tokio::test]
    async fn a_sanitizer_error_fails_the_task() {
        let (tx, _rx) = mpsc::channel(1);
        let (tx_in, rx_in) = mpsc::channel(1);
        tx_in
            .send(vec![Record::from_value(json!({"id": "1"}))])
            .await
            .unwrap();
        drop(tx_in);
        let err = sanitize_task(tx, rx_in, rules(), Box::new(Boom)).await;
        assert!(matches!(err, Err(SanitizeError::SanitizerFailed)));
    }

    #[tokio::test]
    async fn a_dropped_receiver_fails_the_task() {
        let (tx, rx_out) = mpsc::channel(1);
        drop(rx_out);
        let (tx_in, rx_in) = mpsc::channel(1);
        tx_in
            .send(vec![Record::from_value(json!({"id": "1"}))])
            .await
            .unwrap();
        drop(tx_in);
        let err = sanitize_task(tx, rx_in, rules(), Box::new(Echo)).await;
        assert!(matches!(err, Err(SanitizeError::SanitizerFailed)));
    }

    #[tokio::test]
    async fn batches_are_forwarded_in_order() {
        let (tx, mut rx_out) = mpsc::channel(4);
        let (tx_in, rx_in) = mpsc::channel(4);
        tx_in
            .send(vec![Record::from_value(json!({"id": 1}))])
            .await
            .unwrap();
        tx_in
            .send(vec![Record::from_value(json!({"id": 2}))])
            .await
            .unwrap();
        drop(tx_in);
        assert!(
            sanitize_task(tx, rx_in, rules(), Box::new(Echo))
                .await
                .is_ok()
        );
        let first = rx_out.recv().await.unwrap();
        let second = rx_out.recv().await.unwrap();
        assert_eq!(first[0]["id"], 1);
        assert_eq!(second[0]["id"], 2);
    }

    #[tokio::test]
    async fn a_closed_input_finishes_without_work() {
        let (tx, _rx) = mpsc::channel(1);
        let (tx_in, rx_in) = mpsc::channel(1);
        drop(tx_in);
        assert!(
            sanitize_task(tx, rx_in, rules(), Box::new(Echo))
                .await
                .is_ok()
        );
    }
}
