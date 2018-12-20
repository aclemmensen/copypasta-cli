use std::fmt::Display;
use std::error::Error;
use serde_derive::{Serialize, Deserialize};
use reqwest::{StatusCode};
use std::io::{self};
use std::path::Path;

fn main() {

        Ok(app) => {
            match app.latest() {
                Ok(pasta) => println!("{}", pasta.content),
                Err(e) => eprintln!("Error: {:?}", e)
            };
        },
        Err(e) => eprintln!("Error: {:?}", e)
    }
}

fn get_app(path: String) -> Result<PastaClient, HeyError> {
    match PastaClient::from_config(&path) {
        Ok(mut app) => {
            if verify_login(&mut app)? {
                // We went through the token flow again so we
                // have to store an updated config
                app.save_config().unwrap();
            }
            
            Ok(app)
        },
        Err(HeyError::NoConfigFound) => {
            let client = reqwest::Client::new();
            let mut new_app = PastaClient::new(client, path);

            verify_login(&mut new_app)?;

            new_app.save_config().unwrap();

            Ok(new_app)
        },
        e => e
    }
}

fn verify_login(app: &mut PastaClient) -> Result<bool, HeyError> {
    match app.login() {
        Ok(_) => Ok(false),
        Err(HeyError::NotLoggedIn(login_url)) => {
            let token = prompt_token(login_url).unwrap();
            app.set_token(token.trim().to_string());

            match app.login() {
                Ok(user) => {
                    eprintln!("You are now logged in as {}!", user.username);
                    Ok(true)
                },
                Err(e) =>  {
                    eprintln!("An error occurred during login: {:?}", e);
                    Err(e)
                }
            }
        },
        Err(e) => Err(e)
    }
}

fn prompt_token(login_url: String) -> Result<String, Box<Error>> {
    eprintln!("You are not logged in. Please visit this URL in a browser:\n{}\n\nThen paste the token here:", login_url);
    let mut buffer = String::new();
    io::stdin().read_line(&mut buffer)?;
    Ok(buffer)
}

struct PastaClient {
    client: reqwest::Client,
    token: Option<String>,
    config_path: String
}

#[derive(Debug)]
enum HeyError {
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
    fn new(client: reqwest::Client, path: String) -> PastaClient {
        PastaClient {
            client,
            token: None,
            config_path: path
        }
    }

    fn from_config(path: &String) -> Result<PastaClient, HeyError> {
        let fspath = Path::new(path);
        if !fspath.is_file() {
            return Err(HeyError::NoConfigFound);
        }

        let content = std::fs::read_to_string(path).unwrap();
        let deser: ClientConfig = serde_json::from_str(&content).unwrap();

        Ok(PastaClient {
            token: Some(deser.token),
            client: reqwest::Client::new(),
            config_path: path.to_string()
        })
    }

    fn set_token(&mut self, token: String) -> () {
        self.token = Some(token);
    }
    
    fn login(&self) -> Result<UserInfo, HeyError> {
        let mut resp = self.add_token(self.client.get("http://localhost:4000/api"))
            .send()?;
        
        check_resp(&mut resp)?;

        let resp: UserInfo = resp.json()?;

        Ok(resp)
    }

    fn latest(&self) -> Result<Pasta, HeyError> {
        let mut resp = self.add_token(self.client.get("http://localhost:4000/api/latest"))
            .send()?;
        
        check_resp(&mut resp)?;

        let pasta: Pasta = resp.json()?;

        Ok(pasta)
    }

    fn save_config(&self) -> Result<(), Box<Error>> {
        if let Some(token) = &self.token {
            let conf = ClientConfig {
                token: token.to_string()
            };

            let ser = serde_json::to_string(&conf)?;
            std::fs::write(&self.config_path, &ser)?;

            Ok(())
        } else {
            Err(Box::new(HeyError::NoToken))
        }
    }

    fn add_token(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(token) = &self.token {
            return builder.bearer_auth(token);
        }

        return builder;
    }
}

#[derive(Deserialize, Serialize)]
struct ClientConfig {
    token: String
}

#[derive(Deserialize, Debug)]
struct Pasta {
    content: String,
    copied_count: i32,
    perma_id: String,
    id: i64,
    inserted_at: String
}

#[derive(Deserialize)]
struct LoginResponse {
    login_url: String
}

#[derive(Deserialize)]
struct UserInfo {
    //user_id: i64,
    username: String
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
