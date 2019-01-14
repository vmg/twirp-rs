extern crate prost;
extern crate serde_json;

#[cfg(feature = "service-gen")]
extern crate prost_build;

#[cfg(feature = "service-gen")]
mod service_gen;


#[cfg(feature = "service-gen")]
pub use self::service_gen::TwirpServiceGenerator;

mod service_run;
pub use self::service_run::*;
