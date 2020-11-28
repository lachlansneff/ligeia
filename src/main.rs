
use std::{fs::File, collections::HashSet, io, path::PathBuf, collections::HashMap};

use structopt::StructOpt;
use vcd::{self, ScopeItem, Value, TimescaleUnit, SimulationCommand};

mod db;

use db::VarMmapVec;

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

    let (tree, vars) = find_all_scopes_and_variables(header);

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

    Ok(())
}

pub type VarId = u64;

#[derive(Debug)]
pub struct VarInfo {
    name: String,
    id: VarId,
}

#[derive(Debug)]
pub struct Scope {
    name: String,
    scopes: Vec<Scope>,
    vars: Vec<VarInfo>,
}

fn find_all_scopes_and_variables(header: vcd::Header) -> (Scope, Vec<VarId>) {
    fn recurse(ids: &mut Vec<VarId>, items: impl Iterator<Item=vcd::ScopeItem>) -> (Vec<Scope>, Vec<VarInfo>) {
        let mut scopes = vec![];
        let mut vars = vec![];

        for item in items {
            match item {
                ScopeItem::Var(var) => {
                    ids.push(var.code.number());
                    vars.push(VarInfo {
                        name: var.reference,
                        id: var.code.number(),
                    })
                }
                ScopeItem::Scope(scope) => {
                    let (sub_scopes, sub_vars) = recurse(ids, scope.children.into_iter());
                    scopes.push(Scope {
                        name: scope.identifier,
                        scopes: sub_scopes,
                        vars: sub_vars,
                    });
                }
            }
        }

        (scopes, vars)
    }

    let (name, top_items) = header.items.into_iter().find_map(|item| {
        if let ScopeItem::Scope(scope) = item {
            Some((scope.identifier, scope.children))
        } else {
            None
        }
    }).expect("failed to find top-level scope in vcd file");

    let mut ids = Vec::new();
    let (scopes, vars) = recurse(&mut ids, top_items.into_iter());

    ids.sort_unstable();
    ids.dedup();

    // INFO: Turns out the variable ids are usually sequential, but not always
    // let mut previous = vars[0].id;
    // for var in vars[1..].iter() {
    //     if var.id != previous + 1 {
    //         eprintln!("wasn't sequential at {}", var.id);
    //     }
    //     previous = var.id;
    // }
    
    (Scope {
        name, scopes, vars,
    }, ids)
}

// fn load_data()
