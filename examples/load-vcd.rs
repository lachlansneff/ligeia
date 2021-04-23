use std::{env, error, path::Path};

use ligeia_core::{self, mmap_alloc::MmappableAllocator, waveform::WaveformLoader, Progress};
use ligeia_vcd::VcdLoader;

const LOADER: &dyn WaveformLoader<MmappableAllocator> = &VcdLoader::new();

struct NullProgress;

impl Progress for NullProgress {
    fn start(&mut self, _total_len: Option<usize>) {}
    fn finish(&mut self) {}
    fn set(&mut self, _progress: usize) {}
}

fn main() -> Result<(), Box<dyn error::Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 1 {
        eprintln!("must have 1 argument");
        return Ok(());
    }

    let path = Path::new(&args[0]);

    let _waveform = LOADER.load_file(MmappableAllocator::new(), &mut NullProgress, path)?;

    println!("loaded vcd");

    Ok(())
}
