use std::time::Duration;
mod batteries;
mod os_impl;

pub use batteries::{BatteryInfo, BatteryState, BatteryTechnology};

pub use os_impl::*;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to spawn callback thread: {0}")]
    CallbackThreadSpawnFailed(#[source] std::io::Error),
    #[error("failed to receive callback registration result: {0}")]
    CallbackRegistrationChannelClosed(#[from] oneshot::RecvError),
    #[cfg(target_os = "windows")]
    #[error(transparent)]
    Windows(#[from] windows::core::Error),
    #[cfg(target_os = "macos")]
    #[error(transparent)]
    Macos(#[from] MacosError),
    #[cfg(target_os = "linux")]
    #[error("Linux is not supported")]
    Linux,
}

#[derive(Debug, Clone)]
pub enum EstimatedTimeRemaining {
    Charging(Duration),
    Discharging(Duration),
}

#[derive(Debug, Default, Clone)]
pub struct Status {
    pub power_state: PowerState,
    /// Estimated energy percentage, in range [0, 100].
    pub estimated_energy_percentage: Option<u8>,
    pub estimated_time_remaining: Option<EstimatedTimeRemaining>,
    pub batteries: Vec<BatteryInfo>,
    /// Whether the system is in power saving mode.
    ///
    /// In macos, this also called `Low Power Mode`
    pub power_saving_mode: bool,
}

type OnPowerStateChange = Box<dyn Fn(Result<Status, Error>) + Send + Sync>;

#[derive(Debug, Default, Clone, Copy)]
pub enum PowerState {
    Battery,
    AC,
    #[default]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_current_power_state() {
        let status = get_current_power_state();
        #[cfg(target_os = "linux")]
        assert!(matches!(status, Err(Error::Linux)));

        #[cfg(not(target_os = "linux"))]
        println!("{:#?}", status.unwrap());
    }
}
