use crate::collector::energy::Sample;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

#[derive(Clone)]
pub struct SampleBuffer {
    inner: Arc<Mutex<BufferInner>>,
    overflow_logged: Arc<AtomicBool>,
}

struct BufferInner {
    samples: VecDeque<Sample>,
    max_samples: usize,
}

impl SampleBuffer {
    pub fn new(max_samples: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(BufferInner {
                samples: VecDeque::with_capacity(max_samples),
                max_samples: max_samples.max(1),
            })),
            overflow_logged: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn push(&self, sample: Sample) {
        let mut inner = self.inner.lock().unwrap();
        if inner.samples.len() >= inner.max_samples {
            inner.samples.pop_front();
            if !self.overflow_logged.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    "Buffer full, dropping oldest samples (max: {})",
                    inner.max_samples
                );
            }
        }
        inner.samples.push_back(sample);
    }

    pub fn extend_front(&self, samples: Vec<Sample>) {
        let mut inner = self.inner.lock().unwrap();
        for sample in samples.into_iter().rev() {
            if inner.samples.len() >= inner.max_samples {
                inner.samples.pop_back();
            }
            inner.samples.push_front(sample);
        }
    }

    pub fn drain_all(&self) -> Vec<Sample> {
        let mut inner = self.inner.lock().unwrap();
        self.overflow_logged.store(false, Ordering::Relaxed);
        inner.samples.drain(..).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().samples.is_empty()
    }
}
