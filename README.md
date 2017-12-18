# Rust libaries as Xcode projects

Generates Xcode project files from `Cargo.toml` allowing use of Rust libraries in Mac Cocoa applications without leaving Xcode.

## Requirements

 * [Rust](https://www.rust-lang.org/) (tested with 1.23)
 * [Xcode](https://developer.apple.com/xcode/) (tested with 9.2)

Once the Xcode project file is generated, `cargo-xcode` is no longer needed.

## Installation

```sh
cargo install cargo-xcode
```

## Usage

> TL;DR: Run `cargo xcode` and use the generated project files as **sub**projects in other Xcode projects.

This tool will generate Rust-specific project files for all binaries and C-compatible libraries in Cargo workspace. The generated Xcode projects are not suitable for standalone use, and are supposed to be used only as **sub**projects of regular Mac Xcode projects (Xcode can nest projects).

1. If you don't have an existing Cocoa Mac app project yet, create one in Xcode (normal ObjC or Swift app). This will be called your "parent project" in later steps.

2. If your Rust project is a library, edit `Cargo.toml` and add `crate-type = ["lib", "staticlib"]` in the `[lib]` section. Only libraries of type `"staticlib"` or `"cdylib"` are used (leave `"lib"` type for compatibility with Rust libraries and tests).

3. In the same directory as `Cargo.toml` (or root of a Cargo workspace) run:

   ```sh
   cargo xcode
   ```

   This will generate `<rust-project-name>.xcodeproj`. *Don't* open it yet!

4. Open your parent project (from step 1) in Xcode and add `<rust-project-name>.xcodeproj` to the workspace (drag the file into the parent project's sidebar). You should see the Rust project embedded in your parent project. If the Rust project appears empty in the sidebar, close all Xcode projects and re-open only the parent project.

5. In your parent project's target's **Build Phases**, in **Link Binary With Libraries** phase, you can now add Rust libraries from the workspace.

6. If you're linking with *static* Rust libraries, also link your executables/libraries with `libresolv.dylib` (without it Xcode won't find `_res_9_init` required by Rust's stdlib).

## Features

It's better than just launching `cargo build` from a script:

 * Configures Cargo to follow Xcode's Debug and Release configurations.
 * Configures Cargo to use Xcode's standard build folder.
 * Makes Xcode aware of dependencies and rebuild Rust code when needed.
 * Supports Cargo workspaces and multiple targets per crate.

## Limitations

Only Mac target is supported at the moment. Only native architecture (i.e. 64-bit Intel).

Rust binaries are exported as command-line tools. This tool intentionally does not make app bundles. If you want to build a Mac GUI app, create one as ObjC project in Xcode and run Rust code from a Rust static library.
