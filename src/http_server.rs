use std::io::prelude::*;

use hyper::header;
use hyper::server::{Request, Response, Server};
use serde_json;
use websocket::server::upgrade::HyperIntoWsError;
use websocket::sync::server::upgrade::{HyperRequest, IntoWs};

use options;
use ws_messages;
use ws_server;

pub fn launch_server(options: options::Options) {
    info!("Starting server on port {}", options.port);
    Server::http(format!("127.0.0.1:{}", options.port))
        .unwrap()
        .handle(move |req: Request, mut res: Response| {
            match HyperRequest(req).into_ws() {
                Ok(upgrade) => {
                    match upgrade.accept() {
                        Ok(client) => ws_server::ws_thread(client),
                        Err((_, err)) => {
                            error!("Unable to upgrade websocket - {:?}", err);
                        }
                    };
                }

                Err((mut request, HyperIntoWsError::NoSecWsKeyHeader)) => {
                    let mut req_body = String::new();
                    request.read_to_string(&mut req_body).unwrap();
                    res.headers_mut().set(header::ContentType::json());
                    res.send(
                        &serde_json::to_vec(&ws_messages::RedirectToWebSocket {
                            WebSocketPort: options.port,
                            ProtocolVersion: 1,
                        })
                        .unwrap(),
                    )
                    .unwrap();
                }
                Err((_, err)) => {
                    error!("Unable to open websocket - {:?}", err);
                }
            };
        })
        .unwrap();
}
