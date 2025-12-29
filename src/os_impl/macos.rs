use std::{ffi::c_void, time::Duration};

use crate::{EstimatedTimeRemaining, PowerState, Status, batteries::get_batteries};

use objc2::MainThreadMarker;
use objc2_core_foundation::{
    CFDictionary, CFNumber, CFRetained, CFRunLoop, CFRunLoopSource, CFString, CFType,
    kCFRunLoopDefaultMode,
};
use objc2_io_kit::{
    self, IOPSCopyPowerSourcesInfo, IOPSCopyPowerSourcesList, IOPSGetPowerSourceDescription,
    IOPSNotificationCreateRunLoopSource,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Failed to create run loop source")]
    FailedToCreateRunLoopSource,
    #[error("Missing run loop")]
    MissingRunLoop,
}

struct PowerSourceDescKey;
#[allow(dead_code)]
impl PowerSourceDescKey {
    /// Battery Provides Time Remaining
    pub const BATTERY_PROVIDES_TIME_REMAINING: &'static str = "Battery Provides Time Remaining";
    /// Battery Health
    pub const BETTERY_HEALTH: &'static str = "BatteryHealth";
    /// Battery Health Condition
    pub const BETTERY_HEALTH_CONDITION: &'static str = "BatteryHealthCondition";
    /// Current
    pub const CURRENT: &'static str = "Current";
    /// Current Capacity
    pub const CURRENT_CAPACITY: &'static str = "Current Capacity";
    /// Design Cycle Count
    pub const DESIGN_CYCLE_COUNT: &'static str = "DesignCycleCount";
    /// Hardware serial number
    pub const HARDWARE_SERIAL_NUMBER: &'static str = "Hardware Serial Number";
    /// Is Charing
    pub const IS_CHARGING: &'static str = "Is Charging";
    /// Is Finishing Charge
    pub const IS_FINISHING_CHARGE: &'static str = "Is Finishing Charge";
    /// Is Present
    pub const IS_PRESENT: &'static str = "Is Present";
    /// Low Power Mode Active
    pub const LPM_ACTIVE: &'static str = "LPM Active";
    /// Max Capacity
    pub const MAX_CAPACITY: &'static str = "Max Capacity";
    /// Power Source Name
    pub const NAME: &'static str = "Name";
    /// Power Source ID
    pub const POWER_SOURCE_ID: &'static str = "Power Source ID";
    /// Power Source State
    pub const POWER_SOURCE_STATE: &'static str = "Power Source State";
    /// Time to Full Charge, in minutes
    pub const TIME_TO_FULL_CHARGE: &'static str = "Time to Full Charge";
    /// Time to Empty Now, in minutes
    pub const TIME_TO_EMPTY: &'static str = "Time to Empty";
    /// Transport Type
    pub const TRANSPORT_TYPE: &'static str = "Transport Type";
    /// Type
    pub const TYPE: &'static str = "Type";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::EnumString)]
pub enum PowerSourceState {
    #[strum(serialize = "AC Power")]
    AcPower,
    #[strum(serialize = "Battery Power")]
    BatteryPower,
}

impl From<PowerSourceState> for PowerState {
    fn from(value: PowerSourceState) -> Self {
        match value {
            PowerSourceState::AcPower => PowerState::AC,
            PowerSourceState::BatteryPower => PowerState::Battery,
        }
    }
}

fn get_power_source_state() -> Option<Status> {
    let mut status = unsafe {
        let blob = IOPSCopyPowerSourcesInfo()?;
        let list = IOPSCopyPowerSourcesList(Some(&blob))?;
        let count = list.count();
        let mut i = 0;
        // TODO: support non-battery devices, such as mac mini?
        loop {
            let ps = list.value_at_index(i);

            let desc = IOPSGetPowerSourceDescription(Some(&blob), Some(&*(ps as *const CFType)));
            if let Some(desc) = desc {
                let desc: CFRetained<CFDictionary<CFString, objc2_core_foundation::CFType>> =
                    CFRetained::cast_unchecked(desc);
                // eprintln!("{desc:#?}");

                // Pick first internal battery
                let power_source_type = CFString::from_static_str(PowerSourceDescKey::TYPE);
                if let Some(power_source_type) = desc.get(power_source_type.as_ref())
                    && let Ok(power_source_type) = power_source_type.downcast::<CFString>()
                {
                    let power_source_type = power_source_type.to_string();
                    if power_source_type != "InternalBattery" {
                        i += 1;
                        if i >= count {
                            log::warn!("No internal battery found");
                            break None;
                        }
                        continue;
                    }
                }

                let power_source_state =
                    CFString::from_static_str(PowerSourceDescKey::POWER_SOURCE_STATE);
                let power_state: PowerState = if let Some(power_source_state) =
                    desc.get(power_source_state.as_ref())
                    && let Ok(power_source_state) = power_source_state.downcast::<CFString>()
                {
                    let power_source_state = power_source_state.to_string();
                    match PowerSourceState::try_from(power_source_state.as_str()) {
                        Ok(power_source_state) => power_source_state.into(),
                        Err(_) => {
                            log::warn!("Unknown power source state: {power_source_state}");
                            PowerState::Unknown
                        }
                    }
                } else {
                    PowerState::Unknown
                };

                let current_capacity =
                    CFString::from_static_str(PowerSourceDescKey::CURRENT_CAPACITY);
                let estimated_energy_percentage = if let Some(current_capacity) =
                    desc.get(current_capacity.as_ref())
                    && let Ok(current_capacity) = current_capacity.downcast::<CFNumber>()
                    && let Some(current_capacity) = current_capacity.as_i8()
                {
                    #[allow(clippy::manual_range_contains)]
                    if current_capacity >= 0 && current_capacity <= 100 {
                        Some(current_capacity as u8)
                    } else {
                        log::warn!("Current capacity is out of range: {current_capacity}");
                        None
                    }
                } else {
                    None
                };

                let time_to_full_charge =
                    CFString::from_static_str(PowerSourceDescKey::TIME_TO_FULL_CHARGE);
                let time_to_empty = CFString::from_static_str(PowerSourceDescKey::TIME_TO_EMPTY);
                let mut estimated_time_remaining = None;
                if let Some(time_to_full_charge) = desc.get(time_to_full_charge.as_ref())
                    && let Ok(time_to_full_charge) = time_to_full_charge.downcast::<CFNumber>()
                {
                    if let Some(time_to_full_charge) = time_to_full_charge.as_i32() {
                        if time_to_full_charge > 0 {
                            estimated_time_remaining = Some(EstimatedTimeRemaining::Charging(
                                Duration::from_secs(time_to_full_charge as u64 * 60),
                            ));
                        }
                    } else {
                        log::warn!("Time to full charge is not a i32 : {time_to_full_charge:?}");
                    }
                }
                if let Some(time_to_empty) = desc.get(time_to_empty.as_ref())
                    && let Ok(time_to_empty) = time_to_empty.downcast::<CFNumber>()
                {
                    if let Some(time_to_empty) = time_to_empty.as_i32() {
                        if time_to_empty > 0 {
                            estimated_time_remaining = Some(EstimatedTimeRemaining::Discharging(
                                Duration::from_secs(time_to_empty as u64 * 60),
                            ));
                        }
                    } else {
                        log::warn!("Time to empty is not a i32 : {time_to_empty:?}");
                    }
                }

                let lpm_active = CFString::from_static_str(PowerSourceDescKey::LPM_ACTIVE);
                let power_saving_mode = if let Some(lpm_active) = desc.get(lpm_active.as_ref())
                    && let Ok(lpm_active) = lpm_active.downcast::<CFNumber>()
                    && lpm_active.as_isize() == Some(1)
                {
                    true
                } else {
                    false
                };

                break Some(Status {
                    power_state,
                    estimated_energy_percentage,
                    estimated_time_remaining,
                    power_saving_mode,
                    batteries: vec![],
                });
            }
        }
        .unwrap_or_default()
    };

    let batteries = get_batteries()
        .inspect_err(|e| log::warn!("Unable to access battery information: {e}"))
        .unwrap_or_default();
    status.batteries = batteries;
    Some(status)
}

pub struct Guard {
    _mtm: MainThreadMarker,
    source: CFRetained<CFRunLoopSource>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        unsafe {
            if let Some(run_loop) = CFRunLoop::current() {
                run_loop.remove_source(Some(&self.source), kCFRunLoopDefaultMode);
            }
        }
    }
}

struct Context {
    callback: crate::OnPowerStateChange,
}

pub fn get_current_power_state() -> Result<Status, crate::Error> {
    Ok(get_power_source_state().unwrap_or_default())
}

unsafe extern "C-unwind" fn on_power_state_change(context: *mut c_void) {
    let context = context as *mut Context;
    unsafe { ((*context).callback)(get_current_power_state()) };
}

// TODO: implement LPM callback support?
pub fn register_power_state_change_callback<F>(
    mtm: MainThreadMarker,
    cb: F,
) -> Result<Guard, crate::Error>
where
    F: Fn(Result<Status, crate::Error>) + Send + Sync + 'static,
{
    let context = Box::new(Context {
        callback: Box::new(cb),
    });
    unsafe {
        let run_loop = CFRunLoop::current().ok_or(Error::MissingRunLoop)?;
        let context_ptr = Box::into_raw(context);
        let source = IOPSNotificationCreateRunLoopSource(
            Some(on_power_state_change),
            context_ptr as *mut c_void,
        )
        .ok_or(Error::FailedToCreateRunLoopSource)?;

        run_loop.add_source(Some(&source), kCFRunLoopDefaultMode);
        Ok(Guard { _mtm: mtm, source })
    }
}
