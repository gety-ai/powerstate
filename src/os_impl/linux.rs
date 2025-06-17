use crate::Status;

pub struct Guard;

pub fn get_current_power_state() -> Result<Status, crate::Error> {
    todo!();
}

pub fn register_power_state_change_callback<F>(cb: F) -> Result<Guard, crate::Error>
where
    F: Fn(Result<Status, crate::Error>) + Send + Sync + 'static,
{
    todo!();
}
