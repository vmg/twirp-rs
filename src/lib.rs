#![recursion_limit="128"]
extern crate prost;

extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;

#[cfg(feature = "service-gen")]
#[macro_use]
extern crate quote;

#[cfg(feature = "service-gen")]
mod service_gen;

#[cfg(feature = "service-gen")]
pub use self::service_gen::TwirpServiceGenerator;

mod service_run;
pub use self::service_run::*;
