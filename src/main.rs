extern crate cargo_metadata;
extern crate getopts;
extern crate sha1;
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
        Ok(m) => m,
        Err(f) => panic!(f.to_string()),
    };

    let path = matches.opt_str("manifest-path").map(PathBuf::from);

    let meta = cargo_metadata::metadata(path.as_ref().map(|p| p.as_path())).unwrap();

    let ok = meta.packages
        .into_iter()
        .filter_map(filter_package)
        .map(|p| {
            let g = Generator::new(p);
            let p = g.write_pbxproj().unwrap();
            println!("Written {}", p.display());
        })
        .count();

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

struct XcodeTarget {
    kind: String,
    base_name: String,
    file_name: String,
    base_name_prefix: &'static str,
    file_type: &'static str,
    prod_type: &'static str,
}

struct XcodeObject {
    id: String,
    def: String,
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

    fn project_targets(&self) -> Vec<XcodeTarget> {
        self.package.targets.iter().flat_map(|target| target.kind.iter().zip(std::iter::repeat(target.name.clone())).filter_map(|(kind, base_name)| {
            let (base_name_prefix, file_name, file_type, prod_type) = match kind.as_str() {
                "bin" => ("", base_name.clone(), "compiled.mach-o.executable", "com.apple.product-type.tool"),
                "cdylib" => ("lib", format!("lib{}.dylib", base_name), "compiled.mach-o.dylib", "com.apple.product-type.library.dynamic"),
                "staticlib" => {
                    ("", format!("lib{}.a", base_name), "archive.ar", "com.apple.product-type.library.static")
                },
                _ => return None,
            };

            Some(XcodeTarget {
                kind: kind.to_owned(),
                base_name,
                base_name_prefix,
                file_name, file_type,
                prod_type,
            })
        })).collect()
    }

    fn products_pbxproj(&self, cargo_targets: &[XcodeTarget], cargo_dependency_id: &str) -> (Vec<XcodeObject>, Vec<XcodeObject>, Vec<XcodeObject>) {
        let mut other = Vec::new();
        let mut targets = Vec::new();
        let mut products = Vec::new();

        for target in cargo_targets.iter() {
            let prod_id = self.make_id(target.file_type, &target.file_name);
            let target_id = self.make_id(target.file_type, &prod_id);
            let conf_list_id = self.make_id("<config-list>", &prod_id);
            let conf_release_id = self.make_id("<config-release>", &prod_id);
            let conf_debug_id = self.make_id("<config-debug>", &prod_id);

            targets.push(XcodeObject {
                id: target_id.clone(),
                def: format!(
                    r##"{target_id} /* {kind} */ = {{
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
        }};"##,
                    base_name = target.base_name,
                    prod_type = target.prod_type,
                    prod_id = prod_id,
                    file_name = target.file_name,
                    conf_list_id = conf_list_id,
                    kind = target.kind,
                    target_id = target_id,
                    cargo_dependency_id = cargo_dependency_id,
                    base_name_prefix = target.base_name_prefix,
                ),
            });

            other.push(XcodeObject {
                id: conf_list_id.to_owned(),
                def: format!(
                    r##"
        {conf_list_id} /* {kind} */ = {{
            isa = XCConfigurationList;
            buildConfigurations = (
                {conf_release_id} /* Release */,
                {conf_debug_id} /* Debug */,
            );
            defaultConfigurationIsVisible = 0;
            defaultConfigurationName = Release;
        }};"##,
                    conf_list_id = conf_list_id,
                    kind = target.kind,
                    conf_release_id = conf_release_id,
                    conf_debug_id = conf_debug_id,
                ),
            });

            other.push(XcodeObject {
                id: conf_release_id.to_owned(),
                def: format!(
                    r##"
        {conf_release_id} /* {kind} */ = {{
            isa = XCBuildConfiguration;
            buildSettings = {{
                PRODUCT_NAME = "$(TARGET_NAME)";
            }};
            name = Release;
        }};"##,
                    conf_release_id = conf_release_id,
                    kind = target.kind
                ),
            });

            other.push(XcodeObject {
                id: conf_release_id.to_owned(),
                def: format!(
                    r##"
        {conf_debug_id} /* {kind} */ = {{
            isa = XCBuildConfiguration;
            buildSettings = {{
                PRODUCT_NAME = "$(TARGET_NAME)";
            }};
            name = Debug;
        }};"##,
                    conf_debug_id = conf_debug_id,
                    kind = target.kind
                ),
            });

