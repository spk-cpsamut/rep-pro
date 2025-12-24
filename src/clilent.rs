pub struct Client {
    username: String,
    password: String,
    config: Config
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

struct Config {

}