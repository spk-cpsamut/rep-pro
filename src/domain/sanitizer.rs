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

#[cfg(test)]
mod tests {
    use super::generate_mock_text;
    use crate::domain::resource::SanitizeType;

    fn is_ssn(value: &str) -> bool {
        let mut parts = value.split('-');
        let Some(area) = parts.next() else {
            return false;
        };
        let Some(group) = parts.next() else {
            return false;
        };
        let Some(serial) = parts.next() else {
            return false;
        };
        parts.next().is_none()
            && area.len() == 3
            && group.len() == 2
            && serial.len() == 4
            && area
                .chars()
                .chain(group.chars())
                .chain(serial.chars())
                .all(|c| c.is_ascii_digit())
    }

    #[test]
    fn mock_text_matches_each_sanitize_type() {
        let name = generate_mock_text(&SanitizeType::Name);
        let email = generate_mock_text(&SanitizeType::Email);
        let phone = generate_mock_text(&SanitizeType::PhoneNumber);
        let ssn = generate_mock_text(&SanitizeType::Ssn);

        assert!(!name.is_empty());
        assert!(!email.is_empty());
        assert!(email.contains('@'));
        assert!(!phone.is_empty());
        assert!(is_ssn(&ssn));
    }
}
