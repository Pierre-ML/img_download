// Pas de console en release sur Windows
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    media_downloader_lib::run()
}
