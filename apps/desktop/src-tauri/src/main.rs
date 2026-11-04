// Hide the console window on Windows release builds (harmless on Linux).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    sky_linux_lib::run();
}
