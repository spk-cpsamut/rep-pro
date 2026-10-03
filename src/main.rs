mod api;

use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let database_url = std::env::var("DATABASE_URL")?;
    let kms_key_id = std::env::var("KMS_KEY_ID")?;
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;
    let sdk_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let app = api::router(api::AppState {
        pool,
        kms: aws_sdk_kms::Client::new(&sdk_config),
        kms_key_id,
    });
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
