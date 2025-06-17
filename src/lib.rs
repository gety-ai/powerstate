mod os_impl;

pub use os_impl::*;

#[derive(Debug, thiserror::Error)]
pub enum Error {
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

#[derive(Debug, Default, Clone, Copy)]
pub struct Status {
    pub power_state: PowerState,
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
        let status = get_current_power_state().unwrap();
        println!("{status:#?}");
    }
}
