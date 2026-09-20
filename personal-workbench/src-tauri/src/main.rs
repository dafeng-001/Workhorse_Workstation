#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Single instance: second launch focuses the existing window and exits.
    if !personal_workbench_lib::acquire_single_instance() {
        personal_workbench_lib::focus_existing_window();
        return;
    }
    personal_workbench_lib::run()
}
