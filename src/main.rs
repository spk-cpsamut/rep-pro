mod clilent;
mod server;
use std::fmt::Error;

use clilent::Client;

#[tokio::main]
async fn main(){
    let client = Client::new();

    server::encrypt(client).await;
}
