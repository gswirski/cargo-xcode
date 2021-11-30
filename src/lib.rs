use cargo_metadata::Package;
use std::path::{Path, PathBuf};
use std::fs;
use std::io;
use std::io::Write;

struct XcodeTarget {
    kind: String,
    base_name: String,
    file_name: String,
    compiler_flags: String,
    base_name_prefix: &'static str,
    file_type: &'static str,
    prod_type: &'static str,
    supported_platforms: &'static str,
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
const EXECUTABLE_APPLE_PRODUCT_TYPE: &str = "com.apple.product-type.tool";

impl Generator {
    pub fn new(package: Package) -> Self {
        let mut id_base = sha1::Sha1::new();
        id_base.update(package.id.repr.as_bytes());

        Self { id_base, package }
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
                "bin" => ("", base_name.clone(), "compiled.mach-o.executable", EXECUTABLE_APPLE_PRODUCT_TYPE, false),
                "cdylib" => ("lib", format!("lib{}.dylib", base_name.replace('-', "_")), "compiled.mach-o.dylib", "com.apple.product-type.library.dynamic", false),
                "staticlib" => {
                    ("", format!("lib{}.a", base_name.replace('-', "_")), "archive.ar", STATIC_LIB_APPLE_PRODUCT_TYPE, true)
                },
                _ => return None,
            };

            Some(XcodeTarget {
                kind: kind.to_owned(),
                compiler_flags: if prod_type == EXECUTABLE_APPLE_PRODUCT_TYPE { format!("--bin {}", base_name) } else { "--lib".into() },
                supported_platforms: if prod_type == STATIC_LIB_APPLE_PRODUCT_TYPE { "macosx iphonesimulator iphoneos appletvsimulator appletvos" } else { "macosx" },
                base_name,
                base_name_prefix,
                file_name, file_type,
                prod_type,
                skip_install,
            })
        })).collect()
    }

    fn products_pbxproj(&self, cargo_targets: &[XcodeTarget], manifest_path_id: &str, build_rule_id: &str) -> (Vec<XcodeObject>, Vec<XcodeObject>, Vec<XcodeObject>) {
        let mut other = Vec::new();
        let mut targets = Vec::new();
        let mut products = Vec::new();

        for target in cargo_targets.iter() {
            let prod_id = self.make_id(target.file_type, &target.file_name);
            let target_id = self.make_id(target.file_type, &prod_id);
            let conf_list_id = self.make_id("<config-list>", &prod_id);
            let conf_release_id = self.make_id("<config-release>", &prod_id);
            let conf_debug_id = self.make_id("<config-debug>", &prod_id);
            let compile_cargo_id = self.make_id("<cargo>", &prod_id);
            let manifest_path_build_object_id = self.make_id("<cargo-toml>", &prod_id);

            targets.push(XcodeObject {
                id: target_id.clone(),
                def: format!(
                    r##"{target_id} /* {base_name}-{kind} */ = {{
            isa = PBXNativeTarget;
            buildConfigurationList = {conf_list_id};
            buildPhases = (
                {compile_cargo_id}
            );
            buildRules = (
                {build_rule_id}
            );
            dependencies = (
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
                    compile_cargo_id = compile_cargo_id,
                    build_rule_id = build_rule_id,
                    kind = target.kind,
                    target_id = target_id,
                ),
            });

            other.push(XcodeObject {
                id: compile_cargo_id.clone(),
                def: format!(
                    r##"{compile_cargo_id} = {{
                    isa = PBXSourcesBuildPhase;
                    buildActionMask = 2147483647;
                    files = (
                        {manifest_path_build_object_id}
                    );
                    runOnlyForDeploymentPostprocessing = 0;
                }};
                "##,
                    compile_cargo_id = compile_cargo_id,
                    manifest_path_build_object_id = manifest_path_build_object_id,
                ),
            });

            other.push(XcodeObject {
                id: manifest_path_build_object_id.clone(),
                def: format!(r#"
                {manifest_path_build_object_id} /* Cargo.toml in Sources */ = {{
                    isa = PBXBuildFile;
                    fileRef = {manifest_path_id} /* Cargo.toml */;
                    settings = {{
                        COMPILER_FLAGS = "{compiler_flags}"; /* == OTHER_INPUT_FILE_FLAGS */
                    }};
                }};
                "#,
                    manifest_path_build_object_id = manifest_path_build_object_id,
                    manifest_path_id = manifest_path_id,
                    compiler_flags = target.compiler_flags,
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
                    "CARGO_XCODE_CARGO_FILE_NAME" = "{file_name}";
                    "CARGO_XCODE_CARGO_DEP_FILE_NAME" = "{dep_file_name}";
                    SUPPORTED_PLATFORMS = "{supported_platforms}";
                    {skip_install_flags}
                }};
                name = {name};
            }};"##,
                    name = name,
                    id = id,
                    kind = target.kind,
                    file_name = target.file_name,
                    dep_file_name = Path::new(&target.file_name).with_extension("d").file_name().unwrap().to_str().unwrap(),
                    base_name = target.base_name,
                    base_name_prefix = target.base_name_prefix,
                    supported_platforms = target.supported_platforms,
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
        let build_rule_id = self.make_id("", "BuildRule");
        let conf_list_id = self.make_id("", "<configuration-list>");
        let conf_release_id = self.make_id("configuration", "Release");
        let conf_debug_id = self.make_id("configuration", "Debug");
        let manifest_path_id = self.make_id("", "Cargo.toml");

        let rust_targets = self.project_targets();
        let has_static = rust_targets.iter().any(|t| t.prod_type == STATIC_LIB_APPLE_PRODUCT_TYPE);
        let (targets, products, mut other_defs) = self.products_pbxproj(&rust_targets, &manifest_path_id, &build_rule_id);


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
                    lastKnownFileType = text;
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

        let build_script = r##"
