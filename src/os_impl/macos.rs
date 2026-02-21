use std::{ffi::c_void, panic, ptr, time::Duration};

use crate::{EstimatedTimeRemaining, PowerState, Status, batteries::get_batteries};

use objc2::MainThreadMarker;
use objc2_core_foundation::{
    CFDictionary, CFNumber, CFRetained, CFRunLoop, CFRunLoopSource, CFString, CFType,
    kCFRunLoopDefaultMode,
};
use objc2_io_kit::{
    IOPSCopyPowerSourcesInfo, IOPSCopyPowerSourcesList, IOPSGetPowerSourceDescription,
    IOPSNotificationCreateRunLoopSource,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Failed to query power source information")]
    FailedToCopyPowerSourcesInfo,
    #[error("Failed to query power source list")]
    FailedToCopyPowerSourcesList,
    #[error("Failed to create run loop source")]
    FailedToCreateRunLoopSource,
    #[error("Missing run loop")]
    MissingRunLoop,
}

type PowerSourceDictionary = CFDictionary<CFString, objc2_core_foundation::CFType>;

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

fn status_for_non_battery_device() -> Status {
    Status {
        // For desktops like Mac mini, power state should be treated as always plugged in.
        power_state: PowerState::AC,
        ..Status::default()
    }
}

fn power_source_type(desc: &PowerSourceDictionary) -> Option<String> {
    let power_source_type = CFString::from_static_str(PowerSourceDescKey::TYPE);
    desc.get(power_source_type.as_ref())
        .and_then(|value| value.downcast::<CFString>().ok())
        .map(|value| value.to_string())
}

fn parse_power_source_status(desc: &PowerSourceDictionary) -> Status {
    let power_source_state = CFString::from_static_str(PowerSourceDescKey::POWER_SOURCE_STATE);
    let power_state: PowerState = if let Some(power_source_state) =
        desc.get(power_source_state.as_ref())
        && let Ok(power_source_state) = power_source_state.downcast::<CFString>()
    {
        let power_source_state = power_source_state.to_string();
        match PowerSourceState::try_from(power_source_state.as_str()) {
            Ok(power_source_state) => power_source_state.into(),
            Err(_) => PowerState::Unknown,
        }
    } else {
        PowerState::Unknown
    };

    let current_capacity = CFString::from_static_str(PowerSourceDescKey::CURRENT_CAPACITY);
    let estimated_energy_percentage = if let Some(current_capacity) =
        desc.get(current_capacity.as_ref())
        && let Ok(current_capacity) = current_capacity.downcast::<CFNumber>()
        && let Some(current_capacity) = current_capacity.as_i8()
    {
        #[allow(clippy::manual_range_contains)]
        if current_capacity >= 0 && current_capacity <= 100 {
            Some(current_capacity as u8)
        } else {
            None
        }
    } else {
        None
    };

    let time_to_full_charge = CFString::from_static_str(PowerSourceDescKey::TIME_TO_FULL_CHARGE);
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

    Status {
        power_state,
        estimated_energy_percentage,
        estimated_time_remaining,
        power_saving_mode,
        batteries: vec![],
    }
}

fn get_power_source_state() -> Result<Status, Error> {
    unsafe {
        let blob = IOPSCopyPowerSourcesInfo().ok_or(Error::FailedToCopyPowerSourcesInfo)?;
        let list =
            IOPSCopyPowerSourcesList(Some(&blob)).ok_or(Error::FailedToCopyPowerSourcesList)?;
        let count = list.count();
        if count == 0 {
            return Ok(status_for_non_battery_device());
        }

        let mut fallback_status = None;
        for i in 0..count {
            let ps = list.value_at_index(i as _);
            if ps.is_null() {
                continue;
            }

            let desc = IOPSGetPowerSourceDescription(Some(&blob), Some(&*(ps as *const CFType)));
            let Some(desc) = desc else {
                continue;
            };
            let desc: CFRetained<PowerSourceDictionary> = CFRetained::cast_unchecked(desc);
            let source_type = power_source_type(&desc);
            let status = parse_power_source_status(&desc);

            if source_type.as_deref() == Some("InternalBattery") {
                return Ok(status);
            }
            if fallback_status.is_none() {
                fallback_status = Some(status);
            }
        }

        if let Some(status) = fallback_status {
            Ok(status)
        } else {
            Ok(status_for_non_battery_device())
        }
    }
}

pub struct Guard {
    _mtm: MainThreadMarker,
    run_loop: CFRetained<CFRunLoop>,
    source: CFRetained<CFRunLoopSource>,
    context_ptr: *mut Context,
}

impl Drop for Guard {
    fn drop(&mut self) {
        unsafe {
            self.run_loop
                .remove_source(Some(&self.source), kCFRunLoopDefaultMode);
            if !self.context_ptr.is_null() {
                let _ = Box::from_raw(self.context_ptr);
                self.context_ptr = ptr::null_mut();
            }
        }
    }
}

struct Context {
    callback: crate::OnPowerStateChange,
}

pub fn get_current_power_state() -> Result<Status, crate::Error> {
    let mut status = get_power_source_state()?;
    if let Ok(batteries) = get_batteries() {
        status.batteries = batteries;
    }
    Ok(status)
}

unsafe extern "C-unwind" fn on_power_state_change(context: *mut c_void) {
    if context.is_null() {
        return;
    }
    let context = unsafe { &*(context as *const Context) };
    let status = get_current_power_state();
    let _ = panic::catch_unwind(panic::AssertUnwindSafe(|| (context.callback)(status)));
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
        );
        let Some(source) = source else {
            let _ = Box::from_raw(context_ptr);
            return Err(Error::FailedToCreateRunLoopSource.into());
        };

        run_loop.add_source(Some(&source), kCFRunLoopDefaultMode);
        Ok(Guard {
            _mtm: mtm,
            run_loop,
            source,
            context_ptr,
        })
    }
}
