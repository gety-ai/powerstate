use crate::Status;

pub struct Guard;

pub fn get_current_power_state() -> Result<Status, crate::Error> {
    Err(crate::Error::Linux)
}

pub fn register_power_state_change_callback<F>(cb: F) -> Result<Guard, crate::Error>
where
    F: Fn(Result<Status, crate::Error>) + Send + Sync + 'static,
{
    let _ = cb;
    Err(crate::Error::Linux)
}
