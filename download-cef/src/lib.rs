#![doc = include_str!("../README.md")]

use std::fmt::{self, Display};

#[cfg(feature = "download")]
mod download;
#[cfg(feature = "download")]
pub use download::*;

#[macro_use]
extern crate thiserror;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Unsupported target triplet: {0}")]
    UnsupportedTarget(String),
    #[cfg(feature = "download")]
    #[error("HTTP request error: {0}")]
    Request(#[from] ureq::Error),
    #[cfg(feature = "download")]
    #[error("Invalid version: {0}")]
    InvalidVersion(#[from] semver::Error),
    #[error("Version not found: {0}")]
    VersionNotFound(String),
    #[error("Missing Content-Length header")]
    MissingContentLength,
    #[cfg(feature = "download")]
    #[error("Opaque Content-Length header: {0}")]
    OpaqueContentLength(#[from] ureq::http::header::ToStrError),
    #[error("Invalid Content-Length header: {0}")]
    InvalidContentLength(String),
    #[error("File I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Unexpected file size: downloaded {downloaded} expected {expected}")]
    UnexpectedFileSize { downloaded: u64, expected: u64 },
    #[error("Bad SHA1 file hash: {0}")]
    CorruptedFile(String),
    #[error("Invalid archive file path: {0}")]
    InvalidArchiveFile(String),
    #[cfg(feature = "download")]
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error(
        "Undexpected archive version: location: {location} archive {archive} expected {expected}"
    )]
    VersionMismatch {
        location: String,
        archive: String,
        expected: String,
    },
    #[cfg(feature = "download")]
    #[error("Invalid regex pattern: {0}")]
    InvalidRegexPattern(#[from] regex::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

pub const LINUX_TARGETS: &[&str] = &[
    "x86_64-unknown-linux-gnu",
    "arm-unknown-linux-gnueabi",
    "aarch64-unknown-linux-gnu",
];

pub const MACOS_TARGETS: &[&str] = &["aarch64-apple-darwin", "x86_64-apple-darwin"];

pub const WINDOWS_TARGETS: &[&str] = &[
    "x86_64-pc-windows-msvc",
    "aarch64-pc-windows-msvc",
    "i686-pc-windows-msvc",
];

pub struct OsAndArch {
    pub os: &'static str,
    pub arch: &'static str,
}

impl Display for OsAndArch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let os = self.os;
        let arch = self.arch;
        write!(f, "cef_{os}_{arch}")
    }
}

impl TryFrom<&str> for OsAndArch {
    type Error = Error;

    fn try_from(target: &str) -> Result<Self> {
        match target {
            "aarch64-apple-darwin" => Ok(OsAndArch {
                os: "macos",
                arch: "aarch64",
            }),
            "x86_64-apple-darwin" => Ok(OsAndArch {
                os: "macos",
                arch: "x86_64",
            }),
            "x86_64-pc-windows-msvc" => Ok(OsAndArch {
                os: "windows",
                arch: "x86_64",
            }),
            "aarch64-pc-windows-msvc" => Ok(OsAndArch {
                os: "windows",
                arch: "aarch64",
            }),
            "i686-pc-windows-msvc" => Ok(OsAndArch {
                os: "windows",
                arch: "x86",
            }),
            "x86_64-unknown-linux-gnu" => Ok(OsAndArch {
                os: "linux",
                arch: "x86_64",
            }),
            "aarch64-unknown-linux-gnu" => Ok(OsAndArch {
                os: "linux",
                arch: "aarch64",
            }),
            "arm-unknown-linux-gnueabi" => Ok(OsAndArch {
                os: "linux",
                arch: "arm",
            }),
            v => Err(Error::UnsupportedTarget(v.to_string())),
        }
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub const DEFAULT_TARGET: &str = "x86_64-unknown-linux-gnu";
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub const DEFAULT_TARGET: &str = "aarch64-unknown-linux-gnu";
#[cfg(all(target_os = "linux", target_arch = "arm"))]
pub const DEFAULT_TARGET: &str = "arm-unknown-linux-gnueabi";

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
pub const DEFAULT_TARGET: &str = "x86_64-pc-windows-msvc";
#[cfg(all(target_os = "windows", target_arch = "x86"))]
pub const DEFAULT_TARGET: &str = "i686-pc-windows-msvc";
#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
pub const DEFAULT_TARGET: &str = "aarch64-pc-windows-msvc";

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
pub const DEFAULT_TARGET: &str = "x86_64-apple-darwin";
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub const DEFAULT_TARGET: &str = "aarch64-apple-darwin";
