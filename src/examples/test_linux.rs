#[cfg(target_os = "linux")]
use powerstate::{get_current_power_state, register_power_state_change_callback};

#[cfg(target_os = "linux")]
fn main() {
    simple_logging::log_to_stderr(log::LevelFilter::Trace);

    let status = get_current_power_state().unwrap();
    println!("Initial power state: {status:#?}");

    let _guard = register_power_state_change_callback(|status| {
        println!("{status:#?}");
    })
    .unwrap();

    std::thread::sleep(std::time::Duration::from_secs(60));
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("This is a linux example, please run it on linux");
}
