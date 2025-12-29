#[cfg(target_os = "macos")]
use objc2::MainThreadMarker;
#[cfg(target_os = "macos")]
use objc2_core_foundation::CFRunLoop;
#[cfg(target_os = "macos")]
use powerstate::register_power_state_change_callback;

#[cfg(target_os = "macos")]
fn main() {
    simple_logging::log_to_stderr(log::LevelFilter::Trace);
    let mtm = MainThreadMarker::new().unwrap();
    let guard = register_power_state_change_callback(mtm, |status| {
        println!("{:?}", status);
    })
    .unwrap();

    CFRunLoop::run();
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This is a macos example, please run it on macos");
}
