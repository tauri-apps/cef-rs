#![doc = include_str!("../README.md")]

#[macro_use]
extern crate thiserror;

use clap::Parser;
use download_cef::DEFAULT_TARGET;
use std::{fs, io::Read, path::Path, sync::OnceLock};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Missing Parent")]
    MissingParent(std::path::PathBuf),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Bindgen(#[from] bindgen::BindgenError),
    #[error(transparent)]
    Regex(#[from] regex::Error),
    #[error(transparent)]
    Syn(#[from] syn::Error),
    #[error("Parsing bindgen output failed")]
    Parse(#[from] parse_tree::Unrecognized),
    #[error("Missing Path")]
    MissingPath(std::path::PathBuf),
}

pub type Result<T> = std::result::Result<T, Error>;

mod dirs;
mod parse_tree;
mod resources;
mod upgrade;

fn default_version() -> &'static str {
    static DEFAULT_VERSION: OnceLock<String> = OnceLock::new();
    DEFAULT_VERSION
        .get_or_init(|| download_cef::default_version(env!("CARGO_PKG_VERSION")))
        .as_str()
}

fn default_download_url() -> &'static str {
    static DEFAULT_DOWNLOAD_URL: OnceLock<String> = OnceLock::new();
    DEFAULT_DOWNLOAD_URL
        .get_or_init(download_cef::default_download_url)
        .as_str()
}

#[derive(Parser, Debug)]
#[command(about, long_about = None)]
struct Args {
    #[arg(short, long)]
    download: bool,
    #[arg(short, long)]
    bindgen: bool,
    #[arg(short, long, default_value = DEFAULT_TARGET)]
    target: String,
    #[arg(short, long, default_value = default_version())]
    version: String,
    #[arg(short, long, default_value = default_download_url())]
    mirror_url: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let target = args.target.as_str();

    if args.bindgen {
        if args.download {
            let _ = upgrade::download(args.mirror_url.as_str(), target, args.version.as_str());
        }

        upgrade::sys_bindgen(target)?;
    }

    let bindings_file = upgrade::get_target_bindings(target);
    let mut sys_bindings = dirs::get_sys_dir()?;
    sys_bindings.push("src");
    sys_bindings.push("bindings");
    sys_bindings.push(&bindings_file);
    let mut cef_bindings = dirs::get_cef_dir()?;
    cef_bindings.push("src");
    let mut cef_resources = cef_bindings.clone();
    cef_bindings.push("bindings");
    cef_bindings.push(&bindings_file);
    cef_resources.push("resources");
    cef_resources.push(&bindings_file);

    let bindings = parse_tree::generate_bindings(&sys_bindings)?;
    let source = read_bindings(&bindings)?;
    let dest = read_bindings(&cef_bindings).unwrap_or_default();

    if source != dest {
        fs::copy(&bindings, &cef_bindings)?;
        println!("Updated: {}", cef_bindings.display());
    }

    let resources = resources::generate_bindings(&sys_bindings)?;
    let source = read_bindings(&resources)?;
    let dest = read_bindings(&cef_resources).unwrap_or_default();

    if source != dest {
        fs::copy(&resources, &cef_resources)?;
        println!("Updated: {}", cef_resources.display());
    }

    Ok(())
}

fn read_bindings(source_path: &Path) -> crate::Result<String> {
    let mut source_file = fs::File::open(source_path)?;
    let mut updated = String::default();
    source_file.read_to_string(&mut updated)?;
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::parse_tree;
    use std::fs;

    #[test]
    fn generated_wrap_macros_support_external_trait_impls() {
        let test_dir = std::env::temp_dir().join(format!(
            "cef-rs-wrap-macro-generator-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&test_dir).unwrap();
        let source_path = test_dir.join("bindings.rs");
        fs::write(
            &source_path,
            r#"
#![allow(non_camel_case_types)]

pub type size_t = usize;

#[repr(C)]
pub type cef_base_ref_counted_t = _cef_base_ref_counted_t;

#[repr(C)]
pub struct _cef_base_ref_counted_t {
    pub size: size_t,
    pub add_ref: Option<unsafe extern "C" fn(self_: *mut _cef_base_ref_counted_t)>,
    pub release: Option<unsafe extern "C" fn(self_: *mut _cef_base_ref_counted_t) -> ::std::os::raw::c_int>,
    pub has_one_ref: Option<unsafe extern "C" fn(self_: *mut _cef_base_ref_counted_t) -> ::std::os::raw::c_int>,
    pub has_at_least_one_ref: Option<unsafe extern "C" fn(self_: *mut _cef_base_ref_counted_t) -> ::std::os::raw::c_int>,
}

#[repr(C)]
pub struct cef_size_t {
    pub width: ::std::os::raw::c_int,
    pub height: ::std::os::raw::c_int,
}

#[repr(C)]
pub struct _cef_view_delegate_t {
    pub base: _cef_base_ref_counted_t,
    pub get_preferred_size: Option<unsafe extern "C" fn(self_: *mut _cef_view_delegate_t) -> cef_size_t>,
}

#[repr(C)]
pub struct _cef_panel_delegate_t {
    pub base: _cef_view_delegate_t,
}

#[repr(C)]
pub struct _cef_window_delegate_t {
    pub base: _cef_panel_delegate_t,
    pub can_close: Option<unsafe extern "C" fn(self_: *mut _cef_window_delegate_t) -> ::std::os::raw::c_int>,
}
"#,
        )
        .unwrap();

        let generated_path = parse_tree::generate_bindings(&source_path).unwrap();
        let generated = fs::read_to_string(generated_path).unwrap();
        let patterns = [
            "pubtraitImplWindowDelegate:ImplPanelDelegate",
            "pubtraitImplViewDelegate:Clone+Sized+Rc+crate::rc::WrapRcPtr",
            "($vis:visstruct$name:ident;)=>{wrap_window_delegate!{$visstruct$name{}}}",
            "fnget_raw(&self)->*mut_cef_view_delegate_t{self.as_rc_ptr().cast()}",
        ]
        .map(strip_whitespace);

        let bindings = strip_whitespace(&generated);
        for pattern in patterns.iter() {
            assert!(
                bindings.contains(pattern),
                "generated bindings are missing pattern: {pattern}"
            );
        }
    }

    fn strip_whitespace(source: &str) -> String {
        source.chars().filter(|c| !c.is_whitespace()).collect()
    }
}
