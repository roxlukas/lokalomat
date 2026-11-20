// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The free app: its own configuration, no extension (ADR 0035).
    studio_lokalsi_lib::run_with(tauri::generate_context!(), None);
}
