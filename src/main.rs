use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::time::Duration;

use async_stream::{stream, try_stream};
use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral, ScanFilter};
use btleplug::platform::{Manager, PeripheralId};
use btsensor::Reading;
use clap::Parser;
use color_eyre::eyre;
use color_eyre::eyre::eyre;
use eyre::Result;
use futures_core::stream::Stream;
use futures_util::pin_mut;
use futures_util::stream::StreamExt;
use rumqttc::{AsyncClient, MqttOptions, QoS};
use ruuvi_sensor_protocol::{BatteryPotential, Humidity, SensorValues, Temperature};
use serde::{Deserialize, Serialize};
use tokio::task;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "lowercase")]
pub struct DeviceId {
    peripheral_id: Uuid,
    device_name: String,
}

pub enum DeviceEvent {
    ManufacturerDataAdvertisement {
        device_id: DeviceId,
        manufacturer_data: HashMap<u16, Vec<u8>>,
    },

    ServiceDataAdvertisement {
        device_id: DeviceId,
        service_data: HashMap<Uuid, Vec<u8>>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum Measurement {
    Humidity(f64),
    Temperature(f64),
    Battery(f64),
    Voltage(f64),
}

impl Display for Measurement {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Measurement::Humidity(v) => f.write_fmt(format_args!("humidity {}%", v)),
            Measurement::Temperature(v) => f.write_fmt(format_args!("temperature {}°C", v)),
            Measurement::Battery(v) => f.write_fmt(format_args!("battery {}%", v)),
            Measurement::Voltage(v) => f.write_fmt(format_args!("voltage {}V", v)),
        }
    }
}

impl Measurement {
    pub fn kind(&self) -> impl ToString {
        match self {
            Measurement::Humidity(_) => "humidity",
            Measurement::Temperature(_) => "temperature",
            Measurement::Battery(_) => "battery",
            Measurement::Voltage(_) => "voltage",
        }
    }

