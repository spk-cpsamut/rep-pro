mod telemetry;

use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    telemetry::init();
    dotenvy::dotenv().ok();
    let database_url = std::env::var("DATABASE_URL")?;
    let kms_key_id = std::env::var("KMS_KEY_ID")?;
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;
    let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest());
    if let Ok(profile) = std::env::var("AWS_PROFILE_NAME") {
        loader = loader.profile_name(profile);
    }
    let sdk_config = loader.load().await;
    let app = rep_pro::domain::router(rep_pro::AppState::new(
        pool,
        aws_sdk_kms::Client::new(&sdk_config),
        kms_key_id,
    ));
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
