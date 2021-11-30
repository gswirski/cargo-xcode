use cargo_metadata::Package;
use std::path::{Path, PathBuf};
use std::fs;
use std::io;
use std::io::Write;

struct XcodeTarget {
    kind: String,
    base_name: String,
    file_name: String,
    base_name_prefix: &'static str,
    file_type: &'static str,
    prod_type: &'static str,
    skip_install: bool,
}

struct XcodeObject {
    id: String,
    def: String,
}

pub struct Generator {
    id_base: sha1::Sha1,
    package: Package,
}

const STATIC_LIB_APPLE_PRODUCT_TYPE: &str = "com.apple.product-type.library.static";

impl Generator {
    pub fn new(package: Package) -> Self {
        let mut id_base = sha1::Sha1::new();
        id_base.update(package.id.repr.as_bytes());

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

    pub fn write_pbxproj(&self) -> Result<PathBuf, io::Error> {
        let proj_path = self.prepare_project_path()?;
        let proj_data = self.pbxproj()?;

        let pbx_path = proj_path.join("project.pbxproj");
        let mut f = fs::File::create(pbx_path)?;
        f.write_all(proj_data.as_bytes())?;

        Ok(proj_path)
    }

    fn project_targets(&self) -> Vec<XcodeTarget> {
        self.package.targets.iter().flat_map(|target| target.kind.iter().zip(std::iter::repeat(target.name.clone())).filter_map(|(kind, base_name)| {
            let (base_name_prefix, file_name, file_type, prod_type, skip_install) = match kind.as_str() {
                "bin" => ("", base_name.clone(), "compiled.mach-o.executable", "com.apple.product-type.tool", false),
                "cdylib" => ("lib", format!("lib{}.dylib", base_name.replace('-', "_")), "compiled.mach-o.dylib", "com.apple.product-type.library.dynamic", false),
                "staticlib" => {
                    ("", format!("lib{}.a", base_name.replace('-', "_")), "archive.ar", STATIC_LIB_APPLE_PRODUCT_TYPE, true)
                },
                _ => return None,
            };

            Some(XcodeTarget {
                kind: kind.to_owned(),
                base_name,
                base_name_prefix,
                file_name, file_type,
                prod_type,
                skip_install,
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
            let copy_script_id = self.make_id("<copy>", &prod_id);

            targets.push(XcodeObject {
                id: target_id.clone(),
                def: format!(
                    r##"{target_id} /* {base_name}-{kind} */ = {{
			isa = PBXNativeTarget;
			buildConfigurationList = {conf_list_id};
			buildPhases = (
				{copy_script_id}
			);
			buildRules = (
			);
			dependencies = (
				{cargo_dependency_id}
			);
			name = "{base_name}-{kind}";
			productName = "{file_name}";
			productReference = {prod_id};
			productType = "{prod_type}";
		}};
		"##,
                    base_name = target.base_name,
                    prod_type = target.prod_type,
                    prod_id = prod_id,
                    file_name = target.file_name,
                    conf_list_id = conf_list_id,
                    copy_script_id = copy_script_id,
                    kind = target.kind,
                    target_id = target_id,
                    cargo_dependency_id = cargo_dependency_id,
                ),
            });

            // EXECUTABLE_PATH is relative, e.g. "MyApp.app/Contents/MacOS/MyApp"
            other.push(XcodeObject {
                id: copy_script_id.clone(),
                def: format!(
                    r##"{copy_script_id} = {{
					isa = PBXShellScriptBuildPhase;
					buildActionMask = 2147483647;
					name = "Copy files ({file_name})";
					files = (
					);
					inputFileListPaths = (
					);
					inputPaths = (
						"$(CARGO_XCODE_PRODUCTS_DIR)/{file_name}",
					);
					outputFileListPaths = ();
					outputPaths = (
						"$(BUILT_PRODUCTS_DIR)/$(EXECUTABLE_PATH)",
					);
					runOnlyForDeploymentPostprocessing = 0;
					shellPath = /bin/sh;
					shellScript = "ln -f \"${{CARGO_XCODE_PRODUCTS_DIR}}/{file_name}\" \"${{BUILT_PRODUCTS_DIR}}/${{EXECUTABLE_PATH}}\"";
				}};
				"##,
                    copy_script_id = copy_script_id,
                    file_name = target.file_name
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

            // Xcode tries to chmod it when archiving, even though it doesn't belong to the archive
            let skip_install_flags = if target.skip_install {
                r#"SKIP_INSTALL = YES;
				INSTALL_GROUP = "";
				INSTALL_MODE_FLAG = "";
				INSTALL_OWNER = "";"#
            } else {
                ""
            };

            other.extend([(conf_release_id, "Release"), (conf_debug_id, "Debug")].iter().map(|(id, name)| XcodeObject {
                id: id.to_owned(),
                def: format!(
                    r##"
			{id} /* {kind} */ = {{
				isa = XCBuildConfiguration;
				buildSettings = {{
					PRODUCT_NAME = "{base_name_prefix}{base_name}";
					{skip_install_flags}
				}};
				name = {name};
			}};"##,
                    name = name,
                    id = id,
                    kind = target.kind,
                    base_name = target.base_name,
                    base_name_prefix = target.base_name_prefix,
                    skip_install_flags = skip_install_flags
                ),
            }));

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

    pub fn pbxproj(&self) -> Result<String, io::Error> {
        let main_group_id = self.make_id("", "<root>");
        let prod_group_id = self.make_id("", "Products");
        let project_id = self.make_id("", "<project>");
        let cargo_target_id = self.make_id("", "<cargo>");
        let target_proxy_id = self.make_id("proxy", "<cargo>");
        let cargo_dependency_id = self.make_id("dep", "<cargo>");
        let conf_list_id = self.make_id("", "<configuration-list>");
        let conf_release_id = self.make_id("configuration", "Release");
        let conf_debug_id = self.make_id("configuration", "Debug");
        let manifest_path_id = self.make_id("", "Cargo.toml");
        let aggregate_script_id = self.make_id("", "<cargo>sh");

        let rust_targets = self.project_targets();
        let has_static = rust_targets.iter().any(|t| t.prod_type == STATIC_LIB_APPLE_PRODUCT_TYPE);
        let (mut targets, products, mut other_defs) = self.products_pbxproj(&rust_targets, &cargo_dependency_id);

        targets.push(XcodeObject {
            id: cargo_target_id.clone(),
            def: format!(
                r##"{cargo_target_id} = {{
			isa = PBXAggregateTarget;
			buildConfigurationList = {conf_list_id};
			buildPhases = (
				{aggregate_script_id}
			);
			dependencies = (
			);
			name = Cargo;
			productName = Cargo;
		}};
			"##,
                cargo_target_id = cargo_target_id,
                conf_list_id = conf_list_id,
                aggregate_script_id = aggregate_script_id
            ),
        });

        other_defs.push(XcodeObject {
            id: aggregate_script_id.clone(),
            def: format!(
                r##"{aggregate_script_id} = {{
				isa = PBXShellScriptBuildPhase;
				buildActionMask = 2147483647;
				name = "Cargo build";
				files = (
				);
				inputFileListPaths = (
				);
				inputPaths = (
					"$(SRCROOT)/Cargo.toml"
				);
				outputFileListPaths = (
				);
				outputPaths = (
				);
				runOnlyForDeploymentPostprocessing = 0;
				shellPath = /bin/bash;
				shellScript = "set -e; export PATH=$PATH:~/.cargo/bin:/usr/local/bin;
if [ \"$ACTION\" = \"clean\" ]; then
	cargo clean;
else
	cargo build $CARGO_FLAGS;
fi
";
		}};
			"##,
                aggregate_script_id = aggregate_script_id
            ),
        });

        let product_refs = products.iter().map(|o| format!("{},\n", o.id)).collect::<String>();
        let target_refs = targets.iter().map(|o| format!("{},\n", o.id)).collect::<String>();
        let target_attrs = targets
            .iter()
            .map(|o| {
                format!(
                    r"{} = {{
						CreatedOnToolsVersion = 9.2;
						ProvisioningStyle = Automatic;
					}};
					",
                    o.id
                )
            })
            .collect::<String>();
        let mut folder_refs = Vec::new();

        folder_refs.push(manifest_path_id.clone());
        other_defs.push(XcodeObject {
            id: manifest_path_id.clone(),
            def: format!(
                r#"
				{manifest_path_id} /* Cargo.toml */ = {{
					isa = PBXFileReference;
					lastKnownFileType = "sourcecode.text-based-dylib-definition";
					fileEncoding = 4;
					path = Cargo.toml;
					sourceTree = "<group>";
			}};"#,
                manifest_path_id = manifest_path_id
            ),
        });

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

        let tpl = format!(
            r###"// !$*UTF8*$!
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
				CARGO_TARGET_DIR = "$(BUILD_DIR)/cargo-target"; /* for cargo */
				CARGO_XCODE_PRODUCTS_DIR = "$(BUILD_DIR)/cargo-target/release"; /* for xcode scripts */
				CARGO_FLAGS = "--release";
				ARCHS = "$(NATIVE_ARCH_ACTUAL)";
				ONLY_ACTIVE_ARCH = YES;
				SDKROOT = macosx;
				PRODUCT_NAME = "{product_name}";
			}};
			name = Release;
		}};

		{conf_debug_id} = {{
			isa = XCBuildConfiguration;
			buildSettings = {{
				CARGO_TARGET_DIR = "$(BUILD_DIR)/cargo-target";
				CARGO_XCODE_PRODUCTS_DIR = "$(BUILD_DIR)/cargo-target/debug";
				CARGO_FLAGS = "";
				ARCHS = "$(NATIVE_ARCH_ACTUAL)";
				ONLY_ACTIVE_ARCH = YES;
				SDKROOT = macosx;
				PRODUCT_NAME = "{product_name}";
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
            product_name = self.package.name, // not really used, but Xcode demands it
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
