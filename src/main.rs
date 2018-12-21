use phoenix::PhoenixEvent;
use std::fmt::Display;
use std::error::Error;
use serde_derive::{Serialize, Deserialize};
use reqwest::{StatusCode};
use std::io::{self, Read, Write};
use std::path::Path;
use std::collections::HashMap;
use atty::Stream as AStream;
use clap::{Arg, App, SubCommand};
use term_size as ts;

use phoenix::{Phoenix, Event};
use websocket::futures::sync::mpsc::channel;
use tokio_core::reactor::Core;
use futures::stream::Stream;

fn main() {
    let matches = App::new("Copypasta")
        .version("1.0")
        .author("@aclemmensen")
        .about("Copies your pastas")
        .arg(Arg::with_name("config")
            .short("c")
            .long("config")
            .value_name("FILE")
            .help("Sets a custom config file")
            .takes_value(true))
        .subcommand(SubCommand::with_name("list")
            .about("Lists your pasta"))
        .subcommand(SubCommand::with_name("produce"))
        .subcommand(SubCommand::with_name("consume"))
        .get_matches();

    if let Some(_) = matches.subcommand_matches("produce") {
        produce();
    }

    if let Some(_) = matches.subcommand_matches("consume") {
        consume();
    }

    return;


    let config_file = matches.value_of("config").unwrap_or(".pastaconfig");

    match get_app(config_file.to_string()) {
        Ok(app) => {
            if atty::is(AStream::Stdin) {
                if let Some(_) = matches.subcommand_matches("list") {
                    let lst = app.list().unwrap();
                    let w = ts::dimensions()
                        .map(|(w, _)| w)
                        .unwrap_or(80);
                    
                    for p in lst {
                        let content = p.content.replace("\n", "\\n").replace("\t", " ").to_string();
                        let w = std::cmp::min(w, content.chars().count());
                        let w = w-6;
                        let w = std::cmp::max(w, 0);
                        let snippet = &content[0..w];
                        println!("{:05} {}", p.id, snippet);
                    }
                } else {
                    match app.latest() {
                        Ok(pasta) => println!("{}", pasta.content),
                        Err(e) => eprintln!("Error: {:?}", e)
                    };
                }
            } else {
                let input = read_all_input().unwrap();
                app.post(input).unwrap();
            }
        },
        Err(e) => eprintln!("Error: {:?}", e)
    }
}

#[derive(Copy, Clone, Debug)]
enum ProducerState  {
    ReadyToProduce,
    WaitingForAck,
    Producing,
    Done
}

#[derive(Copy, Clone)]
enum ConsumerState {
    ReadyToConsume,
    Consuming,
    Done
}

enum StreamEvent<'a> {
    Custom(&'a str),
    Defined(&'a PhoenixEvent)
}

fn into_good<'a>(evt: &'a Event) -> StreamEvent {
    match evt {
        Event::Custom(x) => StreamEvent::Custom(x.as_ref()),
        Event::Defined(x) => StreamEvent::Defined(x)
    }
}

fn send_msg(chan: &std::sync::Arc<std::sync::Mutex<phoenix::chan::Channel>>, msg_type: &str, payload: &str) {
    let mut chan = chan.lock().unwrap();
    let body = serde_json::from_str(payload).unwrap();
    chan.send(Event::Custom(msg_type.to_string()), &body);
}

fn send_msg2(chan: &mut phoenix::chan::Channel, msg_type: &str, payload: &str) {
    let body = serde_json::from_str(payload).unwrap();
    chan.send(Event::Custom(msg_type.to_string()), &body);
}

