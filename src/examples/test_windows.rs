#[cfg(target_os = "windows")]
use powerstate::register_power_state_change_callback;

#[cfg(target_os = "windows")]
fn main() {
    let guard = register_power_state_change_callback(|status| {
        println!("{status:#?}");
    })
    .unwrap();

    std::thread::sleep(std::time::Duration::from_secs(10));
}