    pub fn value(&self) -> f64 {
        match self {
            Measurement::Humidity(v) => *v,
            Measurement::Temperature(v) => *v,
            Measurement::Battery(v) => *v,
            Measurement::Voltage(v) => *v,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub struct DeviceReading {
    #[serde(flatten)]
    device_id: DeviceId,
    #[serde(flatten)]
    measurement: Measurement,
}

impl Display for DeviceReading {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{:?} -> {}", self.device_id, self.measurement))
    }
}

// bt_stream builds a stream of DeviceEvents, which are CentralEvents of interest augmented with
// device names rather than IDs.
fn bt_stream() -> impl Stream<Item = Result<DeviceEvent>> {

    try_stream! {
        let manager = Manager::new().await?;
        let adapters = manager.adapters().await?;
        let central = adapters.into_iter().next().ok_or(eyre!("No BT Adapter"))?;
        let events = central.events().await?;
        let mut device_names = HashMap::<PeripheralId, DeviceId>::new();
        central.start_scan(ScanFilter::default()).await?;

        for await event in events {
            match event {
                CentralEvent::DeviceDiscovered(id) => {
                    let peripheral = central.peripheral(&id).await?;
                    if let Some(prop) = peripheral.properties().await? {
                        if let Some(device_name) = prop.local_name {
                            match Uuid::try_parse_ascii(id.to_string().as_bytes()) {
                                Ok(peripheral_id) => {
                                    device_names.insert(id, DeviceId{peripheral_id, device_name});
                                },
                                Err(e) => {
                                    println!("parsing uuid error {:?} {:?}", id.to_string(), e)
                                }
                            }
                            // let peripheral_id = Uuid::parse_str(id.to_string().as_str()).unwrap_or_default();

                        }
                    }
                }
                 CentralEvent::ServiceDataAdvertisement { id, service_data } => {
                     if let Some(device_id) = device_names.get(&id) {
                        let device_id = device_id.clone();
                        yield DeviceEvent::ServiceDataAdvertisement {device_id, service_data };
                    }
                }
                CentralEvent::ManufacturerDataAdvertisement { id, manufacturer_data } => {
                     if let Some(device_id) = device_names.get(&id) {
                        let device_id = device_id.clone();
                        yield DeviceEvent::ManufacturerDataAdvertisement {device_id, manufacturer_data };
                    }
                }
                _ => {}
            }
        }
    }
}

fn device_reading_stream(
    event_stream: impl Stream<Item = Result<DeviceEvent>>,
) -> impl Stream<Item = DeviceReading> {
    stream! {
        for await event in event_stream {
            match event {
                Ok(DeviceEvent::ServiceDataAdvertisement { device_id, service_data }) => {
                    for measurement in measurements_from_service_data(service_data) {
                        let device_id = device_id.clone();
                        yield DeviceReading{device_id, measurement}
                    }
                }
                Ok(DeviceEvent::ManufacturerDataAdvertisement { device_id, manufacturer_data }) => {
                    for measurement in measurements_from_manufacturer_data(manufacturer_data) {
                        let device_id = device_id.clone();
                        yield DeviceReading{device_id, measurement}
                    }
                }
                Err(e) => {
                    println!("received error! {:?}", e.to_string())
                }
            }
        }
    }
}

fn measurements_from_manufacturer_data(
    manufacturer_data: HashMap<u16, Vec<u8>>,
) -> Vec<Measurement> {
    manufacturer_data
        .iter()
        .flat_map(|(id, data)| {
            let mut measurements: Vec<Measurement> = vec![];
            if let Ok(parsed) = SensorValues::from_manufacturer_specific_data(*id, data) {
                if let Some(humidity) = parsed.humidity_as_ppm() {
                    measurements.push(Measurement::Humidity(humidity as f64 / 10000.0));
                }

                if let Some(temp) = parsed.temperature_as_millicelsius() {
                    measurements.push(Measurement::Temperature(temp as f64 / 1000.0));
                }

                if let Some(batt) = parsed.battery_potential_as_millivolts() {
                    measurements.push(Measurement::Voltage(batt as f64 / 1000.0));
                }
            }
            measurements
        })
        .collect()
}

fn measurements_from_service_data(service_data: HashMap<Uuid, Vec<u8>>) -> Vec<Measurement> {
    if let Some(decoded) = Reading::decode(&service_data) {
        match decoded {
            Reading::BtHomeV2(v2) => {
                return v2
                    .elements
                    .iter()
                    .filter_map(|e| match e.name() {
                        "humidity" => Some(Measurement::Humidity(e.value_float().unwrap_or(0f64))),
                        "temperature" => {
                            Some(Measurement::Temperature(e.value_float().unwrap_or(0f64)))
                        }
                        "battery" => {
                            Some(Measurement::Battery(e.value_int().unwrap_or(0i64) as f64))
                        }
                        &_ => None,
                    })
                    .collect();
            }

            Reading::Atc(_) => {}
            Reading::BtHomeV1(_) => {}
        }
    }
    Vec::new()
}

#[derive(Parser, Debug)]
struct Args {
    client_id: String,
    #[arg(default_value = "starscourge.local")]
    mqtt_host: String,
    #[arg(default_value_t = 1883)]
    mqtt_port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let mut mqttoptions = MqttOptions::new(args.client_id, args.mqtt_host, args.mqtt_port);
    mqttoptions.set_keep_alive(Duration::from_secs(5));

    let (client, mut eventloop) = AsyncClient::new(mqttoptions, 10);

    task::spawn(async move {
        let events = bt_stream();
        pin_mut!(events);

        let device_readings = device_reading_stream(events);
        pin_mut!(device_readings);

        while let Some(reading) = device_readings.next().await {
            if let Ok(payload) = serde_json::to_string(&reading) {
                if client
                    .publish(
                        format!(
                            "device_reading/{}/{}",
                            reading.measurement.kind().to_string(),
                            reading.device_id.device_name
                        ),
                        QoS::AtLeastOnce,
                        false,
                        payload.as_bytes(),
                    )
                    .await
                    .is_ok()
                {
                    println!("published {}", payload);
                }
            }
        }
    });

    loop {
        match eventloop.poll().await {
            Ok(notification) => {} /* println!("Received = {:?}", notification),*/
            Err(e) => println!("error {:?}", e),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use uuid::Uuid;

    use crate::{measurements_from_service_data, Measurement};

    #[test]
    fn test_measurements_from_service_data() {
        let sd = HashMap::<Uuid, Vec<u8>>::from([(
            Uuid::from_u128(0x0000fcd2_0000_1000_8000_00805f9b34fb),
            vec![64, 0, 126, 1, 100, 2, 124, 7, 3, 60, 15],
        )]);

        for measurement in measurements_from_service_data(sd).iter() {
            match measurement {
                Measurement::Humidity(v) => assert_eq!(v.clone(), 39.0f64),
                Measurement::Temperature(v) => assert_eq!(v.clone(), 19.16f64),
                Measurement::Battery(v) => assert_eq!(v.clone(), 100.0f64),
                Measurement::Voltage(_) => {}
            }
        }
    }
}
