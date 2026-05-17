//! # App Loader
//!
//! Manages loading, reloading, and calling into the app dylib.

use libloading::{Library, Symbol};
use std::path::PathBuf;

type RenderFn =
    unsafe extern "C" fn(*mut ratatui::Frame, *const super::AppState);
type KeyFn = unsafe extern "C" fn(*const crossterm::event::KeyEvent, *mut super::AppState);

pub struct AppLoader {
    library: Option<Library>,
    hot_path: PathBuf,
    last_load_time: std::time::SystemTime,
}

impl AppLoader {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let mut loader = Self {
            library: None,
            hot_path: PathBuf::from("target/hot/libapp.so"),
            last_load_time: std::time::SystemTime::UNIX_EPOCH,
        };
        loader.reload()?;
        Ok(loader)
    }

    pub fn reload(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.hot_path.exists() {
            return Ok(());
        }

        let modified = std::fs::metadata(&self.hot_path)?.modified()?;
        if modified <= self.last_load_time {
            return Ok(());
        }

        // Unload old library
        self.library = None;

        // Load new library
        unsafe {
            self.library = Some(Library::new(&self.hot_path)?);
        }
        self.last_load_time = modified;
        println!("Hot reloaded: {}", self.hot_path.display());
        Ok(())
    }

    pub fn get_render_fn(&self) -> Option<RenderFn> {
        unsafe {
            self.library
                .as_ref()?
                .get(b"render_app\0")
                .ok()
                .copied()
        }
    }

    pub fn get_key_fn(&self) -> Option<KeyFn> {
        unsafe {
            self.library
                .as_ref()?
                .get(b"handle_key\0")
                .ok()
                .copied()
        }
    }

    pub fn handle_key(&self, _key: &crossterm::event::KeyEvent) -> bool {
        // Check if file changed (simplified)
        if self.hot_path.exists() {
            if let Ok(meta) = std::fs::metadata(&self.hot_path) {
                if let Ok(modified) = meta.modified() {
                    return modified > self.last_load_time;
                }
            }
        }
        false
    }
}
