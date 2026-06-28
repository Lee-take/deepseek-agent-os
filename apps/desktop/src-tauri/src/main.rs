mod commands;
mod kernel;

use commands::get_foundation_state;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_foundation_state])
        .run(tauri::generate_context!())
        .expect("failed to run DeepSeek Agent OS desktop app");
}

fn main() {
    run();
}
