#[cfg(target_os = "macos")]
mod mac {
    use serde::Serialize;
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    #[derive(Serialize)]
    struct InfoPlist {
        #[serde(rename = "CFBundleDevelopmentRegion")]
        cf_bundle_development_region: String,
        #[serde(rename = "CFBundleDisplayName")]
        cf_bundle_display_name: String,
        #[serde(rename = "CFBundleExecutable")]
        cf_bundle_executable: String,
        #[serde(rename = "CFBundleIdentifier")]
        cf_bundle_identifier: String,
        #[serde(rename = "CFBundleInfoDictionaryVersion")]
        cf_bundle_info_dictionary_version: String,
        #[serde(rename = "CFBundleName")]
        cf_bundle_name: String,
        #[serde(rename = "CFBundlePackageType")]
        cf_bundle_package_type: String,
        #[serde(rename = "CFBundleSignature")]
        cf_bundle_signature: String,
        #[serde(rename = "CFBundleVersion")]
        cf_bundle_version: String,
        #[serde(rename = "CFBundleShortVersionString")]
        cf_bundle_short_version_string: String,
        #[serde(rename = "LSEnvironment")]
        ls_environment: HashMap<String, String>,
        #[serde(rename = "LSFileQuarantineEnabled")]
        ls_file_quarantine_enabled: bool,
        #[serde(rename = "LSMinimumSystemVersion")]
        ls_minimum_system_version: String,
        #[serde(rename = "LSUIElement")]
        ls_ui_element: String,
        #[serde(rename = "NSBluetoothAlwaysUsageDescription")]
        ns_bluetooth_always_usage_description: String,
        #[serde(rename = "NSSupportsAutomaticGraphicsSwitching")]
        ns_supports_automatic_graphics_switching: bool,
        #[serde(rename = "NSWebBrowserPublicKeyCredentialUsageDescription")]
        ns_web_browser_publickey_credential_usage_description: String,
        #[serde(rename = "NSCameraUsageDescription")]
        ns_camera_usage_description: String,
        #[serde(rename = "NSMicrophoneUsageDescription")]
        ns_microphone_usage_description: String,
    }

    const EXEC_PATH: &str = "Contents/MacOS";
    const FRAMEWORKS_PATH: &str = "Contents/Frameworks";
    const RESOURCES_PATH: &str = "Contents/Resources";
    const FRAMEWORK: &str = "Chromium Embedded Framework.framework";
    const HELPERS: &[&str] = &[
        "cefsimple Helper (GPU)",
        "cefsimple Helper (Renderer)",
        "cefsimple Helper (Plugin)",
        "cefsimple Helper (Alerts)",
        "cefsimple Helper",
    ];

    fn create_app_layout(app_path: &Path) -> PathBuf {
        [EXEC_PATH, RESOURCES_PATH, FRAMEWORKS_PATH]
            .iter()
            .for_each(|p| fs::create_dir_all(app_path.join(p)).unwrap());
        app_path.join("Contents")
    }

    fn create_app(app_path: &Path, exec_name: &str, bin: &Path) -> PathBuf {
        let app_path = app_path.join(exec_name).with_extension("app");
        let contents_path = create_app_layout(&app_path);
        create_info_plist(&contents_path, exec_name).unwrap();
        fs::copy(bin, app_path.join(EXEC_PATH).join(exec_name)).unwrap();
        app_path
    }

    // See https://bitbucket.org/chromiumembedded/cef/wiki/GeneralUsage.md#markdown-header-macos
    fn bundle(app_path: &Path) {
        let example_path = PathBuf::from(app_path);
        let main_app_path = create_app(app_path, "cefsimple", &example_path.join("cefsimple"));
        let cef_path = cef_dll_sys::get_cef_dir().unwrap();
        let to = main_app_path.join(FRAMEWORKS_PATH).join(FRAMEWORK);
        if to.exists() {
            fs::remove_dir_all(&to).unwrap();
        }
        copy_directory(&cef_path.join(FRAMEWORK), &to);
        HELPERS.iter().for_each(|helper| {
            create_app(
                &main_app_path.join(FRAMEWORKS_PATH),
                helper,
                &example_path.join("cefsimple_helper"),
            );
        });
    }

    fn create_info_plist(
        contents_path: &Path,
        exec_name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let info_plist = InfoPlist {
            cf_bundle_development_region: "en".to_string(),
            cf_bundle_display_name: exec_name.to_string(),
            cf_bundle_executable: exec_name.to_string(),
            cf_bundle_identifier: "org.cef-rs.cefsimple.helper".to_string(),
            cf_bundle_info_dictionary_version: "6.0".to_string(),
            cf_bundle_name: "cef-rs".to_string(),
            cf_bundle_package_type: "APPL".to_string(),
            cf_bundle_signature: "????".to_string(),
            cf_bundle_version: "1.0.0".to_string(),
            cf_bundle_short_version_string: "1.0".to_string(),
            ls_environment: [("MallocNanoZone".to_string(), "0".to_string())]
                .iter()
                .cloned()
                .collect(),
            ls_file_quarantine_enabled: true,
            ls_minimum_system_version: "11.0".to_string(),
            ls_ui_element: "1".to_string(),
            ns_bluetooth_always_usage_description: exec_name.to_string(),
            ns_supports_automatic_graphics_switching: true,
            ns_web_browser_publickey_credential_usage_description: exec_name.to_string(),
            ns_camera_usage_description: exec_name.to_string(),
            ns_microphone_usage_description: exec_name.to_string(),
        };

        plist::to_file_xml(contents_path.join("Info.plist"), &info_plist)?;
        Ok(())
    }

    fn copy_directory(src: &Path, dst: &Path) {
        fs::create_dir_all(dst).unwrap();
        for entry in fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let dst_path = dst.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_directory(&entry.path(), &dst_path);
            } else {
                fs::copy(&entry.path(), &dst_path).unwrap();
            }
        }
    }

    fn run_command(args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
        let status = Command::new("cargo")
            .args(args)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()?;

        if !status.success() {
            std::process::exit(1);
        }
        Ok(())
    }

    pub fn main() -> Result<(), Box<dyn std::error::Error>> {
        let app_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/debug/examples");
        run_command(&["build", "--example", "cefsimple"])?;
        run_command(&["build", "--example", "cefsimple_helper"])?;
        bundle(&app_path);
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    mac::main()
}

#[cfg(not(target_os = "macos"))]
fn main() {}
