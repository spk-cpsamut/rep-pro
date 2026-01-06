use serde::Serialize;
pub struct Client {
    pub username: String,
    pub password: String,
    pub config: Config,
}

impl Client {
    pub fn new() -> Client {
        Client {
            username: "new".to_string(),
            password: "year".to_string(),
            config: Config {  }
        }
    }
}

#[derive(Serialize)]
pub struct Config {

}