use crate::Status;
use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString, CFType};
use objc2_io_kit::{
    self, IOPSCopyPowerSourcesInfo, IOPSCopyPowerSourcesList, IOPSGetPowerSourceDescription,
};

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

pub fn get_current_power_state() -> Result<Status, crate::Error> {
    Ok(get_power_source_state().unwrap_or_default())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_current_power_state() {
        let status = get_current_power_state().unwrap();
        println!("{:?}", status);
    }
}