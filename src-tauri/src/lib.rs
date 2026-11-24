//! Claude Fleet. On Windows it runs this PC's collector plus the dashboard (`desktop`); on a
//! phone it is only the dashboard, connecting to the PCs' collectors over the network (`mobile`).

#[cfg(desktop)]
mod desktop;
#[cfg(mobile)]
mod mobile;
#[cfg(desktop)]
mod system;
#[cfg(desktop)]
mod tray;
#[cfg(desktop)]
mod updates;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(desktop)]
    desktop::run();
    #[cfg(mobile)]
    mobile::run();
}
