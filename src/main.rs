use clap::Parser;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::{fs, path};
use svd_rs as svd;

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to SVDs
    inpath: Option<PathBuf>,

    /// Don't delete descriptions
    #[arg(short('d'), long)]
    keep_descriptions: bool,

    /// Compare ogirin files instead of patched
    #[arg(short('o'), long)]
    origin: bool,

    #[arg(short('n'), long)]
    show_name: bool,
}

fn main() {
    let args = Args::parse();
    let pth = path::Path::new(if args.origin { "yamls_orig" } else { "yamls" });
    if pth.is_dir() {
        std::fs::remove_dir_all(&pth).unwrap();
    }
    let mut entries: Vec<_> = fs::read_dir(args.inpath.as_deref().unwrap_or(Path::new(".")))
        .unwrap()
        .filter_map(|f| f.ok())
        .collect();
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let svd_fn = entry.path();
        let ext = if args.origin { "svd" } else { "patched" };
        if svd_fn.extension() == Some(std::ffi::OsStr::new(ext)) {
            let svd_xml = &mut String::new();
            fs::File::open(&svd_fn)
                .expect("Failed to open SVD input file")
                .read_to_string(svd_xml)
                .expect("Failed to read SVD input file to a String");

            let config = svd_parser::Config::default().validate_level(svd::ValidateLevel::Disabled);
            //config.validate_level = svd::ValidateLevel::Strict;
            let mut device = svd_parser::parse_with_config(svd_xml, &config)
                .expect("Failed to parse the SVD file into Rust structs");

            if device.name.starts_with("STM32MP1") {
                continue;
            }

            println!("Device {} ({:?})", device.name, svd_fn);

            if !args.keep_descriptions {
                clean_device(&mut device);
            }

            let mut groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

            for p in &device.peripherals {
                let mut p2 = p.clone();
                clear_fields(&mut p2);
                if p.derived_from.is_none() {
                    groups
                        .entry(p.group_name.clone().unwrap_or_else(|| p.name.clone()))
                        .or_default()
                        .insert(p.name.clone());
                }
                if let (Some(registers1), Some(registers2)) =
                    (p.registers.as_ref(), p2.registers.as_ref())
                {
                    let s1 = serde_yaml::to_string(&registers1).expect("Serialization failed");
                    let s2 = serde_yaml::to_string(&registers2).expect("Serialization failed");
                    let digest1 = format!("{:?}", md5::compute(s1.as_bytes()));
                    let digest2 = format!("{:?}", md5::compute(s2.as_bytes()));
                    let digest = if args.show_name {
                        format!("{}_{}_{}", &digest1[..8], device.name, p.name)
                    } else {
                        format!("{}_{}", &digest1[..8], &digest2[..8])
                    };
                    let yaml_fn = format!("{}.yaml", digest,);
                    let refer = format!("{} {} {}\n", digest, p.name, device.name);
                    let mut pth = path::PathBuf::from(pth);
                    pth.push(p.group_name.as_ref().unwrap_or_else(|| &p.name));
                    fs::create_dir_all(&pth).unwrap();
                    let mut ymlpth = pth.clone();
                    let mut txtpth = pth.clone();
                    ymlpth.push(&yaml_fn);
                    txtpth.push("peripherals.txt");
                    if !ymlpth.exists() {
                        fs::File::create(&ymlpth)
                            .expect("Failed to open JSON output file")
                            .write_all(s1.as_bytes())
                            .expect("Failed to write to JSON output file");
                    }
                    if !args.show_name {
                        fs::OpenOptions::new()
                            .write(true)
                            .append(true)
                            .create(true)
                            .open(&txtpth)
                            .expect("Failed to open txt output file")
                            .write_all(refer.as_bytes())
                            .expect("Failed to write to txt output file");
                    }
                }
            }
            for (g, periphs) in &groups {
                if periphs.iter().all(|p| p.starts_with(g)) {
                    let idx = periphs
                        .iter()
                        .map(|p| {
                            let suf = p.strip_prefix(g).unwrap();
                            if suf.len() == 0 {
                                'x'.to_string()
                            } else if suf.len() == 1 {
                                suf.to_string()
                            } else {
                                format!("{{{suf}}}")
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("");
                    if idx == "x" {
                        println!("{g}");
                    } else if idx.len() == 1 {
                        println!("{g}{idx}");
                    } else {
                        println!("{g}[{idx}]",)
                    }
                } else {
                    println!(
                        "{g}: {}",
                        periphs.iter().cloned().collect::<Vec<_>>().join(", ")
                    );
                }
            }
        }
    }
    sort_txts(pth);
}

fn sort_txts(pth: &path::Path) {
    for dir in fs::read_dir(pth).unwrap() {
        if pth.is_dir() {
            let mut txtpth = path::PathBuf::from(dir.unwrap().path());
            txtpth.push("peripherals.txt");
            if txtpth.exists() {
                let mut lines = read_lines(&txtpth)
                    .expect("Failed to read txt")
                    .flatten()
                    .collect::<Vec<_>>();
                lines.sort();
                fs::OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .open(txtpth)
                    .expect("Failed to open txt output file")
                    .write_all(lines.join("\n").as_bytes())
                    .expect("Failed to write to txt output file");
            }
        }
    }
}

fn read_lines<P>(filename: P) -> io::Result<io::Lines<io::BufReader<fs::File>>>
where
    P: AsRef<path::Path>,
{
    let file = fs::File::open(filename)?;
    Ok(io::BufReader::new(file).lines())
}

fn clear_fields(p: &mut svd::Peripheral) {
    let pname = p.name.clone();
    for r in p.all_registers_mut() {
        if r.name.starts_with(&pname) {
            if !r.name.starts_with("OPAMP")
                && !r.name.starts_with("COMP")
                && !r.name.starts_with("EXTICR")
                && !(r.name.starts_with("TIM")
                    && (r.name.ends_with("_OR")
                        || r.name.ends_with("_AF1")
                        || r.name.ends_with("_AF1")
                        || r.name.ends_with("_TISEL")))
            {
                //println!("  r: {}", r.name);
            }
        }
        if let Some(fields) = r.fields.as_mut() {
            for f in fields {
                f.enumerated_values = vec![];
                f.write_constraint = None;
            }
        }
    }
}

fn clean_device(d: &mut svd::Device) {
    for p in &mut d.peripherals {
        clean_peripheral(p);
    }
}

fn clean_peripheral(p: &mut svd::Peripheral) {
    p.description = None;
    p.display_name = None;
    for i in &mut p.interrupt {
        i.description = None;
    }
    if let Some(registers) = p.registers.as_mut() {
        registers.sort_by_key(|rc| match rc {
            svd::RegisterCluster::Register(r) => r.address_offset,
            svd::RegisterCluster::Cluster(c) => c.address_offset,
        });

        for rc in registers {
            clean_register_cluster(rc);
        }
    }
}

fn clean_register_cluster(rc: &mut svd::RegisterCluster) {
    match rc {
        svd::RegisterCluster::Register(r) => clean_register(r),
        svd::RegisterCluster::Cluster(c) => clean_cluster(c),
    }
}
fn clean_cluster(c: &mut svd::Cluster) {
    c.description = None;

    for rc in &mut c.children {
        clean_register_cluster(rc);
    }
}
fn clean_register(r: &mut svd::Register) {
    r.description = None;
    r.display_name = None;

    if let Some(fields) = r.fields.as_mut() {
        fields.sort_by_key(|f| f.bit_range.offset);
        for f in fields {
            f.description = None;

            for evs in &mut f.enumerated_values {
                evs.values.sort_by_key(|ev| ev.value);
                for ev in &mut evs.values {
                    ev.description = None;
                }
            }
        }
    }
}
