fn main() {
    // Only the desktop app needs the Tauri context and codegen; a headless
    // build of the core (the `app` feature off) skips it, so it does not need a
    // built frontend or a Tauri config to compile.
    if std::env::var_os("CARGO_FEATURE_APP").is_some() {
        tauri_build::build();
    }
}
