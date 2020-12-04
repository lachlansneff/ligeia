
use std::{convert::TryFrom, fmt::Debug, fmt::{Display, Formatter}, fs::File, io, num::NonZeroU64, path::PathBuf, convert::TryInto};

use structopt::StructOpt;
use vcd::{self, ScopeItem, Value, TimescaleUnit, SimulationCommand};
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

mod db;
mod mmap_vec;

#[derive(Debug, StructOpt)]
#[structopt(name = "ligeia", about="A waveform display program.")]
struct Opt {
    /// Input file
    #[structopt(parse(from_os_str))]
    file: PathBuf,
}

fn main() -> io::Result<()> {
    let opt: Opt = Opt::from_args();
    println!("{:?}", opt);

    let mut f = File::open(&opt.file)?;

    let mut parser = vcd::Parser::new(&mut f);

    let header = parser.parse_header()?;

    // println!("{:#?}", header);

    let (tree, vars) = db::find_all_scopes_and_variables(header);

    for var_id in vars {
        println!("{}", var_id);
    }

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new().build(&event_loop).unwrap();

    let instance = wgpu::Instance::new(wgpu::BackendBit::PRIMARY);
    let surface = unsafe { instance.create_surface(&window) };

    let (device, queue) = futures::executor::block_on(async {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
            })
            .await
            .unwrap();
        
        adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    shader_validation: true,
                    ..Default::default()
                },
                None
            )
            .await
            .unwrap()
    });

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                window_id,
            } if window_id == window.id() => *control_flow = ControlFlow::Exit,
            _ => {},
        }
    })

    // let mut data = HashMap::new();

    // for var_id in vars {
    //     println!("{}", var_id);
    // }

    // println!("{:#?}", tree);

    // for scope in &scope_tree.scopes {
    //     println!("{}", scope.name);
    // }

    // let mut mmap: MmapVarVec<u64> = unsafe { MmapVarVec::create_temp()? };

    // for i in 0..10_000 {
    //     mmap.push(i);
    // }

    // let mut counter = 0;
    // for i in mmap.iter() {
    //     // print!("{}, ", i);
    //     if i != counter {
    //         eprintln!("not sequential at {}, {}", i, counter);
    //         break;
    //     }
    //     counter += 1;
    // }

    // drop(mmap);

    // let mmap: MmapVecMut<u64> = unsafe { MmapVecMut::open("test.db")? };

    // let mut counter = 0;

    // // println!("{:?}", mmap.iter().nth(2));

    // for i in mmap.iter() {
    //     // print!("{}, ", i);
    //     if i != counter {
    //         eprintln!("not sequential at {}, {}", i, counter);
    //         break;
    //     }
    //     counter += 1;
    // }

    // Ok(())
}

// fn load_data()
