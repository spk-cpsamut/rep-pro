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
