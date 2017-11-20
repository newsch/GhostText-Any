extern crate env_logger;
extern crate structopt;

extern crate ghost_text_file;

use structopt::StructOpt;

fn main() {
    env_logger::init().unwrap();

    let options = ghost_text_file::options::Options::from_args();

    ghost_text_file::http_server::launch_server(options);
}