set -eu; export PATH=$PATH:~/.cargo/bin:/usr/local/bin;
if [ "${IS_MACCATALYST-NO}" = YES ]; then
    CARGO_XCODE_TARGET_TRIPLE="${CARGO_XCODE_TARGET_ARCH}-apple-ios-macabi"
else
    CARGO_XCODE_TARGET_TRIPLE="${CARGO_XCODE_TARGET_ARCH}-apple-${CARGO_XCODE_TARGET_OS}"
fi
if [ "$CARGO_XCODE_TARGET_OS" != "darwin" ]; then
    PATH="${PATH/\/Contents\/Developer\/Toolchains\/XcodeDefault.xctoolchain\/usr\/bin:/xcode-provided-ld-cant-link-lSystem-for-the-host-build-script:}"
fi
if [ "$CARGO_XCODE_BUILD_MODE" == release ]; then
    OTHER_INPUT_FILE_FLAGS="${OTHER_INPUT_FILE_FLAGS} --release"
fi
if ! rustup target list --installed | egrep -q "${CARGO_XCODE_TARGET_TRIPLE}"; then
    echo "warning: this build requires rustup toolchain for $CARGO_XCODE_TARGET_TRIPLE, but it isn't installed"
    rustup target add "${CARGO_XCODE_TARGET_TRIPLE}" || echo >&2 "warning: can't install $CARGO_XCODE_TARGET_TRIPLE"
fi
if [ "$ACTION" = clean ]; then
 echo cargo clean ${OTHER_INPUT_FILE_FLAGS} --target="${CARGO_XCODE_TARGET_TRIPLE}"
 cargo clean ${OTHER_INPUT_FILE_FLAGS} --target="${CARGO_XCODE_TARGET_TRIPLE}"
else
 echo cargo build ${OTHER_INPUT_FILE_FLAGS} --target="${CARGO_XCODE_TARGET_TRIPLE}"
 cargo build ${OTHER_INPUT_FILE_FLAGS} --target="${CARGO_XCODE_TARGET_TRIPLE}"
fi
# it's too hard to explain Cargo's actual exe path to Xcode build graph, so hardlink to a known-good path instead
BUILT_SRC="${CARGO_TARGET_DIR}/${CARGO_XCODE_TARGET_TRIPLE}/${CARGO_XCODE_BUILD_MODE}/${CARGO_XCODE_CARGO_FILE_NAME}"
BUILD_DST="${BUILT_PRODUCTS_DIR}/${EXECUTABLE_PATH}"
ln -f -- "$BUILT_SRC" "$BUILD_DST"

