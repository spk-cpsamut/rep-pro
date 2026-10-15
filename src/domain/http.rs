use axum::http::StatusCode;

pub(crate) const MAX_IDENT: usize = 255;

pub(crate) fn is_safe_ident(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

pub(crate) fn bad_request() -> (StatusCode, &'static str) {
    (StatusCode::BAD_REQUEST, "bad request")
}

pub(crate) fn internal_error() -> (StatusCode, &'static str) {
    (StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
}

#[cfg(test)]
mod tests {
    use super::{bad_request, internal_error, is_safe_ident};
    use axum::http::StatusCode;

    #[test]
    fn ident_accepts_letters_and_underscores_only() {
        assert!(!is_safe_ident(""));
        assert!(!is_safe_ident("1users"));
        assert!(is_safe_ident("_users"));
        assert!(!is_safe_ident("user-name"));
        assert!(is_safe_ident("users"));
    }

    #[test]
    fn error_helpers_use_fixed_status_and_message() {
        assert_eq!(bad_request(), (StatusCode::BAD_REQUEST, "bad request"));
        assert_eq!(
            internal_error(),
            (StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
        );
    }
}
