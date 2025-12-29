use std::time::Duration;

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
            DispatchMessageW, GWLP_USERDATA, GetMessageW, GetWindowLongPtrW, HWND_MESSAGE,
            PostMessageW, RegisterClassW, SetWindowLongPtrW, TranslateMessage, WINDOW_EX_STYLE,
            WINDOW_STYLE, WM_CREATE, WM_DESTROY, WM_POWERBROADCAST, WNDCLASSW,
        },
    },
    core::{GUID, Owned, PCWSTR, w},
};

use crate::{
    EstimatedTimeRemaining, OnPowerStateChange, PowerState, Status, batteries::get_batteries,
};

const GUID_ACDC_POWER_SOURCE: &str = "5D3E9A59-E9D5-4B00-A6BD-FF34FF516548";

pub struct Guard {
    hwnd: HWND,
    token: Option<Owned<HPOWERNOTIFY>>,
}

unsafe impl Send for Guard {}
unsafe impl Sync for Guard {}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.token.take();
        unsafe {
            let _ = PostMessageW(Some(self.hwnd), WM_DESTROY, WPARAM(0), LPARAM(0));
        };
    }
}

const CLASS: PCWSTR = w!("PowerSink");

struct Context {
    callback: OnPowerStateChange,
}

fn create_message_only_window(cb: OnPowerStateChange) -> windows::core::Result<Guard> {
    let hinst = unsafe { GetModuleHandleW(None)? };
    let wc = WNDCLASSW {
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinst.into(),
        lpszClassName: CLASS,
        ..Default::default()
    };

    let ctx = Box::new(Context { callback: cb });
    let ctx_ptr = Box::into_raw(ctx);

    let hwnd = unsafe {
        let ret = RegisterClassW(&wc);
        if ret == 0 {
            Err(windows::core::Error::from_thread())?;
        }
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
        )?
    };

    let token = unsafe {
        let guid: GUID = GUID_ACDC_POWER_SOURCE.try_into()?;
        RegisterPowerSettingNotification(HANDLE(hwnd.0), &guid, DEVICE_NOTIFY_WINDOW_HANDLE)?
    };

    Ok(Guard {
        hwnd,
        token: Some(unsafe { Owned::new(token) }),
    })
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
        Some(power_status.BatteryLifePercent as f32 / 100.0)
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

    let batteries = get_batteries()
        .inspect_err(|e| log::warn!("Unable to access battery information: {e}"))
        .unwrap_or_default();

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
    log::debug!("wnd_proc: {msg:#x}");
    match msg {
        WM_CREATE => unsafe {
            let createstruct = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, createstruct.lpCreateParams as _);
        },
        WM_POWERBROADCAST => unsafe {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Context;
            if ptr.is_null() {
                log::error!("GetWindowLongPtrW returned null");
            } else {
                let status = get_current_power_state();
                ((*ptr).callback)(status);
            }
        },
        WM_DESTROY => unsafe {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Context;
            if !ptr.is_null() {
                let _ = Box::from_raw(ptr);
            }
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
    std::thread::spawn(move || {
        match create_message_only_window(Box::new(cb)) {
            Ok(guard) => {
                let _ = tx.send(Ok(guard));
            }
            Err(e) => {
                let _ = tx.send(Err(e.into()));
            }
        }

        unsafe {
            let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).into() {
                let _ = TranslateMessage(&msg);
                let _ = DispatchMessageW(&msg);
            }
        }
    });

    rx.recv().unwrap()
}
