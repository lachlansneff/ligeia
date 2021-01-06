use std::{
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

#[derive(Default)]
struct Data {
    acc: AtomicUsize,
    end: AtomicUsize,
    stage: AtomicUsize,
}

#[derive(Clone)]
pub struct Progress {
    data: Arc<Data>,
}

impl Progress {
    pub fn new(end: Option<NonZeroUsize>) -> Self {
        Self {
            data: Arc::new(Data {
                end: AtomicUsize::new(end.map(|n| n.get()).unwrap_or(0)),
                ..Default::default()
            }),
        }
    }

    pub fn tick(&self) {
        self.data.acc.fetch_add(1, Ordering::Relaxed);
    }

    pub fn percentage(&self) -> Option<f32> {
        let acc = self.data.acc.load(Ordering::Relaxed);
        let end = self.data.end.load(Ordering::Relaxed);

        if end == 0 {
            None
        } else {
            Some(acc as f32 / end as f32)
        }
    }

    pub fn next_stage(&self, new_end: Option<NonZeroUsize>) {
        self.data.acc.store(0, Ordering::Relaxed);
        self.data
            .end
            .store(new_end.map(|n| n.get()).unwrap_or(0), Ordering::Relaxed);
        self.data.stage.fetch_add(1, Ordering::Relaxed);
    }
}
