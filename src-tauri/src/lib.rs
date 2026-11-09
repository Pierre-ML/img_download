mod auth;
mod ffmpeg;
mod pipeline;
mod queue;
mod settings;
mod sources;
mod updater;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle();
            settings::init(handle)?;
            queue::init(handle)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            settings::get_settings,
            settings::save_settings,
            queue::queue_add,
            queue::queue_list,
            queue::queue_cancel,
            queue::queue_clear_finished,
            queue::recent_media,
            ffmpeg::ffmpeg_status,
            auth::account_status,
            auth::account_login,
            auth::account_logout,
            updater::app_check_update,
            updater::app_install_update,
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de l'application Tauri");
}
