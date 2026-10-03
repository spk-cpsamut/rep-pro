pub mod orchestrator;
pub mod resource;
pub mod resource_map;
pub mod sanitizer;
pub mod state;

mod http;

pub use state::AppState;

pub fn router(state: AppState) -> axum::Router {
    axum::Router::new()
        .merge(resource::handlers::router())
        .merge(resource_map::handlers::router())
        .with_state(state)
}
