#![feature(allocator_api)]

use std::{alloc::Global, env, error, fs::File, path::Path, time::Instant};

use ligeia_core::{self, WavesLoader};
use ligeia_vcd::VcdLoader;

const LOADER: &dyn WavesLoader<Global> = &VcdLoader;

fn main() -> Result<(), Box<dyn error::Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 1 {
        eprintln!("must have 1 argument");
        return Ok(());
    }

    let path = Path::new(&args[0]);
    let f = File::open(path)?;

    let start = Instant::now();

    let _waves = LOADER.load_file(Global, &mut |progress, _| {
        println!("{:#?}", progress);
    }, f)?;

    let elapsed = start.elapsed();

    println!("loaded vcd in {:?}", elapsed);

    Ok(())
}