fn produce() {
    let (sender, emitter) = channel(0);
    let (callback, messages) = channel(0);

    let mut p = HashMap::new();
    p.insert("token", "xxx");

    let mut phx = Phoenix::new_with_parameters(&sender, emitter, &callback, "ws://localhost:4000/socket", &p);
    let chan = phx.channel("streams:x").clone();
    {
        let mut chan = chan.lock().unwrap();
        chan.join();
    }

    send_msg(&chan, "producer_join", "{}");
    let output_buffer = String::new();
    let input_buffer = vec![0; 1_000_000];
    let mut chan = chan.lock().unwrap();

    let runner = messages.fold((ProducerState::ReadyToProduce, input_buffer, output_buffer), |(state, mut in_buf, mut out_buf), message| {
        // eprintln!("SAD {:#?} {:?}", message, state); 
        match (state, into_good(&message.event)) {
            (ProducerState::ReadyToProduce, StreamEvent::Custom("bytes_requested")) => {
                match io::stdin().read(&mut in_buf) {
                    Ok(0) => {
                        send_msg2(&mut chan, "done", "{}");
                        Err(())
                    },
                    Ok(n) => {
                        out_buf.clear();
                        out_buf.push('"');
                        base64::encode_config_buf(&in_buf[0..n], base64::STANDARD, &mut out_buf);
                        out_buf.push('"');
                        send_msg2(&mut chan, "bytes", &out_buf);
                        Ok((ProducerState::ReadyToProduce, in_buf, out_buf))
                    },
                    Err(_) => {
                        send_msg2(&mut chan, "done", "{\"error\": true}");
                        Err(())
                    }
                }
            },
            _ => Ok((state, in_buf, out_buf))
        }
    });

    let mut core = Core::new().unwrap();
    core.run(runner).map(|_| ()).unwrap_or(());

    eprintln!("Done producing");
}

fn consume() {
    let (sender, emitter) = channel(0);
    let (callback, messages) = channel(0);

    let mut p = HashMap::new();
    p.insert("token", "xxx");

    let mut phx = Phoenix::new_with_parameters(&sender, emitter, &callback, "ws://localhost:4000/socket", &p);
    let chan = phx.channel("streams:x").clone();
    {
        let mut chan = chan.lock().unwrap();
        chan.join();
    }

    send_msg(&chan, "consumer_join", "{}");
    send_msg(&chan, "request_bytes", "{}");

    let mut chan = chan.lock().unwrap();

    let runner = messages.fold(ConsumerState::Consuming, |state, message| {
        // eprintln!("{:#?}", message);
        match (state, into_good(&message.event)) {
            (ConsumerState::Consuming, StreamEvent::Custom("bytes")) => {
                // eprintln!("Got some bytes!");
                let x = message.payload.get("data").unwrap().as_str().unwrap();
                let decoded = base64::decode(x).unwrap();
                io::stdout().write(&decoded).unwrap();
                send_msg2(&mut chan, "request_bytes", "{}");
                Ok(ConsumerState::Consuming)
            },
            (ConsumerState::Consuming, StreamEvent::Custom("no_more_data")) => {
                eprintln!("Ate all the bytes");
                Err(())
            }
            _ => Ok(state)
        }
    });

    let mut core = Core::new().unwrap();
    core.run(runner).map(|_| ()).unwrap_or(());

    eprintln!("Done consuming");
}

fn read_all_input() -> Result<String, Box<Error>> {
    let mut buffer = String::new();
    io::stdin().read_to_string(&mut buffer)?;
    Ok(buffer)
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

    fn list(&self) -> Result<Vec<Pasta>, HeyError> {
        let mut resp = self.add_token(self.client.get("http://localhost:4000/api/list"))
            .send()?;
        
        check_resp(&mut resp)?;

        let pastas: Vec<Pasta> = resp.json()?;

        Ok(pastas)
    }

    fn post(&self, content: String) -> Result<(), HeyError> {
        let msg = CreatePasta {
            content
        };

        let mut resp = self.add_token(self.client.post("http://localhost:4000/api/create"))
            .json(&msg)
            .send()?;
        
        check_resp(&mut resp)
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

#[derive(Serialize)]
struct CreatePasta {
    content: String
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
