use crate::domain::resource::{Record, Rules, SanitizedRecord};

pub mod in_memory_sanitizer;
pub mod persistent_sanitizer;

pub use errors::SanitizerError;

#[async_trait::async_trait]
pub trait Sanitizer: Send + Sync {
    async fn sanitize(
        &self,
        records: Vec<Record>,
        rule: Rules,
    ) -> Result<Vec<SanitizedRecord>, SanitizerError>;
}

pub mod errors {
    pub enum SanitizerError {
        NotImplemented,
    }
}
