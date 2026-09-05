// No console window on Windows for a release build.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    ducat_desk_lib::run()
}
