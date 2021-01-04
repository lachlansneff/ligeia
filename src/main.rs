use std::{fmt::Display, fs::File, path::{Path, PathBuf}, str::FromStr, sync::mpsc::sync_channel, thread};
use anyhow::Context;
use clap::arg_enum;
use db::WaveformLoader;
use structopt::StructOpt;
// use winit::{
//     event::{Event, WindowEvent},
//     event_loop::{ControlFlow, EventLoop},
//     window::WindowBuilder,
// };

mod db;
mod mmap_vec;
mod svcb;
mod types;
mod vcd;

#[derive(Debug, StructOpt)]
#[structopt(name = "ligeia", about="A waveform display program.")]
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

const LOADERS: &[(FileFormat, &'static dyn WaveformLoader)] = &[
    (FileFormat::Vcd, &crate::vcd::VcdLoader::new()),
    (FileFormat::Svcb, &crate::svcb::SvcbLoader::new()),
];

fn run(loader: &'static dyn WaveformLoader, path: PathBuf) -> anyhow::Result<()> {
    let (tx, rx) = sync_channel(1);

    let loader_thread = thread::spawn(move || {
        tx.send(loader.load_file(&path)).expect("failed to send on channel");
    });

    // Now that the loader is set up, start spinning up the ui.

    let _vcdb = rx.recv().expect("failed to receive over channel")
        .context("failed to load waveform")?;

    loader_thread.join().unwrap();
    
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let opt: Opt = Opt::from_args();
    println!("{:?}", opt);

    let extension = opt.file.extension()
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

    println!("loading `{}` using {}", opt.file.display(), loader.description());
    run(loader, opt.file)?;

    Ok(())
}
