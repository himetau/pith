// no console window on release builds elsewhere; on linux this is just the entry point
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    pith_lib::run()
}
