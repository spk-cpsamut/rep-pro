#[derive(Clone)]
pub struct PullBatchSize(u32);

impl PullBatchSize {
    pub fn new(size: u32) -> Self {
        Self(size)
    }

    pub fn get(&self) -> u32 {
        self.0
    }
}
