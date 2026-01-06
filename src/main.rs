mod clilent;
mod server;
use clilent::Client;
#[tokio::main]
async fn main() ->  Result<(), Box<dyn std::error::Error>>{
    dotenvy::dotenv().ok();
    let client = Client::new();
    server::encrypt(client).await.expect("server encrypt failed");

    Ok(())
}
