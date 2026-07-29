use crate::domain::{
    resource::{Record, Rules, SanitizedRecord},
    sanitizer::{Sanitizer, SanitizerError},
};

pub struct InPersistentSanitizer {}

#[async_trait::async_trait]
impl Sanitizer for InPersistentSanitizer {
    async fn sanitize(
        &self,
        _record: Vec<Record>,
        _rule: Rules,
    ) -> Result<Vec<SanitizedRecord>, SanitizerError> {
        Err(SanitizerError::NotImplemented)
    }
}