# xcode generates dep file, but for its own path, so append our rename to it
DEP_FILE_SRC="${CARGO_TARGET_DIR}/${CARGO_XCODE_TARGET_TRIPLE}/${CARGO_XCODE_BUILD_MODE}/${CARGO_XCODE_CARGO_DEP_FILE_NAME}"
if [ -f "$DEP_FILE_SRC" ]; then
    DEP_FILE_DST="${BUILT_PRODUCTS_DIR}/${EXECUTABLE_NAME}.d"
    cp -f "$DEP_FILE_SRC" "$DEP_FILE_DST"
    echo >> "$DEP_FILE_DST" "$BUILD_DST: $BUILT_SRC"
fi
"##.escape_default();

        let common_build_settings = format!(r##"
            ALWAYS_SEARCH_USER_PATHS = NO;
            SUPPORTS_MACCATALYST = YES;
            CARGO_TARGET_DIR = "$(TARGET_TEMP_DIR)/cargo-target"; /* for cargo */
            "CARGO_XCODE_TARGET_ARCH[arch=arm64*]" = "aarch64";
            "CARGO_XCODE_TARGET_ARCH[arch=x86_64*]" = "x86_64"; /* catalyst adds h suffix */
            "CARGO_XCODE_TARGET_ARCH[arch=i386]" = "i686";
            "CARGO_XCODE_TARGET_OS[sdk=macosx*]" = "darwin";
            "CARGO_XCODE_TARGET_OS[sdk=iphonesimulator*]" = "ios-sim";
            "CARGO_XCODE_TARGET_OS[sdk=iphoneos*]" = "ios";
            "CARGO_XCODE_TARGET_OS[sdk=appletvsimulator*]" = "tvos";
            "CARGO_XCODE_TARGET_OS[sdk=appletvos*]" = "tvos";
            PRODUCT_NAME = "{product_name}";
            SDKROOT = macosx;
        "##,
            product_name = self.package.name, // used as a base for output filename in Xcode
        );

        let tpl = format!(
            r###"// !$*UTF8*$!
{{
    archiveVersion = 1;
    objectVersion = 55;
    objects = {{
        {build_rule_id} = {{
            isa = PBXBuildRule;
            compilerSpec = com.apple.compilers.proxy.script;
            dependencyFile = "$(BUILT_PRODUCTS_DIR)/$(EXECUTABLE_NAME).d";
            name = "Cargo project build";
            filePatterns = "*/Cargo.toml"; /* must contain asterisk */
            fileType = pattern.proxy;
            inputFiles = ();
            isEditable = 1;
            outputFiles = (
                "$(BUILT_PRODUCTS_DIR)/$(EXECUTABLE_PATH)",
            );
            script = "{build_script}";
        }};

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
                {common_build_settings}
                "CARGO_XCODE_BUILD_MODE" = "release"; /* for xcode scripts */
            }};
            name = Release;
        }};

        {conf_debug_id} = {{
            isa = XCBuildConfiguration;
            buildSettings = {{
                {common_build_settings}
                "CARGO_XCODE_BUILD_MODE" = "debug"; /* for xcode scripts */
                ONLY_ACTIVE_ARCH = YES;
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
            build_rule_id = build_rule_id,
            build_script = build_script,
            main_group_id = main_group_id,
            prod_group_id = prod_group_id,
            folder_refs = folder_refs,
            product_refs = product_refs,
            objects = objects,
            target_attrs = target_attrs,
            target_refs = target_refs,
            conf_list_id = conf_list_id,
            conf_debug_id = conf_debug_id,
            conf_release_id = conf_release_id,
            common_build_settings = common_build_settings
        );

        Ok(tpl)
    }

    fn prepare_project_path(&self) -> Result<PathBuf, io::Error> {
        let proj_path = Path::new(&self.package.manifest_path).with_file_name(format!("{}.xcodeproj", self.package.name));
        fs::create_dir_all(&proj_path)?;
        Ok(proj_path)
    }
}
