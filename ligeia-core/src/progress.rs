/// This trait is used to stand in for something that
/// can progress over time.
pub trait Progress {
    /// Enable progress tracking.
    fn start(&mut self, total_len: Option<usize>);
    /// State that loading has finished.
    fn finish(&mut self);
    /// Set progress to `progress`, which must be within [0, `total_len`].
    fn set(&mut self, progress: usize);
}
