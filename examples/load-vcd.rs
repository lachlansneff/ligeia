#![feature(allocator_api)]

use std::{alloc::Global, env, error, fs::File, path::Path, time::Instant};

use ligeia_core::{self, logic, ImplicitForest, WavesLoader};
use ligeia_vcd::VcdLoader;
use number_prefix::NumberPrefix;

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

    let waves = LOADER.load_file(
        Global,
        &mut |progress, _| {
            println!("{:#?}", progress);
        },
        f,
    )?;

    let elapsed = start.elapsed();

    let info = waves.changes.info();

    let size = match NumberPrefix::decimal(info.total_bytes_used as f64) {
        NumberPrefix::Standalone(bytes) => format!("{} bytes", bytes),
        NumberPrefix::Prefixed(prefix, n) => format!("{:.1} {}B", n, prefix),
    };

    println!("loaded vcd in {:?} using {}", elapsed, size);
    println!("{} storages used", info.total_storages);

    // for &storage_id in waves.storages.keys() {
    //     println!("storage id {:?} has {} changes", storage_id, waves.changes.count_of(storage_id));
    // }

    let id = *waves
        .storages
        .keys()
        .max_by_key(|&&id| waves.changes.count_of(id))
        .unwrap();

    let start = Instant::now();

    let mut forest = ImplicitForest::<logic::Four, _, _>::new_in(1, Global);

    let mut count: usize = 0;
    for offset in waves.changes.iter_storage(id) {
        count += 1;
    }

    let elapsed = start.elapsed();

    println!("count: {}, elapsed: {:?}", count, elapsed);

    Ok(())
}
