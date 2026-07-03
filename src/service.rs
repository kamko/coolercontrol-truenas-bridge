use crate::config::{Config, SERVICE_ID};
use crate::device_service::v1::device_service_server::DeviceService;
use crate::device_service::v1::{
    CustomFunctionOneRequest, CustomFunctionOneResponse, EnableManualFanControlRequest,
    EnableManualFanControlResponse, FixedDutyRequest, FixedDutyResponse, HealthRequest,
    HealthResponse, InitializeDeviceRequest, InitializeDeviceResponse, LcdRequest, LcdResponse,
    LightingRequest, LightingResponse, ListDevicesRequest, ListDevicesResponse,
    ResetChannelRequest, ResetChannelResponse, ShutdownRequest, ShutdownResponse,
    SpeedProfileRequest, SpeedProfileResponse, StatusRequest, StatusResponse, health_response,
};
use crate::models::v1::status::Metric;
use crate::models::v1::{Device, DeviceInfo, DriverInfo, Status, TempInfo};
use crate::truenas::{DiskReading, TrueNasClient};
use log::warn;
use std::collections::{BTreeMap, HashMap};
use std::time::Instant;
use tokio::sync::Mutex;
use tonic::{Request, Response, Status as TonicStatus};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEVICE_ID: &str = "truenas";

#[derive(Debug)]
struct TempCache {
    readings: BTreeMap<String, DiskReading>,
    last_success: Option<Instant>,
    last_attempt: Option<Instant>,
    last_error: Option<String>,
}

pub struct TrueNasDeviceService {
    config: Config,
    client: TrueNasClient,
    cache: Mutex<TempCache>,
    started_at: Instant,
}

impl TrueNasDeviceService {
    pub fn new(config: Config) -> Self {
        let client = TrueNasClient::new(config.truenas.clone(), config.polling.connect_timeout());
        let configured_disks = config
            .truenas
            .disk_names
            .iter()
            .cloned()
            .map(|disk| {
                (
                    disk.clone(),
                    DiskReading {
                        temperature_c: config.polling.failsafe_temperature_c,
                        label: disk,
                    },
                )
            })
            .collect();

        Self {
            config,
            client,
            cache: Mutex::new(TempCache {
                readings: configured_disks,
                last_success: None,
                last_attempt: None,
                last_error: None,
            }),
            started_at: Instant::now(),
        }
    }

    async fn refresh_if_needed(&self) {
        let should_refresh = {
            let cache = self.cache.lock().await;
            cache
                .last_attempt
                .map(|last| last.elapsed() >= self.config.polling.poll_interval())
                .unwrap_or(true)
        };

        if !should_refresh {
            return;
        }

        match self.client.disk_temperatures().await {
            Ok(temperatures) => {
                let mut cache = self.cache.lock().await;
                cache.readings = temperatures;
                cache.last_attempt = Some(Instant::now());
                cache.last_success = Some(Instant::now());
                cache.last_error = None;
            }
            Err(err) => {
                warn!("failed to fetch TrueNAS disk temperatures: {err:?}");
                let mut cache = self.cache.lock().await;
                cache.last_attempt = Some(Instant::now());
                cache.last_error = Some(err.to_string());
                if cache
                    .last_success
                    .map(|last| last.elapsed() > self.config.polling.stale_after())
                    .unwrap_or(true)
                {
                    if cache.readings.is_empty() {
                        cache.readings.insert(
                            "failsafe".to_string(),
                            DiskReading {
                                temperature_c: self.config.polling.failsafe_temperature_c,
                                label: "failsafe".to_string(),
                            },
                        );
                    } else {
                        for reading in cache.readings.values_mut() {
                            reading.temperature_c = self.config.polling.failsafe_temperature_c;
                        }
                    }
                }
            }
        }
    }

    async fn device(&self) -> Device {
        self.refresh_if_needed().await;
        let cache = self.cache.lock().await;

        let temps = cache
            .readings
            .iter()
            .enumerate()
            .map(|(index, (disk, reading))| {
                (
                    temp_id(disk),
                    TempInfo {
                        label: reading.label.clone(),
                        number: (index + 1) as u32,
                    },
                )
            })
            .collect::<HashMap<_, _>>();

        Device {
            id: DEVICE_ID.to_string(),
            name: "TrueNAS".to_string(),
            uid_info: Some(self.config.truenas.host.clone()),
            info: Some(DeviceInfo {
                channels: HashMap::new(),
                temps,
                lighting_speeds: vec![],
                temp_min: Some(0.0),
                temp_max: Some(100.0),
                profile_min_length: None,
                profile_max_length: None,
                model: Some("TrueNAS Disk Temperatures".to_string()),
                driver_info: Some(DriverInfo {
                    name: Some(SERVICE_ID.to_string()),
                    version: Some(VERSION.to_string()),
                    locations: vec![self.config.truenas.host.clone()],
                }),
            }),
        }
    }
}

