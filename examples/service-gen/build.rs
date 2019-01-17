extern crate prost_build;
extern crate twirp_rs;

fn main() {
    let mut conf = prost_build::Config::new();
    conf.service_generator(Box::new(twirp_rs::TwirpServiceGenerator::new()));
    conf.compile_protos(&["service.proto"], &["../"]).unwrap();
}
