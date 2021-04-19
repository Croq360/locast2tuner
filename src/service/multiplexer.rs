use crate::{
    config::Config,
    errors::AppError,
    service::{Geo, LocastServiceArc, Station, StationProvider, Stations},
};
use async_trait::async_trait;
use futures::lock::Mutex;
use log::info;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs::File, sync::Arc};
/// Multiplex `LocastService` objects. `Multiplexer` implements the `StationProvider` trait
/// and can act as a LocastService.
pub struct Multiplexer {
    services: Vec<LocastServiceArc>,
    config: Arc<Config>,
    station_id_service_map: Mutex<HashMap<String, LocastServiceArc>>,
    channel_remap: Option<HashMap<String, ChannelRedef>>,
}

impl Multiplexer {
    /// Create a new `Multiplexer` with a vector of `LocastServiceArcs` and a `Config`
    pub fn new(services: Vec<LocastServiceArc>, config: Arc<Config>) -> MultiplexerArc {
        let channel_remap = match &config.remap_file {
            Some(f) => {
                let file = File::open(f).unwrap();
                let c: HashMap<String, ChannelRedef> = serde_json::from_reader(file).unwrap();
                Some(c)
            }
            None => None,
        };
        Arc::new(Multiplexer {
            services,
            config,
            station_id_service_map: Mutex::new(HashMap::new()),
            channel_remap,
        })
    }
}

type MultiplexerArc = Arc<Multiplexer>;
#[async_trait]
impl StationProvider for Arc<Multiplexer> {
    /// Get the stream URL for a locast station id.
    async fn station_stream_uri(&self, id: &str) -> Result<Mutex<String>, AppError> {
        // Make sure the station_id_service_map is loaded. Feels wrong to do it like this though.. Needs refactoring.
        self.stations().await;

        let service = match self
            .station_id_service_map
            .lock()
            .await
            .get(&id.to_string())
        {
            Some(s) => s.clone(),
            None => return Err(AppError::NotFound),
        };

        service.station_stream_uri(id).await
    }

    /// Get all stations for all `LocastService`s.
    async fn stations(&self) -> Stations {
        let mut all_stations: Vec<Station> = Vec::new();
        let services = self.services.clone();
        let services_len = services.len();
        for (i, service) in services.into_iter().enumerate() {
            let stations_mutex = service.stations().await;

            let stations = stations_mutex.lock().await;
            for mut station in stations.iter().map(|s| s.clone()) {
                if self.config.remap {
                    let channel = station.channel.as_ref().unwrap();
                    if let Ok(c) = channel.parse::<usize>() {
                        station.channel_remapped = Some((c + 100 * i).to_string());
                    } else if let Ok(c) = channel.parse::<f32>() {
                        station.channel_remapped = Some((c + 100.0 * i as f32).to_string());
                    } else {
                        panic!("Could not remap {}", channel);
                    };

                    // Convoluted.. let's fix this sometime..
                    let new_call_sign = station
                        .callSign
                        .replace(channel, &station.channel_remapped.as_ref().unwrap());
                    station.callSign_remapped = Some(new_call_sign);
                } else if self.channel_remap.is_some() {
                    // Look if the channel is is remapped in the channel map
                    let channel_remap = self.channel_remap.as_ref().unwrap();
                    let key = format!("channel.{}", station.id);
                    match channel_remap.get(&key) {
                        Some(r) if r.active => {
                            station.channel_remapped = Some(r.remap_channel.clone());
                            station.callSign_remapped =
                                Some(format!("{} {}", r.remap_channel, r.remap_call_sign));
                            debug!(
                                "Remap -  {} {} => {} {}",
                                station.channel.clone().unwrap(),
                                station.callSign,
                                station.channel_remapped.clone().unwrap(),
                                station.callSign_remapped.clone().unwrap()
                            );
                        }
                        _ => {}
                    }
                }
                self.station_id_service_map
                    .lock()
                    .await
                    .insert(station.id.to_string(), service.clone());
                all_stations.push(station);
            }
        }
        info!(
            "Got {} stations for {} cities",
            all_stations.len(),
            services_len
        );
        Arc::new(Mutex::new(all_stations))
    }

    fn geo(&self) -> Arc<crate::service::Geo> {
        Arc::new(Geo {
            latitude: 0.0,
            longitude: 0.0,
            DMA: "000".to_string(),
            name: "Multiplexer".to_string(),
            active: true,
            timezone: None,
        })
    }

    fn uuid(&self) -> String {
        self.config.uuid.to_owned()
    }

    fn zipcode(&self) -> String {
        "".to_string()
    }

    fn services(&self) -> Vec<LocastServiceArc> {
        self.services.clone()
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct ChannelRedef {
    original_call_sign: String,
    remap_call_sign: String,
    original_channel: String,
    remap_channel: String,
    active: bool,
}
