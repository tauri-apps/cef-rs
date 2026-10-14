use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Cargo metadata error: {0:?}")]
    Metadata(#[from] cargo_metadata::Error),
    #[error("Missing package metadata for {0}")]
    MissingPackageMetadata(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Deserialize)]
struct PackageMetadata {
    cef: Cef,
}

#[derive(Deserialize)]
struct Cef {
    bundle: CargoBundleMetadata,
}

#[derive(Deserialize)]
struct CargoBundleMetadata {
    #[cfg(target_os = "macos")]
    helper_name: String,
    resources_path: Option<String>,
    /// If set (e.g. `"MainMenu"`), written as `NSMainNibFile` on the main app `Info.plist` only.
    #[serde(default)]
    main_nib_file: Option<String>,
    /// If set (e.g. `"VmuxApplication"`), written as `NSPrincipalClass` on the main app only.
    /// Required for CEF macOS embedding when the app uses an `NSApplication` subclass (`CrApp`).
    #[serde(default)]
    principal_class: Option<String>,
}

pub struct BundleMetadata {
    #[cfg(target_os = "macos")]
    pub helper_name: String,
    pub resources_path: Option<PathBuf>,
    #[cfg(target_os = "macos")]
    pub main_nib_file: Option<String>,
    #[cfg(target_os = "macos")]
    pub principal_class: Option<String>,
}

impl BundleMetadata {
    pub fn parse(executable: &str, metadata: &cargo_metadata::Metadata) -> Option<Self> {
        let package = metadata
            .packages
            .iter()
            .find(|p| p.targets.iter().any(|t| t.name == executable))?;
        let package_metadata =
            serde_json::from_value::<PackageMetadata>(package.metadata.clone()).ok()?;
        let resources_path = package_metadata
            .cef
            .bundle
            .resources_path
            .as_deref()
            .and_then(|resources_path| {
                package
                    .manifest_path
                    .clone()
                    .into_std_path_buf()
                    .parent()
                    .map(|manifest_dir| manifest_dir.join(resources_path))
            });
        Some(Self {
            #[cfg(target_os = "macos")]
            helper_name: package_metadata.cef.bundle.helper_name,
            resources_path,
            #[cfg(target_os = "macos")]
            main_nib_file: package_metadata.cef.bundle.main_nib_file.clone(),
            #[cfg(target_os = "macos")]
            principal_class: package_metadata.cef.bundle.principal_class.clone(),
        })
    }
}

pub struct CargoMetadata(cargo_metadata::Metadata);

impl CargoMetadata {
    pub fn target_directory(&self) -> PathBuf {
        PathBuf::from(&self.0.target_directory)
    }

    pub fn parse_bundle_metadata(&self, executable: &str) -> Result<BundleMetadata> {
        BundleMetadata::parse(executable, &self.0)
            .ok_or_else(|| Error::MissingPackageMetadata(executable.to_owned()))
    }
}

/// Run `cargo metadata` to determine the configuration for the current workspace/package.
pub fn get_cargo_metadata() -> Result<CargoMetadata> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .no_deps()
        .other_options(vec!["--frozen".to_string()])
        .exec()?;
    Ok(CargoMetadata(metadata))
}
