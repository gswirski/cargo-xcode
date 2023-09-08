# Rust libaries as Xcode projects

Generates Xcode project files from `Cargo.toml` allowing use of Rust libraries in Mac and iOS applications without leaving Xcode.

## Requirements

 * [Rust](https://www.rust-lang.org/), preferably installed via [`rustup`](https://rustup.rs/) (tested with 1.56)
 * [Xcode](https://developer.apple.com/xcode/) (tested with 13.1)
 * Bash

Once the Xcode project file is generated, `cargo-xcode` is no longer needed.

## Installation

```sh
cargo install cargo-xcode
```

## Usage

> TL;DR: Run `cargo xcode` and use the generated project files as **sub**projects in other Xcode projects.

This tool will generate Rust-aware project files for all binaries and C-compatible libraries in a Cargo workspace. The generated Xcode projects are not meant for standalone use, and are supposed to be used only as **sub**projects of regular Mac Xcode projects (Xcode can nest projects).

1. If you don't have an existing Cocoa app project yet, create one in Xcode (a normal ObjC or Swift app). This will be called your "parent project" in later steps.

2. If your Rust project is a library, edit `Cargo.toml` and add `crate-type = ["lib", "staticlib"]` in the `[lib]` section. Only libraries of type `"staticlib"` or `"cdylib"` are used (but keep the `"lib"` type for compatibility with Rust libraries and tests).

3. In the same directory as `Cargo.toml` (or root of a Cargo workspace) run:

   ```sh
   cargo xcode
   ```

   This will generate `<rust-project-name>.xcodeproj`. *Don't* open it yet!

4. Open your parent project (from step 1) in Xcode and add the `<rust-project-name>.xcodeproj` to the workspace (drag the file into the parent project's sidebar). You should see the Rust project embedded in your parent project. If the Rust project appears empty in the sidebar, close all Xcode projects and re-open only the parent project.

5. In your parent project's target's **Build Phases**, in **Link Binary With Libraries** phase, you can now add the Rust libraries from the workspace.

### Advanced usage

You can set features via `CARGO_XCODE_FEATURES` target's Build Setting in Xcode.

If you're building `.dylib` for including in an application bundle, make sure to set `DYLIB_INSTALL_NAME_BASE` in Xcode's settings to `@executable_path/../Frameworks/` or whatever location you're going to copy the library to.

## Features

It's better than just launching `cargo build` from a script:

 * Configures Cargo to follow Xcode's Debug and Release configurations.
 * Supports Universal Binaries.
 * Configures Cargo to use Xcode's standard build folder.
 * Makes Xcode aware of dependencies and rebuild Rust code when needed.
 * Xcode's "Clean build folder" also cleans Cargo's target dir.
 * Supports Cargo workspaces and multiple targets per crate.

## Limitations

Rust binaries are exported as command-line tools. This tool intentionally does not make app bundles. If you want to build a Mac GUI app, create one as ObjC or Swift project in Xcode and run Rust code from a Rust static library.

AppleTV and Mac Catalyst targets don't have pre-built rustup targets. You will need to use `xargo` for them (not tested).