            products.push(XcodeObject {
                id: prod_id.to_owned(),
                // path of product does not seem to work. Xcode writes it, but can't read it.
                def: format!(
                    r##"
        {prod_id} /* {kind} */ = {{
            isa = PBXFileReference;
            explicitFileType = "{file_type}";
            includeInIndex = 0;
            name = {file_name};
            sourceTree = BUILT_PRODUCTS_DIR;
        }};"##,
                    prod_id = prod_id,
                    kind = target.kind,
                    file_name = target.file_name,
                    file_type = target.file_type
                ),
            });
        }
        (targets, products, other)
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

        let targets = self.project_targets();
        let has_static = targets.iter().any(|t| t.prod_type == "com.apple.product-type.library.static");
        let (mut targets, products, mut other_defs) = self.products_pbxproj(&targets, &cargo_dependency_id);

        targets.push(XcodeObject {
            id: cargo_target_id.clone(),
            def: format!(
                r##"{cargo_target_id} = {{
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
            "##,
                cargo_target_id = cargo_target_id,
                conf_list_id = conf_list_id
            ),
        });

        let product_refs = products.iter().map(|o| format!("{},\n", o.id)).collect::<String>();
        let target_refs = targets.iter().map(|o| format!("{},\n", o.id)).collect::<String>();
        let target_attrs = targets.iter().map(|o| format!(r"{} = {{
                        CreatedOnToolsVersion = 9.2;
                        ProvisioningStyle = Automatic;
                    }};
                    ", o.id)).collect::<String>();
        let mut folder_refs = Vec::new();

        if has_static {
            other_defs.push(XcodeObject {
                id: "ADDEDBA66A6E1".to_owned(),
                def: r#"
                    /* Rust needs libresolv */
                    ADDEDBA66A6E1 = {
                        isa = PBXFileReference; lastKnownFileType = "sourcecode.text-based-dylib-definition";
                        name = libresolv.tbd; path = usr/lib/libresolv.tbd; sourceTree = SDKROOT;
                    };
                "#.to_owned(),
            });
            other_defs.push(XcodeObject {
                id: "ADDEDBA66A6E2".to_owned(),
                def: r#"
                ADDEDBA66A6E2 = {
                    isa = PBXGroup;
                    children = (
                        ADDEDBA66A6E1
                    );
                    name = "Required Libraries";
                    sourceTree = "<group>";
                };"#.to_owned(),
            });
            folder_refs.push("ADDEDBA66A6E2".to_owned());
        }

        folder_refs.push(prod_group_id.clone());

        let objects = products.into_iter().chain(targets).chain(other_defs).map(|o| o.def).collect::<String>();
        let folder_refs = folder_refs.iter().map(|id| format!("{},\n", id)).collect::<String>();

        let tpl = format!(r###"// !$*UTF8*$!
{{
    archiveVersion = 1;
    objectVersion = 42;
    objects = {{
        {main_group_id} = {{
            isa = PBXGroup;
            children = (
                {folder_refs}
            );
            sourceTree = "<group>";
        }};

        {objects}

        {prod_group_id} = {{
            isa = PBXGroup;
            children = (
                {product_refs}
            );
            name = Products;
            sourceTree = "<group>";
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
            );
        }};
    }};
    rootObject = {project_id};
}}
    "###,
            project_id = project_id,
            main_group_id = main_group_id,
            prod_group_id = prod_group_id,
            folder_refs = folder_refs,
            product_refs = product_refs,
            objects = objects,
            target_attrs = target_attrs,
            target_refs = target_refs,
            cargo_target_id = cargo_target_id,
            cargo_dependency_id = cargo_dependency_id,
            target_proxy_id = target_proxy_id,
            conf_list_id = conf_list_id,
            conf_debug_id = conf_debug_id,
            conf_release_id = conf_release_id
        );

        Ok(tpl)
    }

    fn prepare_project_path(&self) -> Result<PathBuf, io::Error> {
        let proj_path = Path::new(&self.package.manifest_path).with_file_name(format!("{}.xcodeproj", self.package.name));
        fs::create_dir_all(&proj_path)?;
        Ok(proj_path)
    }
}
