mod clilent;
mod server;
use clilent::Client;
use sqlx::postgres::PgPoolOptions;
use std::env;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let client = Client::new();
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&env::var("DATABASE_URL")?)
        .await?;
    server::encrypt(client, &pool)
        .await
        .expect("resource from client encrypt failed");

    Ok(())
}
