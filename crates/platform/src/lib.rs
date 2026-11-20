use anyhow::Result;
use tracking_core::TrackerState;

pub type Complete<T> = Box<dyn FnOnce(Result<T>)>;
pub type SaveComplete = Complete<()>;

pub struct ArchiveFile {
    pub name: String,
    pub text: String,
}

pub trait Platform {
    fn save(&mut self, state: &TrackerState, complete: SaveComplete);
    fn open_archive(&self, complete: Complete<Option<ArchiveFile>>);
    fn save_archive(&self, name: &str, contents: String, complete: Complete<bool>);
    fn notify(&self, title: &str, body: &str);
    fn request_notifications(&self, complete: Complete<bool>);
    fn set_title(&self, title: &str);
    fn system_dark(&self) -> bool;
}
