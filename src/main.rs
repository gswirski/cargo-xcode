use getopts::Options;
use std::env;
use cargo_metadata::{Package, Target};

use std::process::exit;

fn main() {
    let mut opts = Options::new();
    opts.optopt("", "manifest-path", "Rust project location", "Cargo.toml");
    opts.optflag("h", "help", "This help");
    let matches = match opts.parse(env::args().skip(1)) {
        Ok(m) => m,
        Err(f) => {
            eprintln!("error: {}", f);
            exit(1);
        },
    };

    if matches.opt_present("help") {
        println!("{}", opts.usage("cargo-xcode generates Xcode project files for Cargo crates"));
        exit(0);
    }

    for arg in matches.free.iter().filter(|&arg| arg != "xcode") {
        eprintln!("warning: '{}' arg unused", arg);
    }

    let path = matches.opt_str("manifest-path");
    let mut cmd = cargo_metadata::MetadataCommand::new();
    if let Some(ref path) = path {
        cmd.manifest_path(path);
    }
    let meta = match cmd.exec() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: Can't parse cargo metadata in {:?} because: {}", path, e);
            exit(1);
        },
    };

    let ok = meta.packages
        .into_iter()
        .filter_map(filter_package)
        .map(|p| {
            let g = cargo_xcode::Generator::new(p);
            let p = g.write_pbxproj().unwrap();
            println!("OK:\n{}", p.display());
        })
        .count();

    if ok == 0 {
        eprintln!(r#"warning: No libraries with crate-type "staticlib" or "cdylib""#);
        exit(1);
    }
}

fn filter_package(mut package: Package) -> Option<Package> {
    package.targets.retain(is_relevant_target);
    if package.targets.is_empty() {
        None
    } else {
        Some(package)
    }
}

fn is_relevant_target(target: &Target) -> bool {
    target.kind.iter().any(|k| k == "bin" || k == "staticlib" || k == "cdylib")
}
