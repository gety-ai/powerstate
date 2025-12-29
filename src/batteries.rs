pub use starship_battery::State as BatteryState;
pub use starship_battery::Technology as BatteryTechnology;

/// A newtype for battery information.
/// Ref: https://docs.rs/starship-battery/latest/starship_battery/struct.Battery.html
#[derive(Debug, Default, Clone)]
pub struct BatteryInfo {
    pub state_of_charge: f32,
    pub energy: f32,
    pub energy_full: f32,
    pub energy_full_design: f32,
    pub energy_rate: f32,
    pub voltage: f32,
    pub state_of_health: f32,
    pub state: BatteryState,
    pub technology: BatteryTechnology,
    pub temperature: f32,
    pub cycle_count: u32,
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub serial_number: Option<String>,
    pub time_to_full: Option<f32>,
    pub time_to_empty: Option<f32>,
}

pub fn get_batteries() -> Result<Vec<BatteryInfo>, starship_battery::Error> {
    let manager = starship_battery::Manager::new()?;
    let mut vc: Vec<BatteryInfo> = Vec::new();

    let iter = manager.batteries()?;

    for bat in iter {
        vc.push(match bat {
            Ok(battery) => BatteryInfo {
                state_of_charge: battery.state_of_charge().value,
                energy: battery.energy().value,
                energy_full: battery.energy_full().value,
                energy_full_design: battery.energy_full_design().value,
                energy_rate: battery.energy_rate().value,
                voltage: battery.voltage().value,
                state_of_health: battery.state_of_health().value,
                state: battery.state(),
                technology: battery.technology(),
                temperature: battery.temperature().map(|t| t.value).unwrap_or_default(),
                cycle_count: battery.cycle_count().unwrap_or_default(),
                vendor: battery.vendor().map(|v| v.to_string()),
                model: battery.model().map(|m| m.to_string()),
                serial_number: battery.serial_number().map(|s| s.to_string()),
                time_to_full: battery.time_to_full().map(|t| t.value),
                time_to_empty: battery.time_to_empty().map(|t| t.value),
            },
            Err(e) => {
                log::warn!("Unable to access battery information: {e}");
                return Err(e);
            }
        })
    }

    Ok(vc)
}
