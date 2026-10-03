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
