mod os_impl;

pub use os_impl::*;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[cfg(target_os = "windows")]
    #[error(transparent)]
    Windows(#[from] windows::core::Error),
}

pub struct Status {
    pub power_state: PowerState,
    /// Whether the system is in power saving mode.
    /// 
    /// In macos, this also called `Low Power Mode`
    pub power_saving_mode: bool,
}

pub type OnPowerStateChange = Box<dyn Fn(Result<Status, Error>) + Send + Sync>;

pub enum PowerState {
    Battery,
    AC,
    Unknown,
}
