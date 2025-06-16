use objc2::MainThreadMarker;
use objc2_core_foundation::CFRunLoop;
use powerstate::register_power_state_change_callback;

fn main() {
    let mtm = MainThreadMarker::new().unwrap();
    let guard = register_power_state_change_callback(
        mtm,
        Box::new(|status| {
            println!("{:?}", status);
        }),
    )
    .unwrap();

    CFRunLoop::run();
}
