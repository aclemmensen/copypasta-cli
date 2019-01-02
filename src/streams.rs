use std::io::{self, Read, Write};
use std::collections::HashMap;
use phoenix::{Phoenix, Event, PhoenixEvent};
use websocket::futures::sync::mpsc::channel;
use tokio_core::reactor::Core;
use futures::stream::Stream;

use super::client::{PastaClient, HeyError};

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

pub fn produce(client: PastaClient) -> Result<(), HeyError> {
    let (sender, emitter) = channel(0);
    let (callback, messages) = channel(0);

    let url = client.get_socket_url()?;
    debug!("Socket URL will be {}", url);

    let token = client.get_token().expect("no token provided");
    debug!("Loaded user token {}", token);

    let create_stream = client.create_stream()?;
    debug!("Created stream id {}", create_stream.name);

    let topic_name = format!("streams:{}", create_stream.name);
    debug!("Will join channel {}", topic_name);

    eprintln!("Created stream {}", create_stream.name);

    let mut p = HashMap::new();
    p.insert("token", token.as_str());

    let mut phx = Phoenix::new_with_parameters(&sender, emitter, &callback, &url, &p);
    let chan = phx.channel(&topic_name).clone();
    {
        let mut chan = chan.lock().unwrap();
        chan.join();
    }

    debug!("Sending producer_join message");
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
                        debug!("No more input, sending done message");
                        send_msg2(&mut chan, "done", "{}");
                        Err(())
                    },
                    Ok(n) => {
                        out_buf.clear();
                        out_buf.push('"');
                        base64::encode_config_buf(&in_buf[0..n], base64::STANDARD, &mut out_buf);
                        out_buf.push('"');
                        debug!("Sending bytes buffer (len {})", out_buf.len());
                        send_msg2(&mut chan, "bytes", &out_buf);
                        Ok((ProducerState::ReadyToProduce, in_buf, out_buf))
                    },
                    Err(e) => {
                        warn!("Error reading, sending done. Error: {:?}", e);
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

    Ok(())
}

pub fn consume(client: PastaClient, stream_name: &str) -> Result<(), HeyError> {
    let (sender, emitter) = channel(0);
    let (callback, messages) = channel(0);
    
    let url = client.get_socket_url()?;
    let token = client.get_token().expect("no token provided");

    let mut p = HashMap::new();
    p.insert("token", token.as_str());
    
    let mut phx = Phoenix::new_with_parameters(&sender, emitter, &callback, &url, &p);
    let chan = phx.channel(&format!("streams:{}", stream_name)).clone();
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

    Ok(())
}