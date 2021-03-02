#![recursion_limit = "256"]
extern crate chrono;
extern crate chrono_tz;
mod config;
mod credentials;
mod fcc_facilities;
mod http;
mod multiplexer;
mod service;
mod streaming;
mod utils;
mod xml_templates;
use chrono::Local;
use env_logger::Builder;
use log::{info, LevelFilter};
use simple_error::SimpleError;
use std::io::Write;
use std::sync::Arc;

const VERSION: &'static str = env!("CARGO_PKG_VERSION");
fn main() -> Result<(), SimpleError> {
    let conf = Arc::new(config::Config::from_args_and_file()?);
    Builder::new()
        .format(|buf, record| {
            writeln!(
                buf,
                "{} [{}] - {}",
                Local::now().format("%Y-%m-%dT%H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .filter(None, LevelFilter::Info)
        .init();

    info!(
        "locast2tuner {} on {} {} starting..",
        VERSION,
        sys_info::os_type().unwrap(),
        sys_info::os_release().unwrap()
    );

    info!("UUID: {}", conf.uuid);

    // Login to locast and get credentials we pass around
    let credentials = Arc::new(credentials::LocastCredentials::new(conf.clone()));

    // Load FCC facilities
    let fcc_facilities = Arc::new(fcc_facilities::FCCFacilities::new(conf.clone()));

    // Create Locast Services
    let services = if let Some(zipcodes) = &conf.override_zipcodes {
        zipcodes
            .into_iter()
            .map(|x| {
                service::LocastService::new(
                    conf.clone(),
                    credentials.clone(),
                    fcc_facilities.clone(),
                    Some(x.to_string()),
                )
            })
            .collect()
    } else {
        vec![service::LocastService::new(
            conf.clone(),
            credentials,
            fcc_facilities,
            None,
        )]
    };

    let mp = vec![multiplexer::Multiplexer::new(
        services.clone(),
        conf.clone(),
    )];

    if conf.multiplex {
        match http::start(mp, conf.clone()) {
            Ok(()) => Ok(()),
            Err(_) => return Err(SimpleError::new("Failed to start servers")),
        }
    } else {
        match http::start(services, conf.clone()) {
            Ok(()) => Ok(()),
            Err(_) => return Err(SimpleError::new("Failed to start servers")),
        }
    }
}
