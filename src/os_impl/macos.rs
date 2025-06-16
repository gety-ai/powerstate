use std::ffi::c_void;

use crate::Status;

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

fn get_power_source_state() -> Option<Status> {
    let mut status = Status {
        power_state: crate::PowerState::Unknown,
        power_saving_mode: false,
    };
    unsafe {
        let blob = IOPSCopyPowerSourcesInfo()?;
        let list = IOPSCopyPowerSourcesList(Some(&blob))?;
        let count = list.count();
        for i in 0..count {
            let ps = list.value_at_index(i);

            let desc = IOPSGetPowerSourceDescription(Some(&blob), Some(&*(ps as *const CFType)));
            if let Some(desc) = desc {
                let desc: CFRetained<CFDictionary<CFString, objc2_core_foundation::CFType>> =
                    CFRetained::cast_unchecked(desc);
                let power_source_state = CFString::from_static_str("Power Source State");
                if let Some(power_source_state) = desc.get(power_source_state.as_ref()) {
                    if let Ok(power_source_state) = power_source_state.downcast::<CFString>() {
                        if power_source_state.to_string() == "AC Power" {
                            status.power_state = crate::PowerState::AC;
                        }
                        if power_source_state.to_string() == "Battery Power" {
                            status.power_state = crate::PowerState::Battery;
                        }
                    }
                }

                let lpm_active = CFString::from_static_str("LPM Active");
                if let Some(lpm_active) = desc.get(lpm_active.as_ref()) {
                    if let Ok(lpm_active) = lpm_active.downcast::<CFNumber>() {
                        if lpm_active.as_isize() == Some(1) {
                            status.power_saving_mode = true;
                        }
                    }
                }
            }
        }
    }
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

pub fn register_power_state_change_callback(
    mtm: MainThreadMarker,
    cb: crate::OnPowerStateChange,
) -> Result<Guard, crate::Error> {
    let context = Box::new(Context { callback: cb });
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