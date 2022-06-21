use std::{env, error, fs::File, os::unix::prelude::MetadataExt, path::Path, time::Instant};

use ligeia_vcd;
use number_prefix::NumberPrefix;

fn main() -> Result<(), Box<dyn error::Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 1 {
        eprintln!("must have 1 argument");
        return Ok(());
    }

    let path = Path::new(&args[0]);
    let f = File::open(path)?;
    let file_size = f.metadata()?.size();

    let start = Instant::now();

    let mut processed = ligeia_vcd::load_vcd(f)?;

    let elapsed = start.elapsed();

    let size = match NumberPrefix::decimal(file_size as f64) {
        NumberPrefix::Standalone(bytes) => format!("{} bytes", bytes),
        NumberPrefix::Prefixed(prefix, n) => format!("{:.1} {}B", n, prefix),
    };

    println!("loaded {} vcd in {:?}", size, elapsed);

    let storage_ids = processed.storage_ids();

    let start = Instant::now();

    for id in storage_ids {
        processed.load_storage(id, |_timestamp, _value| {})?;
    }

    let elapsed = start.elapsed();
    println!("loaded storages in {:?}", elapsed);

    Ok(())
}
