use std::{
    panic,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

use crate::{
    EstimatedTimeRemaining, OnPowerStateChange, PowerState, Status,
    batteries::{BatteryState, get_batteries},
};

pub struct Guard {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub fn get_current_power_state() -> Result<Status, crate::Error> {
    let batteries = get_batteries()?;

    if batteries.is_empty() {
        return Ok(Status {
            power_state: PowerState::AC,
            ..Status::default()
        });
    }

    let power_state = if batteries
        .iter()
        .any(|b| matches!(b.state, BatteryState::Charging | BatteryState::Full))
    {
        PowerState::AC
    } else if batteries
        .iter()
        .any(|b| matches!(b.state, BatteryState::Discharging | BatteryState::Empty))
    {
        PowerState::Battery
    } else {
        PowerState::Unknown
    };

    let total_energy: f32 = batteries.iter().map(|b| b.energy).sum();
    let total_energy_full: f32 = batteries.iter().map(|b| b.energy_full).sum();
    let estimated_energy_percentage = if total_energy_full > 0.0 {
        let pct = (total_energy / total_energy_full * 100.0).round() as u8;
        Some(pct.min(100))
    } else {
        None
    };

    let total_time_to_full: Option<f32> = batteries
        .iter()
        .filter_map(|b| b.time_to_full)
        .reduce(f32::max);
    let total_time_to_empty: Option<f32> = batteries
        .iter()
        .filter_map(|b| b.time_to_empty)
        .reduce(f32::min);

    let estimated_time_remaining = if let Some(secs) = total_time_to_full {
        Some(EstimatedTimeRemaining::Charging(Duration::from_secs_f32(
            secs,
        )))
    } else if let Some(secs) = total_time_to_empty {
        Some(EstimatedTimeRemaining::Discharging(
            Duration::from_secs_f32(secs),
        ))
    } else {
        None
    };

    Ok(Status {
        power_state,
        estimated_energy_percentage,
        estimated_time_remaining,
        batteries,
        power_saving_mode: false,
    })
}

const POLL_INTERVAL: Duration = Duration::from_secs(5);

pub fn register_power_state_change_callback<F>(cb: F) -> Result<Guard, crate::Error>
where
    F: Fn(Result<Status, crate::Error>) + Send + Sync + 'static,
{
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();
    let cb: OnPowerStateChange = Box::new(cb);

    let (tx, rx) = oneshot::channel::<Result<(), crate::Error>>();

    let handle = std::thread::Builder::new()
        .name("powerstate-linux-poll".to_string())
        .spawn(move || {
            let _ = tx.send(Ok(()));

            let mut prev: Option<(PowerState, Option<u8>)> = None;

            loop {
                if stop_clone.load(Ordering::Relaxed) {
                    break;
                }

                let result = get_current_power_state();
                let should_fire = match &result {
                    Ok(status) => {
                        let current = (status.power_state, status.estimated_energy_percentage);
                        let changed = prev.as_ref() != Some(&current);
                        prev = Some(current);
                        changed
                    }
                    Err(_) => true,
                };

                if should_fire {
                    let _ =
                        panic::catch_unwind(panic::AssertUnwindSafe(|| cb(result)));
                }

                // Sleep in small increments so we can respond to stop quickly
                let mut elapsed = Duration::ZERO;
                let step = Duration::from_millis(250);
                while elapsed < POLL_INTERVAL {
                    if stop_clone.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(step);
                    elapsed += step;
                }
            }
        })
        .map_err(crate::Error::CallbackThreadSpawnFailed)?;

    rx.recv().map_err(crate::Error::from)??;

    Ok(Guard {
        stop,
        handle: Some(handle),
    })
}
