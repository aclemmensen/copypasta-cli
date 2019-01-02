#[macro_use] extern crate log;

use std::error::Error;
use std::io::{self, Read};
use atty::Stream as AStream;
use clap::{Arg, App, SubCommand, AppSettings};
use term_size as ts;

mod streams;
mod client;

use self::client::{PastaClient, HeyError, ClientConfig};

const LOGIN_NAME: &str = "login";
const LIST_NAME: &str = "list";
const PRODUCE_NAME: &str = "produce";
const CONSUME_NAME: &str = "consume";

const DEFAULT_HOST: &str = "localhost:4000";
const DEFAULT_CONFIG: &str = ".pastaconfig";

fn main() {
    env_logger::init();

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
                streams::produce(app).unwrap();
            } else if let Some(_) = matches.subcommand_matches(CONSUME_NAME) {
                streams::consume(app, "x").unwrap();
            } else if let Some(_) = matches.subcommand_matches(LIST_NAME) {
                handle_list(&app);
            } else if let Some(_) = matches.subcommand_matches(LOGIN_NAME) {
                eprintln!("You are already logged in");
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
        Err(e) => {
            eprintln!("Unhandled error: {:#?}", e);
        }
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
            debug!("App already configured, returning");
            Ok(app)
        },
        Err(HeyError::NoConfigFound) => {
            debug!("No configuration found, creating one");
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
    }

    Ok(app)
}

fn verify_login(app: &mut PastaClient) -> Result<bool, HeyError> {
    match app.login() {
        Ok(_) => {
            debug!("Login successful, token not updated");
            Ok(false)
        },
        Err(HeyError::NotLoggedIn(login_url)) => {
            debug!("User not logged in, prompting for token");
            let token = prompt_token(login_url).unwrap();
            debug!("Received token \"{}\" from user", token);
            let host = match app.config {
                Some(ref c) => c.host.to_string(),
                None => DEFAULT_HOST.to_string()
            };

            let config = ClientConfig {
                token: token.trim().to_string(),
                host: host
            };

            debug!("Storing user config: {:?}", config);

            app.set_config(config);

            match app.login() {
                Ok(user) => {
                    debug!("Login test successful");
                    eprintln!("welcome, {}!", user.username);
                    Ok(true)
                },
                Err(e) =>  {
                    warn!("Login failed");
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

