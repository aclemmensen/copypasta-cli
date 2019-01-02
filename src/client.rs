use std::fmt::Display;
use std::error::Error;
use serde_derive::{Serialize, Deserialize};
use reqwest::{StatusCode};
use std::path::Path;

const DEFAULT_SCHEME: &str = "http";
const DEFAULT_HOST: &str = "localhost:4000";

pub struct PastaClient {
    pub client: reqwest::Client,
    pub config: Option<ClientConfig>,
    pub config_path: String,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct ClientConfig {
    pub token: String,
    pub host: String
}

impl ClientConfig {
    pub fn load_from_file(path: &str) -> Result<ClientConfig, HeyError> {
        let fspath = Path::new(path);
        if !fspath.is_file() {
            return Err(HeyError::NoConfigFound);
        }

        let content = std::fs::read_to_string(path).unwrap();
        let deser: ClientConfig = serde_json::from_str(&content).unwrap();
        Ok(deser)
    }
}

#[derive(Debug)]
pub enum HeyError {
    NoConfigFound,
    NotLoggedIn(String),
    LoginError,
    NoToken,
    ServerError(StatusCode),
    RequestError(reqwest::Error)
}

impl From<reqwest::Error> for HeyError {
    fn from(err: reqwest::Error) -> Self {
        HeyError::RequestError(err)
    }
}

impl Display for HeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "HeyError")
    }
}

impl Error for HeyError {

}

impl PastaClient {
    pub fn new(client: reqwest::Client, path: String) -> PastaClient {
        PastaClient {
            client,
            config: None,
            config_path: path
        }
    }

    pub fn set_config(&mut self, config: ClientConfig) -> () {
        self.config = Some(config);
    }
    
    pub fn login(&self) -> Result<UserInfo, HeyError> {
        let mut resp = self.add_token(self.client.get(&self.get_url("api")))
            .send()?;
        
        check_resp(&mut resp)?;

        let resp: UserInfo = resp.json()?;

        Ok(resp)
    }

    pub fn latest(&self) -> Result<Pasta, HeyError> {
        let mut resp = self.add_token(self.client.get(&self.get_url("api/latest")))
            .send()?;
        
        check_resp(&mut resp)?;

        let pasta: Pasta = resp.json()?;

        Ok(pasta)
    }

    pub fn list(&self) -> Result<Vec<Pasta>, HeyError> {
        let mut resp = self.add_token(self.client.get(&self.get_url("api/list")))
            .send()?;
        
        check_resp(&mut resp)?;

        let pastas: Vec<Pasta> = resp.json()?;

        Ok(pastas)
    }

    pub fn post(&self, content: String) -> Result<(), HeyError> {
        let msg = CreatePasta {
            content
        };

        let mut resp = self.add_token(self.client.post(&self.get_url("api/create")))
            .json(&msg)
            .send()?;
        
        check_resp(&mut resp)
    }

    pub fn create_stream(&self) -> Result<CreateStreamResponse, HeyError> {
        let mut resp = self.add_token(self.client.get(&self.get_url("api/stream")))
            .send()?;

        check_resp(&mut resp)?;

        let resp: CreateStreamResponse = resp.json()?;

        Ok(resp)
    }

    pub fn get_socket_url(&self) -> Result<String, HeyError> {
        Ok("ws://localhost:4000/socket".to_string())
    }

    pub fn get_token(&self) -> Option<String> {
        if let Some(config) = &self.config {
            return Some(config.token.to_string());
        }

        None
    }

    pub fn save_config(&self) -> Result<(), Box<Error>> {
        if let Some(conf) = &self.config {
            let ser = serde_json::to_string(&conf)?;
            std::fs::write(&self.config_path, &ser)?;
            Ok(())
        } else {
            Err(Box::new(HeyError::NoToken))
        }
    }

    fn add_token(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(config) = &self.config {
            return builder.bearer_auth(config.token.to_string());
        }

        return builder;
    }

    fn get_url(&self, url: &str) -> String {
        match self.config {
            Some(ref c) => format!("{}://{}/{}", DEFAULT_SCHEME, c.host, url),
            None => format!("{}://{}/{}", DEFAULT_SCHEME, DEFAULT_HOST, url)
        }
    }
}

#[derive(Serialize)]
struct CreatePasta {
    pub content: String
}

#[derive(Deserialize, Debug)]
pub struct Pasta {
    pub content: String,
    pub copied_count: i32,
    pub perma_id: String,
    pub id: i64,
    pub inserted_at: String
}

#[derive(Deserialize)]
pub struct LoginResponse {
    pub login_url: String
}

#[derive(Deserialize)]
pub struct UserInfo {
    //user_id: i64,
    pub username: String
}

#[derive(Deserialize)]
pub struct CreateStreamResponse {
    pub name: String
}

fn check_resp(resp: &mut reqwest::Response) -> Result<(), HeyError> {
    match resp.status() {
        StatusCode::OK =>
            Ok(()),
        StatusCode::FORBIDDEN => {
            let r: LoginResponse = resp.json()?;
            Err(HeyError::NotLoggedIn(r.login_url))
        },
        status =>
            Err(HeyError::ServerError(status))
    }
}
