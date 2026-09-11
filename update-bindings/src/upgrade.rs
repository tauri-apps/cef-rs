use crate::dirs;
use bindgen::callbacks::{DeriveInfo, ParseCallbacks};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
};

const TARGETS: &[&str] = &[
    // macos
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    // windows
    "x86_64-pc-windows-msvc",
    "aarch64-pc-windows-msvc",
    "i686-pc-windows-msvc",
    // linux
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "arm-unknown-linux-gnueabi",
];

// Opaque C handle structs that bindgen 0.73.2 otherwise emits without
// `Copy, Clone`, but wrapper code passes by value when converting borrowed FFI
// parameters. Add entries only for zero-sized opaque handle types.
const OPAQUE_STRING_COPY_HANDLES: &[&str] = &[
    "_cef_string_list_t",
    "_cef_string_map_t",
    "_cef_string_multimap_t",
];

const OPAQUE_X11_COPY_HANDLES: &[&str] = &["_XEvent", "_XDisplay"];

pub fn download(url: &str, target: &str, version: &str) -> PathBuf {
    assert!(TARGETS.contains(&target), "unsupported target {target}");

    let archive =
        download_cef::download_target_archive_from(url, target, version, dirs::get_out_dir(), true)
            .expect("download failed");

    download_cef::extract_target_archive(target, &archive, dirs::get_out_dir(), true)
        .expect("extraction failed")
}

pub fn sys_bindgen(target: &str) -> crate::Result<()> {
    assert!(TARGETS.contains(&target), "unsupported target {target}");
    let (os, arch) = target_to_os_arch(target);
    let cef_path = dirs::get_cef_root(os, arch);
    bindgen(target, &cef_path)
}

pub fn get_target_bindings(target: &str) -> String {
    assert!(TARGETS.contains(&target), "unsupported target {target}");
    format!("{}.rs", target.replace('-', "_"))
}

fn bindgen(target: &str, cef_path: &Path) -> crate::Result<()> {
    let mut sys_bindings = dirs::get_sys_dir()?;
    let mut wrapper = sys_bindings.clone();
    sys_bindings.push("src");
    sys_bindings.push("bindings");
    sys_bindings.push(format!("{}.rs", target.replace('-', "_")));
    wrapper.push("wrapper.h");

    let opaque_copy_handles = opaque_copy_handles(target);
    let matched_opaque_copy_handles = Arc::new(Mutex::new(BTreeSet::new()));
    let mut bindings = bindgen::Builder::default()
        .header(wrapper.display().to_string())
        .default_enum_style(bindgen::EnumVariation::Rust {
            non_exhaustive: true,
        })
        .allowlist_type("cef_.*")
        .allowlist_function("cef_.*")
        .allowlist_item("CEF_API_VERSION(_.+)?")
        .allowlist_item("CEF_VERSION(_.+)?")
        .allowlist_item("CHROME_VERSION(_.+)?")
        .allowlist_item("ID[CRS]_.+")
        .default_macro_constant_type(bindgen::MacroTypeVariation::Signed)
        .bitfield_enum(".*_mask_t")
        .bitfield_enum(".*_flags_t")
        .bitfield_enum("cef_v8_propertyattribute_t")
        .parse_callbacks(Box::new(OpaqueHandleDerives {
            handles: opaque_copy_handles.clone(),
            matched: Arc::clone(&matched_opaque_copy_handles),
        }))
        .clang_args([
            format!("-I{}", cef_path.display()),
            format!("--target={target}"),
        ]);

    if target.contains("windows") {
        bindings = bindings.new_type_alias("HINSTANCE").new_type_alias("HWND");
    } else if target.contains("apple") {
        let sdk_path = Command::new("xcrun")
            .args(["--sdk", "macosx", "--show-sdk-path"])
            .output()
            .unwrap()
            .stdout;

        bindings = bindings.clang_arg(format!(
            "--sysroot={}",
            String::from_utf8_lossy(&sdk_path).trim()
        ));
    }

    let bindings = bindings.generate()?;
    warn_unmatched_opaque_copy_handles(&opaque_copy_handles, &matched_opaque_copy_handles);

    bindings.write_to_file(&sys_bindings)?;
    Ok(())
}

fn opaque_copy_handles(target: &str) -> Vec<&'static str> {
    let mut handles = OPAQUE_STRING_COPY_HANDLES.to_vec();
    if target.contains("linux") {
        handles.extend_from_slice(OPAQUE_X11_COPY_HANDLES);
    }
    handles
}

fn warn_unmatched_opaque_copy_handles(
    handles: &[&'static str],
    matched: &Arc<Mutex<BTreeSet<&'static str>>>,
) {
    let matched = matched.lock().expect("opaque copy handle matches poisoned");
    for handle in handles {
        if !matched.contains(handle) {
            eprintln!("warning: bindgen did not emit opaque handle {handle}; Copy/Clone derives were not added");
        }
    }
}

#[derive(Debug)]
struct OpaqueHandleDerives {
    handles: Vec<&'static str>,
    matched: Arc<Mutex<BTreeSet<&'static str>>>,
}

impl ParseCallbacks for OpaqueHandleDerives {
    fn add_derives(&self, info: &DeriveInfo<'_>) -> Vec<String> {
        if let Some(handle) = self
            .handles
            .iter()
            .copied()
            .find(|handle| *handle == info.name)
        {
            self.matched
                .lock()
                .expect("opaque copy handle matches poisoned")
                .insert(handle);
            // Bindgen de-duplicates derives, so returning Copy/Clone here is safe even when bindgen already derives them.
            vec!["Copy".into(), "Clone".into()]
        } else {
            Vec::new()
        }
    }
}

fn target_to_os_arch(target: &str) -> (&str, &str) {
    match target {
        "aarch64-apple-darwin" => ("macos", "aarch64"),
        "x86_64-apple-darwin" => ("macos", "x86_64"),
        "x86_64-pc-windows-msvc" => ("windows", "x86_64"),
        "aarch64-pc-windows-msvc" => ("windows", "aarch64"),
        "i686-pc-windows-msvc" => ("windows", "x86"),
        "x86_64-unknown-linux-gnu" => ("linux", "x86_64"),
        "aarch64-unknown-linux-gnu" => ("linux", "aarch64"),
        "arm-unknown-linux-gnueabi" => ("linux", "arm"),
        v => panic!("unsupported {v:?}"),
    }
}
