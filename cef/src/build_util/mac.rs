use semver::Version;
use serde::Serialize;
use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0:?}")]
    Io(#[from] io::Error),
    #[error("Metadata error: {0:?}")]
    Metadata(#[from] super::metadata::Error),
    #[error("Plist error: {0:?}")]
    Plist(#[from] plist::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Common bundle information that is shared in the [Info.plist](https://developer.apple.com/documentation/bundleresources/information-property-list) files.
#[derive(Clone, Serialize)]
pub struct BundleInfo {
    /// [CFBundleName](https://developer.apple.com/documentation/bundleresources/information-property-list/cfbundlename)
    #[serde(rename = "CFBundleName")]
    pub name: String,
    /// [CFBundleIdentifier](https://developer.apple.com/documentation/bundleresources/information-property-list/cfbundleidentifier)
    #[serde(rename = "CFBundleIdentifier")]
    pub identifier: String,
    /// [CFBundleDisplayName](https://developer.apple.com/documentation/bundleresources/information-property-list/cfbundledisplayname)
    #[serde(rename = "CFBundleDisplayName")]
    pub display_name: String,
    /// [CFBundleDevelopmentRegion](https://developer.apple.com/documentation/bundleresources/information-property-list/cfbundledevelopmentregion)
    #[serde(rename = "CFBundleDevelopmentRegion")]
    pub development_region: String,
    /// [CFBundleVersion](https://developer.apple.com/documentation/bundleresources/information-property-list/cfbundleversion)
    #[serde(rename = "CFBundleVersion", serialize_with = "serialize_version")]
    pub version: Version,
}

impl BundleInfo {
    pub fn new(
        name: &str,
        identifier: &str,
        display_name: &str,
        development_region: &str,
        version: Version,
    ) -> Self {
        Self {
            name: name.to_owned(),
            identifier: identifier.to_owned(),
            display_name: display_name.to_owned(),
            development_region: development_region.to_owned(),
            version,
        }
    }
}

/// See https://bitbucket.org/chromiumembedded/cef/wiki/GeneralUsage.md#markdown-header-macos
pub fn bundle(
    app_path: &Path,
    target_path: &Path,
    executable_name: &str,
    helper_name: &str,
    resources_path: Option<PathBuf>,
    main_nib_file: Option<String>,
    principal_class: Option<String>,
    bundle_info: BundleInfo,
) -> Result<PathBuf> {
    let main_app_path = create_app(
        app_path,
        executable_name,
        false,
        resources_path.as_deref(),
        bundle_info.clone(),
        main_nib_file,
        principal_class,
        &target_path.join(executable_name),
    )?;
    let cef_path = cef_dll_sys::get_cef_dir().unwrap();
    let to = main_app_path.join(FRAMEWORKS_PATH).join(FRAMEWORK);
    if to.exists() {
        fs::remove_dir_all(&to).unwrap();
    }
    copy_directory(&cef_path.join(FRAMEWORK), &to)?;
    // Chromium expects `icudtl.dat` in the main app `Contents/Resources/` (see vmux bootstrap warning).
    let icu_src = to.join("Resources").join("icudtl.dat");
    let icu_dst = main_app_path.join(RESOURCES_PATH).join("icudtl.dat");
    if icu_src.is_file() {
        fs::create_dir_all(main_app_path.join(RESOURCES_PATH))?;
        let _ = fs::remove_file(&icu_dst);
        fs::copy(&icu_src, &icu_dst)?;
    }
    for helper in HELPERS {
        let helper = format!("{executable_name} {helper}");
        create_app(
            &main_app_path.join(FRAMEWORKS_PATH),
            &helper,
            true,
            None,
            bundle_info.clone(),
            None,
            None,
            &target_path.join(helper_name),
        )?;
    }
    if let Some(resources_path) = resources_path {
        let resources_path = resources_path.join("mac");
        let target_path = main_app_path.join(RESOURCES_PATH);
        copy_app_resources(&resources_path, &target_path)?;
    }
    Ok(main_app_path)
}

/// Similar to [`bundle`], but this will invoke `cargo build` to build both the main executable and
/// helper executable targets.
pub fn build_bundle(
    app_path: &Path,
    executable_name: &str,
    bundle_info: BundleInfo,
) -> Result<PathBuf> {
    let cargo_metadata = super::metadata::get_cargo_metadata()?;
    let target_path = cargo_metadata.target_directory().join("debug");
    let bundle_metadata = cargo_metadata.parse_bundle_metadata(executable_name)?;

    cargo_build(executable_name)?;
    cargo_build(&bundle_metadata.helper_name)?;

    bundle(
        app_path,
        &target_path,
        executable_name,
        &bundle_metadata.helper_name,
        bundle_metadata.resources_path,
        bundle_metadata.main_nib_file.clone(),
        bundle_metadata.principal_class.clone(),
        bundle_info,
    )
}

#[derive(Serialize)]
struct InfoPlist {
    #[serde(flatten)]
    bundle_info: BundleInfo,

    #[serde(rename = "CFBundleExecutable")]
    executable_name: String,
    #[serde(rename = "CFBundleInfoDictionaryVersion")]
    bundle_info_dictionary_version: String,
    #[serde(rename = "CFBundlePackageType")]
    bundle_package_type: String,
    #[serde(rename = "CFBundleIconFile", skip_serializing_if = "String::is_empty")]
    icon_file: String,
    #[serde(rename = "CFBundleSignature")]
    bundle_signature: String,
    #[serde(
        rename = "CFBundleShortVersionString",
        serialize_with = "serialize_version"
    )]
    short_version: Version,
    #[serde(rename = "LSEnvironment")]
    environment: HashMap<String, String>,
    #[serde(rename = "LSFileQuarantineEnabled")]
    file_quarantine_enabled: bool,
    #[serde(rename = "LSMinimumSystemVersion")]
    minimum_system_version: String,
    #[serde(
        rename = "LSUIElement",
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_ui_element"
    )]
    ui_element: Option<&'static str>,
    #[serde(rename = "NSBluetoothAlwaysUsageDescription")]
    bluetooth_always_usage_description: String,
    #[serde(rename = "NSSupportsAutomaticGraphicsSwitching")]
    supports_automatic_graphics_switching: bool,
    #[serde(rename = "NSWebBrowserPublicKeyCredentialUsageDescription")]
    web_browser_publickey_credential_usage_description: String,
    #[serde(rename = "NSCameraUsageDescription")]
    camera_usage_description: String,
    #[serde(rename = "NSMicrophoneUsageDescription")]
    microphone_usage_description: String,
    #[serde(rename = "NSMainNibFile", skip_serializing_if = "Option::is_none")]
    main_nib_file: Option<String>,
    #[serde(rename = "NSPrincipalClass", skip_serializing_if = "Option::is_none")]
    principal_class: Option<String>,
}

