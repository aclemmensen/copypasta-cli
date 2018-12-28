use std::fmt::Display;
use std::error::Error;
use serde_derive::{Serialize, Deserialize};
use reqwest::{StatusCode};
use std::io::{self, Read};
use std::path::Path;
use atty::Stream as AStream;
use clap::{Arg, App, SubCommand, AppSettings};
use term_size as ts;

mod streams;

const LOGIN_NAME: &str = "login";
const LIST_NAME: &str = "list";
const PRODUCE_NAME: &str = "produce";
const CONSUME_NAME: &str = "consume";

const DEFAULT_SCHEME: &str = "http";
const DEFAULT_HOST: &str = "localhost:4000";
const DEFAULT_CONFIG: &str = ".pastaconfig";

fn main() {
    let matches = App::new("Copypasta")
        .version("1.0")
        .author("@aclemmensen")
        .about("Copies your pastas")
        .setting(AppSettings::DeriveDisplayOrder)
        .arg(Arg::with_name("config")
            .short("c")
            .long("config")
            .value_name("FILE")
            .help("Sets a custom config file")
            .takes_value(true))
        .subcommand(SubCommand::with_name(LOGIN_NAME)
            .about("Logs you into Copypasta"))
        .subcommand(SubCommand::with_name(LIST_NAME)
            .about("Lists your pasta"))
        .subcommand(SubCommand::with_name(PRODUCE_NAME)
            .about("Produce a stream through your Copypasta account"))
        .subcommand(SubCommand::with_name(CONSUME_NAME)
            .about("Consume a stream created by a Copypasta user"))
        .get_matches();

    let config_file = matches.value_of("config").unwrap_or(DEFAULT_CONFIG);

    match get_app_if_configured(&config_file) {
        Ok(app) => {
            if let Some(_) = matches.subcommand_matches(PRODUCE_NAME) {
                streams::produce();
            } else if let Some(_) = matches.subcommand_matches(CONSUME_NAME) {
                streams::consume();
            } else if let Some(_) = matches.subcommand_matches(LIST_NAME) {
                handle_list(&app);
            } else if let Some(_) = matches.subcommand_matches(LOGIN_NAME) {
                eprintln!("You are already logged");
            } else {
                handle_default(&app);
            }
        },
        Err(HeyError::NoConfigFound) => {
            if let Some(_) = matches.subcommand_matches(LOGIN_NAME) {
                match get_app(config_file.to_string()) {
                   Ok(_app) => {
                       eprintln!("You are now logged in");
                    },
                    Err(e) => {
                        eprintln!("{:?}", e);
                    }
                }
            } else {
                eprintln!("You are not logged in. Run `copypasta login` first.");
            }
        },
        Err(e) => panic!(e)
    }
}

fn handle_default(app: &PastaClient) {
    if atty::is(AStream::Stdin) {
        match app.latest() {
            Ok(pasta) => println!("{}", pasta.content),
            Err(e) => eprintln!("Error: {:?}", e)
        };
    } else {
        let input = read_all_input().unwrap();
        app.post(input).unwrap();
    }
}

fn handle_list(app: &PastaClient) {
    let lst = app.list().unwrap();
    let width = ts::dimensions()
        .map(|(w, _)| w)
        .unwrap_or(80);
    
    for p in lst {
        let content = p.content.replace("\n", "\\n").replace("\t", " ").to_string();
        let width = std::cmp::min(width, content.chars().count());
        let width = width-6;
        let width = std::cmp::max(width, 0);
        let snippet = &content[0..width];
        println!("{:05} {}", p.id, snippet);
    }
}

fn read_all_input() -> Result<String, Box<Error>> {
    let mut buffer = String::new();
    io::stdin().read_to_string(&mut buffer)?;
    Ok(buffer)
}

fn get_app(path: String) -> Result<PastaClient, HeyError> {
    match get_app_if_configured(&path) {
        Ok(app) => {
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

fn get_app_if_configured(path: &str) -> Result<PastaClient, HeyError> {
    let config = ClientConfig::load_from_file(path)?;
    build_and_verify(path, config)
}

fn build_and_verify(path: &str, config: ClientConfig) -> Result<PastaClient, HeyError> {
    let mut app = PastaClient {
        config: Some(config),
        client: reqwest::Client::new(),
        config_path: path.to_string()
    };

    if verify_login(&mut app)? {
        app.save_config().unwrap();
        return Ok(app)
    }

    Err(HeyError::LoginError)
}

fn verify_login(app: &mut PastaClient) -> Result<bool, HeyError> {
    match app.login() {
        Ok(_) => Ok(false),
        Err(HeyError::NotLoggedIn(login_url)) => {
            let token = prompt_token(login_url).unwrap();
            let host = match app.config {
                Some(ref c) => c.host.to_string(),
                None => DEFAULT_HOST.to_string()
            };

            app.set_config(ClientConfig {
                token: token.trim().to_string(),
                host: host
            });

            match app.login() {
                Ok(user) => {
                    eprintln!("welcome, {}!", user.username);
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
    config: Option<ClientConfig>,
    config_path: String,
}

#[derive(Deserialize, Serialize)]
struct ClientConfig {
    token: String,
    host: String
}

impl ClientConfig {
    fn load_from_file(path: &str) -> Result<ClientConfig, HeyError> {
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
            config: None,
            config_path: path
        }
    }

    fn set_config(&mut self, config: ClientConfig) -> () {
        self.config = Some(config);
    }
    
    fn login(&self) -> Result<UserInfo, HeyError> {
        let mut resp = self.add_token(self.client.get(&self.get_url("api")))
            .send()?;
        
        check_resp(&mut resp)?;

        let resp: UserInfo = resp.json()?;

        Ok(resp)
    }

    fn latest(&self) -> Result<Pasta, HeyError> {
        let mut resp = self.add_token(self.client.get(&self.get_url("api/latest")))
            .send()?;
        
        check_resp(&mut resp)?;

        let pasta: Pasta = resp.json()?;

        Ok(pasta)
    }

    fn list(&self) -> Result<Vec<Pasta>, HeyError> {
        let mut resp = self.add_token(self.client.get(&self.get_url("api/list")))
            .send()?;
        
        check_resp(&mut resp)?;

        let pastas: Vec<Pasta> = resp.json()?;

        Ok(pastas)
    }

    fn post(&self, content: String) -> Result<(), HeyError> {
        let msg = CreatePasta {
            content
        };

        let mut resp = self.add_token(self.client.post(&self.get_url("api/create")))
            .json(&msg)
            .send()?;
        
        check_resp(&mut resp)
    }

    fn save_config(&self) -> Result<(), Box<Error>> {
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
    content: String
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
