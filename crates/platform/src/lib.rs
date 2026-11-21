pub mod cloud;

use anyhow::Result;
use tracking_core::TrackerState;

use cloud::{CloudComplete, CloudRequest, CloudState};

pub type Complete<T> = Box<dyn FnOnce(Result<T>)>;
pub type SaveComplete = Complete<()>;

pub struct ArchiveFile {
    pub name: String,
    pub text: String,
}

pub trait Platform {
    fn save(&mut self, state: &TrackerState, cloud: &CloudState, complete: SaveComplete);
    fn cloud(&mut self, request: CloudRequest, complete: CloudComplete);
    fn open_archive(&self, complete: Complete<Option<ArchiveFile>>);
    fn save_archive(&self, name: &str, contents: String, complete: Complete<bool>);
    fn notify(&self, title: &str, body: &str);
    fn request_notifications(&self, complete: Complete<bool>);
    fn set_title(&self, title: &str);
}
