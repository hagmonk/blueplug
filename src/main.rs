use std::collections::HashMap;
use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral, ScanFilter};
use btleplug::platform::{Manager, PeripheralId};
use futures::stream::StreamExt;
use color_eyre::eyre::eyre;
use color_eyre::Result;
use ruuvi_sensor_protocol::{BatteryPotential, Humidity, ParseError, SensorValues, Temperature};

struct Payload {

}

#[tokio::main]
async fn main() -> Result<()> {
    let manager = Manager::new().await?;
    let adapters = manager.adapters().await?;
    let central = adapters.into_iter().next().ok_or(eyre!("No BT Adapter"))?;
    let mut events = central.events().await?;

    central.start_scan(ScanFilter::default()).await?;

    // Constructing a known UUID:
    // let known_id = PeripheralId::from(Uuid::from_u128(0xb5095a0b_ec20_5340_b86f_2712b41fb30e));

    // Store device names as we discover them.
    let mut device_names = HashMap::<PeripheralId, String>::new();

    while let Some(event) = events.next().await {
        match event {
            CentralEvent::DeviceDiscovered(id) => {
                let peripheral = central.peripheral(&id).await?;
                if let Some(prop) = peripheral.properties().await? {
                    if let Some(name) = prop.local_name {
                        device_names.insert(id, name);
                    }
                }
            }
            CentralEvent::ServiceDataAdvertisement { id, service_data } => {
                if let Some(decoded) = btsensor::Reading::decode(&service_data) {
                    match decoded {
                        btsensor::Reading::BtHomeV2(v2) => {
                            let s = v2.elements
                                .iter().map(|e| {
                                let mut value = String::new();

                                if let Some(val_bool) = e.value_bool() {
                                    value = val_bool.to_string()
                                }
                                if let Some(val_float) = e.value_float() {
                                    value = val_float.to_string()
                                }
                                if let Some(val_int) = e.value_int() {
                                    value = val_int.to_string()
                                }

                                format!("{}: {}{}", e.name(), value, e.unit())
                            })
                                .collect::<Vec<String>>()
                                .join("\t");

                            // Only print readings if we've discovered the device name.
                            if let Some(name) = device_names.get(&id) {
                                println!("{} {}", name, s)
                            }
                        }
                        _ => {
                            // Should probably handle bthomev1, pvvx, etc.
                            println!("unknown service data");
                        }
                    }
                }
            }

            CentralEvent::DeviceConnected(_id) => { /* println!("DeviceConnected: {:?}", id); */ }
            CentralEvent::DeviceDisconnected(_id) => { /* println!("DeviceDisconnected: {:?}", id); */ }
            CentralEvent::ManufacturerDataAdvertisement { id: id, manufacturer_data: data } => {
                if let Some(name) = device_names.get(&id) {
                    for (id, data) in data.iter() {
                        if let Ok(parsed) = SensorValues::from_manufacturer_specific_data(id.clone(), data) {
                            let mut output = name.clone();
                            if let Some(humidity) = parsed.humidity_as_ppm() {
                                output = format!("{output}\thumidity {}%", humidity as f32/10000.0);
                            }

                            if let Some(temp) = parsed.temperature_as_millicelsius() {
                                output = format!("{output}\ttemperature {}°C", temp as f32/1000.0);
                            }

                            if let Some(batt) = parsed.battery_potential_as_millivolts() {
                                output = format!("{output}\tvoltage {}V", batt as f32/1000.0);
                            }

                            println!("{output}")
                        }
                    }
                }
                /* println!("ManufacturerDataAdvertisement: {:?} {:?}", id, data); */
            }
            CentralEvent::ServicesAdvertisement { id: _, services: _ } => { /* println!("ServicesAdvertisement: {:?}, {:?}", id, services); */ }
            CentralEvent::DeviceUpdated(_) => {}
        }
    }

    Ok(())
}
