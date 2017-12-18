extern crate sha1;
extern crate cargo_metadata;
extern crate getopts;
use getopts::Options;
use std::env;
use cargo_metadata::{Package, Target};
use std::path::{Path, PathBuf};
use std::fs;
use std::io;
use std::io::Write;

fn main() {
    let mut opts = Options::new();
    opts.optopt("", "manifest-path", "Rust project location", "Cargo.toml");
    let matches = match opts.parse(env::args().skip(1)) {
        Ok(m) => { m }
        Err(f) => { panic!(f.to_string()) }
    };

    let path = matches.opt_str("manifest-path").map(PathBuf::from);

    let meta = cargo_metadata::metadata(path.as_ref().map(|p| p.as_path())).unwrap();

    let ok = meta.packages.into_iter().filter_map(filter_package).map(|p| {
        let g = Generator::new(p);
        let p = g.write_pbxproj().unwrap();
        println!("Written {}", p.display());
    }).count();

    if ok == 0 {
        eprintln!(r#"No libraries with crate-type "staticlib" or "cdylib""#);
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

struct Generator {
    id_base: sha1::Sha1,
    package: Package,
}

impl Generator {
    pub fn new(package: Package) -> Self {
        let mut id_base = sha1::Sha1::new();
        id_base.update(package.id.as_bytes());

        Self {
            id_base,
            package,
        }
    }

    fn make_id(&self, kind: &str, name: &str) -> String {
        let mut sha = self.id_base.clone();
        sha.update(kind.as_bytes());
        sha.update(&[0]);
        sha.update(name.as_bytes());

        let mut out = String::with_capacity(24);
        out.push_str("CA60");
        for &byte in &sha.digest().bytes()[0..10] {
            out.push_str(&format!("{:02X}", byte));
        }
        debug_assert_eq!(24, out.len());
        out
    }

    fn write_pbxproj(&self) -> Result<PathBuf, Box<std::error::Error>> {
        let proj_path = self.prepare_project_path()?;
        let proj_data = self.pbxproj()?;

        let pbx_path = proj_path.join("project.pbxproj");
        let mut f = fs::File::create(pbx_path)?;
        f.write_all(proj_data.as_bytes())?;

        Ok(proj_path)
    }

    fn products_pbxproj(&self, cargo_dependency_id: &str) -> (String, String, String, String, bool) {
        let mut object_defs = String::new();
        let mut product_refs = String::new();
        let mut target_attrs = String::new();
        let mut target_refs = String::new();

        let mut has_static = false;

        for target in &self.package.targets {
            for kind in &target.kind {
                let (base_name_prefix, file_name, file_type, prod_type) = match kind.as_str() {
                    "bin" => ("", target.name.clone(), "compiled.mach-o.executable", "com.apple.product-type.tool"),
                    "cdylib" => ("lib", format!("lib{}.dylib", target.name), "compiled.mach-o.dylib", "com.apple.product-type.library.dynamic"),
                    "staticlib" => {
                        has_static = true;
                        ("", format!("lib{}.a", target.name), "archive.ar", "com.apple.product-type.library.static")
                    },
                    _ => continue,
                };

                let prod_id = self.make_id(&file_type, &file_name);
                let target_id = self.make_id(&file_type, &prod_id);
                let conf_list_id = self.make_id("<config-list>", &prod_id);
                let conf_release_id = self.make_id("<config-release>", &prod_id);
                let conf_debug_id = self.make_id("<config-debug>", &prod_id);

                product_refs.push_str(&format!("{} /* {} */,\n", prod_id, kind));
                target_refs.push_str(&format!("{} /* {} */,\n", target_id, kind));

                object_defs.push_str(&format!(r##"{target_id} /* {kind} */ = {{
            isa = PBXNativeTarget;
            buildConfigurationList = {conf_list_id};
            buildPhases = (
            );
            buildRules = (
            );
            dependencies = (
                {cargo_dependency_id}
            );
            name = "{base_name_prefix}{base_name}";
            productName = "{file_name}";
            productReference = {prod_id};
            productType = "{prod_type}";
        }};

        {conf_list_id} /* {kind} */ = {{
            isa = XCConfigurationList;
            buildConfigurations = (
                {conf_release_id} /* Release */,
                {conf_debug_id} /* Debug */,
            );
            defaultConfigurationIsVisible = 0;
            defaultConfigurationName = Release;
        }};

        {conf_release_id} /* {kind} */ = {{
            isa = XCBuildConfiguration;
            buildSettings = {{
                PRODUCT_NAME = "$(TARGET_NAME)";
            }};
            name = Release;
        }};

        {conf_debug_id} /* {kind} */ = {{
            isa = XCBuildConfiguration;
            buildSettings = {{
                PRODUCT_NAME = "$(TARGET_NAME)";
            }};
            name = Debug;
        }};

        {prod_id} /* {kind} */ = {{
            isa = PBXFileReference;
            explicitFileType = "{file_type}";
            includeInIndex = 0;
            name = {file_name};
            sourceTree = BUILT_PRODUCTS_DIR;
        }};"##, base_name = target.name,
        conf_release_id = conf_release_id,
        conf_debug_id = conf_debug_id,
        conf_list_id = conf_list_id,
        kind = kind, prod_id = prod_id,
        prod_type = prod_type,
        target_id = target_id,
        cargo_dependency_id = cargo_dependency_id,
        base_name_prefix = base_name_prefix,
        file_name = file_name,
        file_type = file_type));

                // path of product does not seem to work. Xcode writes it, but can't read it.

                target_attrs.push_str(&format!(r##"{target_id} /* {kind} */ = {{
                        CreatedOnToolsVersion = 9.2;
                        ProvisioningStyle = Automatic;
                    }};
"##, target_id = target_id, kind = kind));
            }
        }
        (object_defs, product_refs, target_attrs, target_refs, has_static)
    }

    pub fn pbxproj(&self) -> Result<String, Box<std::error::Error>> {
        let main_group_id = self.make_id("", "<root>");
        let prod_group_id = self.make_id("", "Products");
        let project_id = self.make_id("", "<project>");
        let cargo_target_id = self.make_id("", "<cargo>");
        let target_proxy_id = self.make_id("proxy", "<cargo>");
        let cargo_dependency_id = self.make_id("dep", "<cargo>");
        let conf_list_id = self.make_id("", "<configuration-list>");
        let conf_release_id = self.make_id("configuration", "Release");
        let conf_debug_id = self.make_id("configuration", "Debug");

        let (products, product_refs, target_attrs, target_refs, has_static) = self.products_pbxproj(&cargo_dependency_id);

        let (static_libs, static_libs_ref) = if has_static {
            (r#"        /* Rust needs libresolv */
        ADDEDBA66A6E1 = {
            isa = PBXFileReference; lastKnownFileType = "sourcecode.text-based-dylib-definition";
            name = libresolv.tbd; path = usr/lib/libresolv.tbd; sourceTree = SDKROOT;
        };
        ADDEDBA66A6E2 = {
            isa = PBXGroup;
            children = (
                ADDEDBA66A6E1
            );
            name = "Required Libraries";
            sourceTree = "<group>";
        };"#, "ADDEDBA66A6E2")
        } else {("","")};

        let tpl = format!(r###"// !$*UTF8*$!
{{
    archiveVersion = 1;
    objectVersion = 42;
    objects = {{
        {main_group_id} = {{
            isa = PBXGroup;
            children = (
                {prod_group_id},
                {static_libs_ref}
            );
            sourceTree = "<group>";
        }};

        {products}

        {prod_group_id} = {{
            isa = PBXGroup;
            children = (
                {product_refs}            );
            name = Products;
            sourceTree = "<group>";
        }};

        {cargo_target_id} = {{
            isa = PBXLegacyTarget;
            buildArgumentsString = "build $(CARGO_FLAGS)";
            buildConfigurationList = {conf_list_id};
            buildPhases = (
            );
            buildToolPath = "$(HOME)/.cargo/bin/cargo";
            buildWorkingDirectory = "$(SRCROOT)";
            name = Cargo;
            passBuildSettingsInEnvironment = 1;
            productName = Cargo;
        }};

        {target_proxy_id} = {{
            isa = PBXContainerItemProxy;
            containerPortal = {project_id};
            proxyType = 1;
            remoteGlobalIDString = {cargo_target_id};
            remoteInfo = Cargo;
        }};

        {cargo_dependency_id} = {{
            isa = PBXTargetDependency;
            target = {cargo_target_id};
            targetProxy = {target_proxy_id};
        }};

        {conf_list_id} = {{
            isa = XCConfigurationList;
            buildConfigurations = (
                {conf_release_id} /* Release */,
                {conf_debug_id} /* Debug */,
            );
            defaultConfigurationIsVisible = 0;
            defaultConfigurationName = Release;
        }};

        {conf_release_id} = {{
            isa = XCBuildConfiguration;
            buildSettings = {{
                CONFIGURATION_BUILD_DIR = "$(BUILD_DIR)/target/release"; /* hack for Cargo */
                CARGO_TARGET_DIR = "$(BUILD_DIR)/target"; /* hack for Cargo */
                CARGO_FLAGS = "--release";
                ARCHS = "$(NATIVE_ARCH_ACTUAL)";
                ONLY_ACTIVE_ARCH = YES;
                SDKROOT = macosx;
            }};
            name = Release;
        }};

        {conf_debug_id} = {{
            isa = XCBuildConfiguration;
            buildSettings = {{
                CONFIGURATION_BUILD_DIR = "$(BUILD_DIR)/target/debug"; /* hack for Cargo */
                CARGO_TARGET_DIR = "$(BUILD_DIR)/target"; /* hack for Cargo */
                CARGO_FLAGS = "";
                ARCHS = "$(NATIVE_ARCH_ACTUAL)";
                ONLY_ACTIVE_ARCH = YES;
                SDKROOT = macosx;
            }};
            name = Debug;
        }};

        {static_libs}

        {project_id} = {{
            isa = PBXProject;
            attributes = {{
                LastUpgradeCheck = 0920;
                TargetAttributes = {{
                    {target_attrs}                }};
            }};
            buildConfigurationList = {conf_list_id};
            compatibilityVersion = "Xcode 8.0";
            mainGroup = {main_group_id};
            productRefGroup = {prod_group_id};
            projectDirPath = "";
            projectRoot = "";
            targets = (
                {target_refs}
                {cargo_target_id}
            );
        }};
    }};
    rootObject = {project_id};
}}
    "###,
    project_id = project_id,
    main_group_id = main_group_id,
    prod_group_id = prod_group_id,
    product_refs = product_refs,
    static_libs = static_libs,
    static_libs_ref = static_libs_ref,
    products = products,
    target_attrs = target_attrs,
    target_refs = target_refs,
    cargo_target_id = cargo_target_id,
    cargo_dependency_id = cargo_dependency_id,
    target_proxy_id = target_proxy_id,
    conf_list_id = conf_list_id,
    conf_debug_id = conf_debug_id,
    conf_release_id = conf_release_id);

        Ok(tpl)
    }

    fn prepare_project_path(&self) -> Result<PathBuf, io::Error> {

        let proj_path = Path::new(&self.package.manifest_path).with_file_name(format!("{}.xcodeproj", self.package.name));
        fs::create_dir_all(&proj_path)?;
        Ok(proj_path)
    }
}
