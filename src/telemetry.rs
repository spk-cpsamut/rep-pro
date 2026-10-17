use tracing_error::ErrorLayer;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let json = std::env::var("LOG_FORMAT").is_ok_and(|format| format.eq_ignore_ascii_case("json"));
    let events = fmt::format::FmtSpan::CLOSE;

    if json {
        tracing_subscriber::registry()
            .with(filter)
            .with(ErrorLayer::default())
            .with(
                fmt::layer()
                    .json()
                    .with_target(true)
                    .with_span_events(events),
            )
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(ErrorLayer::default())
            .with(
                fmt::layer()
                    .compact()
                    .with_target(true)
                    .with_span_events(events),
            )
            .init();
    }
}