#[tonic::async_trait]
impl DeviceService for TrueNasDeviceService {
    async fn health(
        &self,
        _request: Request<HealthRequest>,
    ) -> Result<Response<HealthResponse>, TonicStatus> {
        let cache = self.cache.lock().await;
        let status = if cache.last_error.is_some() {
            health_response::Status::Warning
        } else {
            health_response::Status::Ok
        };

        Ok(Response::new(HealthResponse {
            name: SERVICE_ID.to_string(),
            version: VERSION.to_string(),
            status: status.into(),
            uptime_seconds: self.started_at.elapsed().as_secs(),
        }))
    }

    async fn list_devices(
        &self,
        _request: Request<ListDevicesRequest>,
    ) -> Result<Response<ListDevicesResponse>, TonicStatus> {
        Ok(Response::new(ListDevicesResponse {
            devices: vec![self.device().await],
        }))
    }

    async fn initialize_device(
        &self,
        _request: Request<InitializeDeviceRequest>,
    ) -> Result<Response<InitializeDeviceResponse>, TonicStatus> {
        self.refresh_if_needed().await;
        Ok(Response::new(InitializeDeviceResponse {}))
    }

    async fn shutdown(
        &self,
        _request: Request<ShutdownRequest>,
    ) -> Result<Response<ShutdownResponse>, TonicStatus> {
        Ok(Response::new(ShutdownResponse {}))
    }

    async fn status(
        &self,
        request: Request<StatusRequest>,
    ) -> Result<Response<StatusResponse>, TonicStatus> {
        if request.get_ref().device_id != DEVICE_ID {
            return Err(TonicStatus::not_found("device not found"));
        }

        self.refresh_if_needed().await;
        let cache = self.cache.lock().await;
        let status = cache
            .readings
            .iter()
            .map(|(disk, reading)| Status {
                id: temp_id(disk),
                metric: Some(Metric::Temp(reading.temperature_c)),
            })
            .collect();

        Ok(Response::new(StatusResponse { status }))
    }

    async fn reset_channel(
        &self,
        _request: Request<ResetChannelRequest>,
    ) -> Result<Response<ResetChannelResponse>, TonicStatus> {
        Ok(Response::new(ResetChannelResponse {}))
    }

    async fn enable_manual_fan_control(
        &self,
        _request: Request<EnableManualFanControlRequest>,
    ) -> Result<Response<EnableManualFanControlResponse>, TonicStatus> {
        Err(TonicStatus::unimplemented(
            "TrueNAS exposes temperatures only",
        ))
    }

    async fn fixed_duty(
        &self,
        _request: Request<FixedDutyRequest>,
    ) -> Result<Response<FixedDutyResponse>, TonicStatus> {
        Err(TonicStatus::unimplemented(
            "TrueNAS exposes temperatures only",
        ))
    }

    async fn speed_profile(
        &self,
        _request: Request<SpeedProfileRequest>,
    ) -> Result<Response<SpeedProfileResponse>, TonicStatus> {
        Err(TonicStatus::unimplemented(
            "TrueNAS exposes temperatures only",
        ))
    }

    async fn lighting(
        &self,
        _request: Request<LightingRequest>,
    ) -> Result<Response<LightingResponse>, TonicStatus> {
        Err(TonicStatus::unimplemented(
            "TrueNAS exposes temperatures only",
        ))
    }

    async fn lcd(
        &self,
        _request: Request<LcdRequest>,
    ) -> Result<Response<LcdResponse>, TonicStatus> {
        Err(TonicStatus::unimplemented(
            "TrueNAS exposes temperatures only",
        ))
    }

    async fn custom_function_one(
        &self,
        _request: Request<CustomFunctionOneRequest>,
    ) -> Result<Response<CustomFunctionOneResponse>, TonicStatus> {
        Err(TonicStatus::unimplemented("no custom functions"))
    }
}

fn temp_id(disk_name: &str) -> String {
    let safe = disk_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches(['_', '-', '.'])
        .to_string();

    if safe.is_empty() {
        "disk".to_string()
    } else {
        safe
    }
}

#[cfg(test)]
mod tests {
    use super::temp_id;

    #[test]
    fn sanitizes_temp_ids() {
        assert_eq!(temp_id("sda"), "sda");
        assert_eq!(temp_id("disk bay/1"), "disk_bay_1");
    }
}
