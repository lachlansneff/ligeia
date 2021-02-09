// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![feature(allocator_api, nonnull_slice_from_raw_parts, alloc_layout_extra, slice_ptr_len, slice_ptr_get, range_bounds_assert_len, maybe_uninit_ref, trait_alias)]

use anyhow::Context;
use clap::arg_enum;
use db::WaveformLoader;
use info_bars::{InfoBar, InfoBars};
use lazy_format::lazy_format;
use std::{ffi::OsStr, fmt::Display, path::PathBuf, sync::{Arc, mpsc::sync_channel}, thread};
use structopt::StructOpt;
// use winit::{
//     event::{Event, WindowEvent},
//     event_loop::{ControlFlow, EventLoop},
//     window::WindowBuilder,
// };

mod db;
mod mmap_vec;
// mod svcb;
mod types;
// mod vcd;
mod vcd2;
mod unsized_types;
mod mmap_alloc;
mod lazy;
mod info_bars;

#[derive(Debug, StructOpt)]
#[structopt(name = "ligeia", about = "A waveform display program.")]
struct Opt {
    /// Input file
    #[structopt(parse(from_os_str))]
    file: PathBuf,

    /// File format
    #[structopt(long = "format", possible_values = &FileFormat::variants(), case_insensitive = true)]
    format: Option<FileFormat>,
}

arg_enum! {
    #[derive(Debug, PartialEq, Eq)]
    enum FileFormat {
        Vcd,
        Svcb,
    }
}

enum Source {
    Path(PathBuf),
    // Reader(Box<dyn Read + Send>),
    Stdin,
}

const LOADERS: &[(FileFormat, &'static dyn WaveformLoader)] = &[
    (FileFormat::Vcd, &crate::vcd2::VcdLoader::new()),
    // (FileFormat::Svcb, &crate::svcb::SvcbLoader::new()),
];

fn run(info_bars: Arc<InfoBars>, loader: &'static dyn WaveformLoader, source: Source) -> anyhow::Result<()> {
    let (tx, rx) = sync_channel(1);

    let loader_thread = thread::spawn(move || {
        tx.send(match source {
            Source::Path(path) => loader.load_file(&info_bars, &path),
            // Source::Reader(reader) => loader.load_stream(reader),
            Source::Stdin => {
                let stdin = std::io::stdin();
                let mut lock = stdin.lock();
                loader.load_stream(&info_bars, &mut lock)
            }
        })
        .expect("failed to send on channel");
    });

    // Now that the loader is set up, start spinning up the ui.

    let _vcdb = rx
        .recv()
        .expect("failed to receive over channel")
        .context("failed to load waveform")?;

    loader_thread.join().unwrap();

    Ok(())
}

fn memory_usage_bar_render<'a>(terminal_width: u16, bar: &'a dyn Display, progress: usize, total: usize) -> Box<dyn Display + 'a> {
    use termion::{color, style};
    use yapb::prefix::Binary;
    Box::new(lazy_format!(
        "{bold}memory usage{reset}: [{}{:width$}{reset}] {bold}{memory_used}B{style_reset}/{bold}{memory_avail}B{style_reset} ({percent:2.0}%)",
        color::Fg(color::Red),
        bar,
        reset=color::Fg(color::Reset),
        style_reset=style::Reset,
        bold=style::Bold,
        width=(terminal_width as usize) - 40,

        memory_used=Binary(progress as f64),
        memory_avail=Binary(total as f64),

        percent=(progress as f32 / total as f32) * 100.0,
    ))
}

fn main() -> anyhow::Result<()> {
    let opt: Opt = Opt::from_args();
    println!("{:?}", opt);

    let info_bars = Arc::new(InfoBars::new());

    // let parser_bar = info_bars.add(InfoBar::new(
    //     ,
    //     memory_usage_bar_render,
    // ));

    if let Ok(total_memory_limit) = effective_limits::memory_limit() {
        let info_bars = Arc::clone(&info_bars);
        let memory_usage_bar = info_bars.add(
            InfoBar::new(
                total_memory_limit as usize,
                memory_usage_bar_render,
            )
        );

        thread::spawn(move || {
            loop {
                let usage = mmap_alloc::MmappableAllocator::rough_total_usage();
                memory_usage_bar.set_progress(usage);

                info_bars.draw().unwrap();

                thread::sleep(std::time::Duration::from_millis(100));
            }
        });
    }

    // effective_limits::memory_limit().ok().map(|total_memory_limit| {
        // let pb = indicatif::ProgressBar::new(total_memory_limit);
        // pb.set_style(
        //     indicatif::ProgressStyle::default_bar()
        //         .template("memory usage: [{bar:.bold.dim}] {bytes}/{total_bytes} ({percent}%)")
        // );

        // thread::spawn(move || {
        //     loop {
        //         let usage = mmap_alloc::MmappableAllocator::rough_total_usage();
        //         pb.set_position(usage as u64);
        //         thread::sleep(std::time::Duration::from_millis(100));
        //     }
        // });
    // });

    let (loader, path_or_reader) = if opt.file == OsStr::new("-") {
        // Just read from stdin.

        let expected_format = opt
            .format
            .ok_or_else(|| anyhow::anyhow!("must provide a file format"))?;

        let loader = LOADERS
            .iter()
            .find_map(|(format, loader)| {
                if expected_format == *format {
                    Some(*loader)
                } else {
                    None
                }
            })
            .ok_or_else(|| anyhow::anyhow!("file format not supported"))?;

        (loader, Source::Stdin)
    } else {
        let extension = opt
            .file
            .extension()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!("file does not have an extension"))?;

        let loader = if let Some(expected_format) = opt.format {
            LOADERS
                .iter()
                .find_map(|(format, loader)| {
                    if expected_format == *format {
                        Some(*loader)
                    } else {
                        None
                    }
                })
                .ok_or_else(|| anyhow::anyhow!("file format not supported"))?
        } else {
            LOADERS
                .iter()
                .find_map(|(_, loader)| {
                    if loader.supports_file_extension(extension) {
                        Some(*loader)
                    } else {
                        None
                    }
                })
                .ok_or_else(|| anyhow::anyhow!("file format not supported"))?
        };

        println!(
            "loading `{}` using {}",
            opt.file.display(),
            loader.description()
        );

        (loader, Source::Path(opt.file))
    };

    run(info_bars, loader, path_or_reader)?;

    // println!("pausing for one second");
    thread::sleep(std::time::Duration::from_secs(1));

    Ok(())
}