impl InfoPlist {
    fn new(
        executable_name: &str,
        is_helper: bool,
        icon_file: Option<String>,
        bundle_info: BundleInfo,
        main_nib_file: Option<String>,
        principal_class: Option<String>,
    ) -> Self {
        Self {
            executable_name: executable_name.to_owned(),
            bundle_info_dictionary_version: "6.0".to_owned(),
            bundle_package_type: "APPL".to_owned(),
            icon_file: icon_file.unwrap_or_default(),
            bundle_signature: "????".to_owned(),
            short_version: Version::new(
                bundle_info.version.major,
                bundle_info.version.minor,
                bundle_info.version.patch,
            ),
            environment: [("MallocNanoZone".to_owned(), "0".to_owned())]
                .into_iter()
                .collect(),
            // Main `.app`: `false` avoids Gatekeeper/quarantine oddities when opening local bundles
            // (`open -n …/target/bundle/foo.app`). Helper bundles keep `true`.
            file_quarantine_enabled: is_helper,
            minimum_system_version: "11.0".to_owned(),
            ui_element: if is_helper { Some("1") } else { None },
            bluetooth_always_usage_description: executable_name.to_owned(),
            supports_automatic_graphics_switching: true,
            web_browser_publickey_credential_usage_description: executable_name.to_owned(),
            camera_usage_description: executable_name.to_owned(),
            microphone_usage_description: executable_name.to_owned(),
            main_nib_file: if is_helper {
                None
            } else {
                main_nib_file
            },
            principal_class: if is_helper {
                None
            } else {
                principal_class
            },
            bundle_info,
        }
    }
}

