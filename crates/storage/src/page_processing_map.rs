use std::collections::HashMap;

use tokio::sync::broadcast;

use crate::PageId;

pub struct PageProcessingMap {
    pub fetches: HashMap<PageId, broadcast::Sender<Result<(), String>>>,
    pub flushes: HashMap<PageId, broadcast::Sender<Result<(), String>>>,
}

impl PageProcessingMap {
    pub fn new() -> Self {
        PageProcessingMap {
            fetches: HashMap::new(),
            flushes: HashMap::new(),
        }
    }
}
