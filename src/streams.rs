use std::io::{self, Read, Write};
use std::collections::HashMap;
use phoenix::{Phoenix, Event, PhoenixEvent};
use websocket::futures::sync::mpsc::channel;
use tokio_core::reactor::Core;
use futures::stream::Stream;

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

pub fn produce() {
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

pub fn consume() {
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