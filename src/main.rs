use getopts::Options;
use std::env;
use cargo_metadata::{Package, Target};

use std::process::exit;

fn main() {
    let mut opts = Options::new();
    opts.optopt("", "manifest-path", "Location of the Rust/Cargo project to convert.", "Cargo.toml");
    opts.optopt("", "output-dir", "Where to write xcodeproj to (default: same directory as the crate)", "");
    opts.optopt("", "project-name", "Override crate name to use a differnet name in Xcode", "");
    opts.optflag("h", "help", "This help.");
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
    let output_dir = matches.opt_str("output-dir");
    let mut cmd = cargo_metadata::MetadataCommand::new();
    cmd.no_deps();
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

    let custom_project_name = matches.opt_str("project-name");

    let ok = meta.packages
        .into_iter()
        .filter_map(filter_package)
        .map(move |p| {
            let g = cargo_xcode::Generator::new(p, output_dir.as_ref().map(From::from), custom_project_name.clone());
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