fn serialize_ui_element<S>(
    ui_element: &Option<&'static str>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match ui_element {
        Some(element) => serializer.serialize_str(element),
        None => unreachable!("None is skipped"),
    }
}

fn serialize_version<S>(version: &Version, serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&version.to_string())
}

const EXEC_PATH: &str = "Contents/MacOS";
const FRAMEWORKS_PATH: &str = "Contents/Frameworks";
const RESOURCES_PATH: &str = "Contents/Resources";
const FRAMEWORK: &str = "Chromium Embedded Framework.framework";
const HELPERS: &[&str] = &[
    "Helper (GPU)",
    "Helper (Renderer)",
    "Helper (Plugin)",
    "Helper (Alerts)",
    "Helper",
];

fn create_app_layout(app_path: &Path) -> Result<PathBuf> {
    for path in [EXEC_PATH, RESOURCES_PATH, FRAMEWORKS_PATH] {
        fs::create_dir_all(app_path.join(path))?;
    }
    Ok(app_path.join("Contents"))
}

fn create_app(
    app_path: &Path,
    executable_name: &str,
    is_helper: bool,
    resources_path: Option<&Path>,
    bundle_info: BundleInfo,
    main_nib_file: Option<String>,
    principal_class: Option<String>,
    bin: &Path,
) -> Result<PathBuf> {
    let app_path = app_path.join(executable_name).with_extension("app");
    let contents_path = create_app_layout(&app_path)?;
    let icon_file = resources_path.and_then(|path| {
        let icon_file = format!("{executable_name}.icns");
        if path.join("mac").join(&icon_file).exists() {
            Some(icon_file)
        } else {
            None
        }
    });
    create_info_plist(
        &contents_path,
        executable_name,
        bundle_info,
        is_helper,
        icon_file,
        main_nib_file,
        principal_class,
    )?;
    let executable_path = app_path.join(EXEC_PATH).join(executable_name);
    fs::copy(bin, executable_path)?;
    Ok(app_path)
}

fn create_info_plist(
    contents_path: &Path,
    executable_name: &str,
    bundle_info: BundleInfo,
    is_helper: bool,
    icon_file: Option<String>,
    main_nib_file: Option<String>,
    principal_class: Option<String>,
) -> Result<()> {
    let info_plist = InfoPlist::new(
        executable_name,
        is_helper,
        icon_file,
        bundle_info,
        main_nib_file,
        principal_class,
    );
    plist::to_file_xml(contents_path.join("Info.plist"), &info_plist)?;
    Ok(())
}

fn copy_directory(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_directory(&entry.path(), &dst_path)?;
        } else {
            fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}

fn copy_app_resources(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let mut dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_app_resources(&entry.path(), &dst_path)?;
        } else {
            let entry = entry.path();
            if entry
                .extension()
                .map(|ext| ext == "xib")
                .unwrap_or_default()
            {
                dst_path.set_extension("nib");
                let (Some(dst_path), Some(entry)) = (dst_path.to_str(), entry.to_str()) else {
                    return Err(io::Error::from(io::ErrorKind::NotFound));
                };
                let status = Command::new("xcrun")
                    .args(["ibtool", "--compile", dst_path, entry])
                    .status()?;
                if !status.success() {
                    return Err(io::Error::from(io::ErrorKind::Interrupted));
                }
            } else {
                fs::copy(entry, &dst_path)?;
            }
        }
    }
    Ok(())
}

fn cargo_build(name: &str) -> Result<()> {
    println!("Building {name}...");

    let status = Command::new(super::cargo_path())
        .args(["build", "--bin", name])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::from(io::ErrorKind::Interrupted).into())
    }
}
