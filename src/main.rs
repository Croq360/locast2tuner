#![recursion_limit = "256"]
#[macro_use]
extern crate log;
mod config;
mod credentials;
mod errors;
mod fcc_facilities;
mod http;
mod logging;
mod service;
mod utils;
use itertools::Itertools;
use service::multiplexer::Multiplexer;
use simple_error::SimpleError;
use std::env;
use std::sync::Arc;

const VERSION: &'static str = env!("CARGO_PKG_VERSION");
#[actix_web::main]
async fn main() -> Result<(), SimpleError> {
    // Create a configuration struct that we'll pass along throughout the application
    let conf = match config::Config::from_args_and_file() {
        Ok(c) => Arc::new(c),
        Err(e) => panic!("{}", e),
    };

    // Enable the RUST_BACKTRACE=1 env variable.
    if conf.rust_backtrace {
        env::set_var("RUST_BACKTRACE", "1");
    }

    // Log level 0 and 1 give info logging, but loglevel 1 adds HTTP logging.
    // Level 2 is debug and anything else defaults to trace.
    let log_level = match conf.verbose {
        0 | 1 => slog::Level::Info,
        2 => slog::Level::Debug,
        _ => slog::Level::Trace,
    };

    // Setup logging
    let logger = crate::logging::logger(log_level, &conf);
    let _scope_guard = slog_scope::set_global_logger(logger);
    let _log_guard = slog_stdlog::init().unwrap();

    info!(
        "locast2tuner {} on {} {} starting..",
        VERSION,
        sys_info::os_type().unwrap(),
        sys_info::os_release().unwrap()
    );

    info!("UUID: {}", conf.clone().uuid);

    // Login to locast and get credentials we pass around
    let credentials = Arc::new(credentials::LocastCredentials::new(conf.clone()).await);

    // Load FCC facilities
    let fcc_facilities = Arc::new(fcc_facilities::FCCFacilities::new(conf.clone()).await);

    // Create Locast Services
    let services = if let Some(zipcodes) = &conf.override_zipcodes {
        let services = zipcodes
            .into_iter()
            .map(|x| {
                service::LocastService::new(
                    conf.clone(),
                    credentials.clone(),
                    fcc_facilities.clone(),
                    Some(x.to_string()),
                )
            })
            .collect_vec();
        futures::future::join_all(services).await
    } else {
        vec![service::LocastService::new(conf.clone(), credentials, fcc_facilities, None).await]
    };

    // Create a multiplexer if necessary
    if conf.multiplex {
        if conf.remap {
            warn!("Channels will be remapped!");
        }
        let mp = vec![Multiplexer::new(services, conf.clone())];
        match http::start(mp, conf.clone()).await {
            Ok(()) => Ok(()),
            Err(_) => return Err(SimpleError::new("Failed to start servers")),
        }
    } else {
        match http::start(services, conf.clone()).await {
            Ok(()) => Ok(()),
            Err(_) => return Err(SimpleError::new("Failed to start servers")),
        }
    }
}
