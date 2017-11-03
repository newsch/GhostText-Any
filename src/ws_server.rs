use websocket;
use websocket::{Message, OwnedMessage};
use serde_json;

use ws_messages;
use run_editor;

pub fn ws_thread<S: websocket::stream::Stream>(mut client: websocket::client::sync::Client<S>) {
    let message = client.recv_message().unwrap();
    let message: ws_messages::GetTextFromComponent = match message {
        OwnedMessage::Text(raw_json) => {
            serde_json::from_str(&raw_json).unwrap()
        },
        _ => panic!(),
    };

    info!("Received request with url={} and title={:?}", message.url, message.title);

    let resulting_text = run_editor::run(message.url, message.title, message.text);

    client.send_message(&Message::text(serde_json::to_string(&ws_messages::SetTextInComponent {
        text: &resulting_text,
    }).unwrap())).unwrap();

    client.send_message(&Message::ping("".to_string().into_bytes())).unwrap();
    let message = client.recv_message().unwrap();
    trace!("Ping received: {:?}", message);

    info!("Closing connection");
    client.send_message(&Message::close()).unwrap();
}
