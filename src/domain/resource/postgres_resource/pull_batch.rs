#[derive(Clone)]
pub enum PullBatchStrategy {
    Cursor {
        field: String,
        pointer: Option<String>,
    },
    LimitOffSet {
        field: String,
        offset: Option<SqlOffset>,
    },
}

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

#[derive(Clone)]
pub struct SqlOffset(u32);

impl SqlOffset {
    pub fn new(size: u32) -> Self {
        Self(size)
    }

    pub fn get(&self) -> u32 {
        self.0
    }
}
