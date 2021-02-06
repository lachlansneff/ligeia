// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![feature(allocator_api, nonnull_slice_from_raw_parts, alloc_layout_extra, slice_ptr_len, slice_ptr_get, range_bounds_assert_len )]

use anyhow::Context;
use clap::arg_enum;
use db::WaveformLoader;
use std::{ffi::OsStr, path::PathBuf, sync::mpsc::sync_channel, thread};
use structopt::StructOpt;
// use winit::{
//     event::{Event, WindowEvent},
//     event_loop::{ControlFlow, EventLoop},
//     window::WindowBuilder,
// };

mod db;
mod mmap_vec;
mod progress;
mod svcb;
mod types;
mod vcd;
mod forest;
mod unsized_types;
mod mmap_alloc;

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
    (FileFormat::Vcd, &crate::vcd::VcdLoader::new()),
    (FileFormat::Svcb, &crate::svcb::SvcbLoader::new()),
];

fn run(loader: &'static dyn WaveformLoader, source: Source) -> anyhow::Result<()> {
    let (tx, rx) = sync_channel(1);

    let loader_thread = thread::spawn(move || {
        tx.send(match source {
            Source::Path(path) => loader.load_file(&path),
            // Source::Reader(reader) => loader.load_stream(reader),
            Source::Stdin => {
                let stdin = std::io::stdin();
                let mut lock = stdin.lock();
                loader.load_stream(&mut lock)
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

fn main() -> anyhow::Result<()> {
    let opt: Opt = Opt::from_args();
    println!("{:?}", opt);

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

    run(loader, path_or_reader)?;

    Ok(())
}
