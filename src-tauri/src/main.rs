// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(feature = "app")]
fn main() {
    harbour_lib::run()
}

// Without the app feature the crate is the headless core, which has no GUI to
// start. The binary still has to have a `main`, so this stands in.
#[cfg(not(feature = "app"))]
fn main() {
    eprintln!("harbour was built without the `app` feature; there is no UI to run.");
}
