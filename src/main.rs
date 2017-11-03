extern crate env_logger;

extern crate ghost_text_file;

fn main() {
    env_logger::init().unwrap();
    ghost_text_file::http_server::launch_server(4001);
}
