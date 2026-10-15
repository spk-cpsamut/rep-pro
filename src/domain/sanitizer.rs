use fake::Fake;
use fake::faker::internet::en::SafeEmail;
use fake::faker::name::en::Name;
use fake::faker::phone_number::en::PhoneNumber;

use crate::domain::resource::{Record, Rules, SanitizeType, SanitizedRecord};

pub mod in_memory_sanitizer;
pub mod persistent_sanitizer;

pub use errors::SanitizerError;

pub(crate) fn generate_mock_text(sanitize_type: &SanitizeType) -> String {
    match sanitize_type {
        SanitizeType::Name => Name().fake(),
        SanitizeType::Email => SafeEmail().fake(),
        SanitizeType::PhoneNumber => PhoneNumber().fake(),
        SanitizeType::Ssn => {
            let n: u32 = (100_000_000..999_999_999).fake();
            format!(
                "{:03}-{:02}-{:04}",
                n / 1_000_000,
                (n / 10_000) % 100,
                n % 10_000
            )
        }
    }
}

#[async_trait::async_trait]
pub trait Sanitizer: Send + Sync {
    async fn sanitize(
        &self,
        records: Vec<Record>,
        rule: Rules,
    ) -> Result<Vec<SanitizedRecord>, SanitizerError>;
}

pub mod errors {
    #[derive(Debug)]
    pub enum SanitizerError {
        NotImplemented,
        StoreFailed,
    }
}
