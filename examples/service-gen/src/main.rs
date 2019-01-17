extern crate futures;
#[macro_use]
extern crate prost_derive;
extern crate twirp_rs;

use futures::Future;
use futures::future;
use futures::sync::oneshot;
use hyper::{Client, Server};
use std::env;
use std::thread;
use std::time::Duration;

extern crate prost;
extern crate hyper;

mod service {
    include!(concat!(env!("OUT_DIR"), "/twitch.twirp.example.rs"));
}

fn main() {
    println!("Starting server");
    let addr = "0.0.0.0:8080".parse().unwrap();
    let service = service::Haberdasher::new_server(HaberdasherService);
    let server = Server::bind(&addr).serve(service).map_err(|e| eprintln!("server error: {}", e));

    hyper::rt::run(server);
}

pub struct HaberdasherService;
impl service::Haberdasher for HaberdasherService {
    fn make_hat(&self, i: service::PTReq<service::Size>) -> service::PTRes<service::Hat> {
        Box::new(future::ok(
            service::Hat { size: i.input.inches, color: "blue".to_string(), name: "fedora".to_string() }.into()
        ))
    }
}
