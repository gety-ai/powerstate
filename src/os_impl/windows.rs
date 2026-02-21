use std::{panic, time::Duration};

use windows::{
    Win32::{
        Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM},
        System::{
            LibraryLoader::GetModuleHandleW,
            Power::{
                GetSystemPowerStatus, HPOWERNOTIFY, RegisterPowerSettingNotification,
                SYSTEM_POWER_STATUS,
            },
        },
        UI::WindowsAndMessaging::{
            CREATESTRUCTW, CreateWindowExW, DEVICE_NOTIFY_WINDOW_HANDLE, DefWindowProcW,
            DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetMessageW, GetWindowLongPtrW,
            HWND_MESSAGE, PostMessageW, PostQuitMessage, RegisterClassW, SetWindowLongPtrW,
            TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CREATE, WM_DESTROY,
            WM_POWERBROADCAST, WNDCLASSW,
        },
    },
    core::{GUID, HRESULT, Owned, PCWSTR, w},
};

use crate::{
    EstimatedTimeRemaining, OnPowerStateChange, PowerState, Status, batteries::get_batteries,
};

// Ref: https://learn.microsoft.com/en-us/windows/win32/power/power-setting-guids
const GUID_ACDC_POWER_SOURCE: &str = "5D3E9A59-E9D5-4B00-A6BD-FF34FF516548";
const GUID_BATTERY_PERCENTAGE_REMAINING: &str = "A7AD8041-B45A-4CAE-87A3-EECBB468A9E1";
const ERROR_CLASS_ALREADY_EXISTS: u32 = 1410;

pub struct Guard {
    hwnd: HWND,
    tokens: Vec<Owned<HPOWERNOTIFY>>,
}

unsafe impl Send for Guard {}
unsafe impl Sync for Guard {}

impl Drop for Guard {
    fn drop(&mut self) {
        self.tokens.clear();
        unsafe {
            let _ = PostMessageW(Some(self.hwnd), WM_DESTROY, WPARAM(0), LPARAM(0));
        };
    }
}

const CLASS: PCWSTR = w!("PowerSink");

struct Context {
    callback: OnPowerStateChange,
}

fn register_window_class(hinst: windows::Win32::Foundation::HMODULE) -> windows::core::Result<()> {
    let wc = WNDCLASSW {
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinst.into(),
        lpszClassName: CLASS,
        ..Default::default()
    };

    let ret = unsafe { RegisterClassW(&wc) };
    if ret == 0 {
        let err = windows::core::Error::from_thread();
        if err.code() != HRESULT::from_win32(ERROR_CLASS_ALREADY_EXISTS) {
            return Err(err);
        }
    }

    Ok(())
}

fn create_message_only_window(cb: OnPowerStateChange) -> windows::core::Result<Guard> {
    let hinst = unsafe { GetModuleHandleW(None)? };
    register_window_class(hinst)?;

    let ctx = Box::new(Context { callback: cb });
    let ctx_ptr = Box::into_raw(ctx);

    let hwnd = match unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS,
            None,
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(hinst.into()),
            Some(ctx_ptr as *const _),
        )
    } {
        Ok(hwnd) => hwnd,
        Err(e) => {
            unsafe {
                let _ = Box::from_raw(ctx_ptr);
            }
            return Err(e);
        }
    };

    let mut tokens = Vec::new();

    // Register AC/DC power source change notification
    let guid: GUID = match GUID_ACDC_POWER_SOURCE.try_into() {
        Ok(guid) => guid,
        Err(e) => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            return Err(e);
        }
    };
    let token = match unsafe {
        RegisterPowerSettingNotification(HANDLE(hwnd.0), &guid, DEVICE_NOTIFY_WINDOW_HANDLE)
    } {
        Ok(token) => unsafe { Owned::new(token) },
        Err(e) => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            return Err(e);
        }
    };
    tokens.push(token);
    // Register battery percentage change notification
    let guid: GUID = match GUID_BATTERY_PERCENTAGE_REMAINING.try_into() {
        Ok(guid) => guid,
        Err(e) => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            return Err(e);
        }
    };
    let token = match unsafe {
        RegisterPowerSettingNotification(HANDLE(hwnd.0), &guid, DEVICE_NOTIFY_WINDOW_HANDLE)
    } {
        Ok(token) => unsafe { Owned::new(token) },
        Err(e) => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            return Err(e);
        }
    };
    tokens.push(token);

    Ok(Guard { hwnd, tokens })
}

/// Get the current power state of the system.
pub fn get_current_power_state() -> Result<Status, crate::Error> {
    let mut power_status = SYSTEM_POWER_STATUS::default();
    unsafe {
        GetSystemPowerStatus(&mut power_status)?;
    }

    let estimated_energy_percentage = if power_status.BatteryLifePercent == u8::MAX {
        None
    } else {
        Some(power_status.BatteryLifePercent)
    };
    let estimated_time_remaining = if power_status.BatteryFullLifeTime != u32::MAX {
        Some(EstimatedTimeRemaining::Charging(Duration::from_secs(
            power_status.BatteryFullLifeTime as u64,
        )))
    } else if power_status.BatteryLifeTime != u32::MAX {
        Some(EstimatedTimeRemaining::Discharging(Duration::from_secs(
            power_status.BatteryLifeTime as u64,
        )))
    } else {
        None
    };

    let batteries = get_batteries().unwrap_or_default();

    Ok(Status {
        estimated_energy_percentage,
        estimated_time_remaining,
        batteries,
        power_state: match power_status.ACLineStatus {
            0 => PowerState::Battery,
            1 => PowerState::AC,
            _ => PowerState::Unknown,
        },
        power_saving_mode: power_status.SystemStatusFlag == 1,
    })
}

extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => unsafe {
            let createstruct = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, createstruct.lpCreateParams as _);
        },
        WM_POWERBROADCAST => unsafe {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Context;
            if ptr.is_null() {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            let status = get_current_power_state();
            let _ = panic::catch_unwind(panic::AssertUnwindSafe(|| ((*ptr).callback)(status)));
        },
        WM_DESTROY => unsafe {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Context;
            if !ptr.is_null() {
                let _ = Box::from_raw(ptr);
            }
            PostQuitMessage(0);
        },
        _ => (),
    }

    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

pub fn register_power_state_change_callback<F>(cb: F) -> Result<Guard, crate::Error>
where
    F: Fn(Result<Status, crate::Error>) + Send + Sync + 'static,
{
    let (tx, rx) = oneshot::channel();
    std::thread::Builder::new()
        .name("powerstate-windows-loop".to_string())
        .spawn(move || {
            match create_message_only_window(Box::new(cb)) {
                Ok(guard) => {
                    let _ = tx.send(Ok(guard));
                }
                Err(e) => {
                    let _ = tx.send(Err(e.into()));
                    return;
                }
            }

            unsafe {
                let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
                loop {
                    match GetMessageW(&mut msg, None, 0, 0).0 {
                        -1 => break,
                        0 => break,
                        _ => {
                            let _ = TranslateMessage(&msg);
                            let _ = DispatchMessageW(&msg);
                        }
                    }
                }
            }
        })
        .map_err(crate::Error::CallbackThreadSpawnFailed)?;

    rx.recv().map_err(crate::Error::from)?
}
