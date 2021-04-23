// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{
    fmt::{self, Display},
    io::{self, stderr, Write},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use yapb::{Bar, Progress};
// use yapb::Bar;

// pub trait BarFormatter = for<'a> Fn(u16, &'a (dyn Display + 'a), usize, usize) -> Box<dyn Display + 'a> + Send + Sync;

pub trait BarFormatter: Send + Sync {
    fn format<'a>(
        &self,
        terminal_width: u16,
        bar: &'a dyn Display,
        progress: usize,
        total: usize,
    ) -> Box<dyn Display + 'a>;
}

impl<F> BarFormatter for F
where
    F: for<'a> Fn(u16, &'a (dyn Display + 'a), usize, usize) -> Box<(dyn Display + 'a)>
        + Send
        + Sync,
{
    fn format<'a>(
        &self,
        terminal_width: u16,
        bar: &'a dyn Display,
        progress: usize,
        total: usize,
    ) -> Box<dyn Display + 'a> {
        (self)(terminal_width, bar, progress, total)
    }
}

// impl<F> BarFormatter for Box<F> where  F: BarFormatter {
//     fn format<'a>(&self, terminal_width: u16, bar: &'a dyn Display, progress: usize, total: usize) -> Box<dyn Display + 'a> {
//         (*self).format(terminal_width, bar, progress, total)
//     }
// }

pub struct InfoBar<F> {
    total: usize,
    progress: AtomicUsize,
    formatter: F,
}

impl<F> InfoBar<F> {
    pub fn new(total: usize, formatter: F) -> Self {
        Self {
            total,
            progress: AtomicUsize::new(0),
            formatter,
        }
    }

    pub fn set_progress(&self, progress: usize) {
        self.progress.store(progress, Ordering::Relaxed);
    }
}

impl<F: BarFormatter> InfoBar<F> {
    // fn draw(&self)
}

pub struct InfoBars {
    bars: Mutex<Vec<Arc<InfoBar<Box<dyn BarFormatter>>>>>,
    has_started: AtomicBool,
}

impl InfoBars {
    pub fn new() -> Self {
        Self {
            bars: Mutex::new(vec![]),
            has_started: AtomicBool::new(false),
        }
    }

    pub fn add<F: BarFormatter + 'static>(
        &self,
        bar: InfoBar<F>,
    ) -> Arc<InfoBar<Box<dyn BarFormatter>>> {
        let arc = Arc::new(InfoBar {
            total: bar.total,
            progress: bar.progress,
            formatter: Box::new(bar.formatter) as _,
        });

        self.bars.lock().unwrap().push(Arc::clone(&arc));
        arc
    }

    pub fn replace<F: BarFormatter + 'static>(
        &self,
        old_bar: Arc<InfoBar<Box<dyn BarFormatter>>>,
        new_bar: InfoBar<F>,
    ) -> Result<(), ()> {
        let mut bars = self.bars.lock().unwrap();
        let index = bars
            .iter()
            .position(|a| Arc::ptr_eq(a, &old_bar))
            .ok_or(())?;
        bars[index] = Arc::new(InfoBar {
            total: new_bar.total,
            progress: new_bar.progress,
            formatter: Box::new(new_bar.formatter) as _,
        });
        Ok(())
    }

    pub fn draw(&self) -> io::Result<()> {
        let stream = stderr();
        let bars = self.bars.lock().unwrap();
        if !termion::is_tty(&stream) || bars.is_empty() {
            return Ok(());
        }

        let mut output = stream.lock();

        if self.has_started.swap(true, Ordering::Relaxed) {
            write!(output, "{}", termion::cursor::Up(bars.len() as u16))?;
        }

        let (terminal_width, _) = termion::terminal_size()?;

        for bar in &*bars {
            let progress = bar.progress.load(Ordering::Relaxed);
            let mut display_bar = yapb::Bar::new();
            display_bar.set(progress as f32 / bar.total as f32);

            let renderer = bar
                .formatter
                .format(terminal_width, &display_bar, progress, bar.total);

            writeln!(output, "\r{}{}", termion::clear::AfterCursor, renderer)?;
        }

        output.flush()?;

        Ok(())
    }
}
