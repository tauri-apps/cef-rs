#![doc = include_str!("../README.md")]

use bzip2::bufread::BzDecoder;
use clap::ValueEnum;
use fs_err::File;
use regex::Regex;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha1_smol::Sha1;
use std::{
    collections::HashMap,
    env,
    fmt::{self, Display},
    io::{self, BufReader, IsTerminal, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    thread,
    time::Duration,
};

#[macro_use]
extern crate thiserror;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Unsupported target triplet: {0}")]
    UnsupportedTarget(String),
    #[error("HTTP request error: {0}")]
    Request(#[from] ureq::Error),
    #[error("Invalid version: {0}")]
    InvalidVersion(#[from] semver::Error),
    #[error("Version not found: {0}")]
    VersionNotFound(String),
    #[error("Missing Content-Length header")]
    MissingContentLength,
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
    #[error("Invalid regex pattern: {0}")]
    InvalidRegexPattern(#[from] regex::Error),
    #[error("Failed to install CEF via Nix: {0}")]
    NixFailed(std::process::ExitStatus),
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

pub fn default_version(version: &str) -> String {
    unwrap_cef_version(version).unwrap_or_else(|_| version.to_string())
}

fn unwrap_cef_version(version: &str) -> Result<String> {
    static VERSIONS: OnceLock<Mutex<HashMap<Version, String>>> = OnceLock::new();
    let mut versions = VERSIONS
        .get_or_init(Default::default)
        .lock()
        .expect("Lock error");
    Ok(versions
        .entry(Version::parse(version)?)
        .or_insert_with_key(|v| {
            if v.build.is_empty() {
                version.to_string()
            } else {
                v.build.to_string()
            }
        })
        .clone())
}

pub fn check_archive_json(version: &str, location: &str) -> Result<()> {
    let expected = Version::parse(&unwrap_cef_version(version)?)?;

    static PATTERN: OnceLock<core::result::Result<Regex, regex::Error>> = OnceLock::new();
    let pattern = PATTERN
        .get_or_init(|| Regex::new(r"^cef_binary_([^+]+)(:?\+.+)?$"))
        .as_ref()
        .map_err(Clone::clone)?;
    let archive_json: CefFile = serde_json::from_reader(File::open(archive_json_path(location))?)?;
    let archive_version = pattern.replace(&archive_json.name, "$1");
    let archive = Version::parse(&archive_version)?;

    if archive <= expected {
        Ok(())
    } else {
        Err(Error::VersionMismatch {
            location: location.to_string(),
            expected: expected.to_string(),
            archive: archive.to_string(),
        })
    }
}

fn archive_json_path<P>(location: P) -> PathBuf
where
    P: AsRef<Path>,
{
    let location = location.as_ref().join("archive.json");
    std::path::absolute(&location).unwrap_or(location)
}

pub const DEFAULT_CDN_URL: &str = "https://cef-builds.spotifycdn.com";

pub fn default_download_url() -> String {
    env::var("CEF_DOWNLOAD_URL").unwrap_or(DEFAULT_CDN_URL.to_owned())
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Stable,
    Beta,
}

impl Display for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Channel::Stable => write!(f, "stable"),
            Channel::Beta => write!(f, "beta"),
        }
    }
}

#[derive(Deserialize, Serialize, Default)]
pub struct CefIndex {
    pub macosarm64: CefPlatform,
    pub macosx64: CefPlatform,
    pub windows64: CefPlatform,
    pub windowsarm64: CefPlatform,
    pub windows32: CefPlatform,
    pub linux64: CefPlatform,
    pub linuxarm64: CefPlatform,
    pub linuxarm: CefPlatform,
}

impl CefIndex {
    pub fn download() -> Result<Self> {
        Self::download_from(DEFAULT_CDN_URL)
    }

    pub fn download_from(url: &str) -> Result<Self> {
        Ok(ureq::get(&format!("{url}/index.json"))
            .call()?
            .into_body()
            .read_json()?)
    }

    pub fn platform(&self, target: &str) -> Result<&CefPlatform> {
        match target {
            "aarch64-apple-darwin" => Ok(&self.macosarm64),
            "x86_64-apple-darwin" => Ok(&self.macosx64),
            "x86_64-pc-windows-msvc" => Ok(&self.windows64),
            "aarch64-pc-windows-msvc" => Ok(&self.windowsarm64),
            "i686-pc-windows-msvc" => Ok(&self.windows32),
            "x86_64-unknown-linux-gnu" => Ok(&self.linux64),
            "aarch64-unknown-linux-gnu" => Ok(&self.linuxarm64),
            "arm-unknown-linux-gnueabi" => Ok(&self.linuxarm),
            v => Err(Error::UnsupportedTarget(v.to_string())),
        }
    }
}

#[derive(Deserialize, Serialize, Default)]
pub struct CefPlatform {
    pub versions: Vec<CefVersion>,
}

impl CefPlatform {
    pub fn version(&self, cef_version: &str) -> Result<&CefVersion> {
        let version_prefix = format!("{cef_version}+");
        self.versions
            .iter()
            .find(|v| v.cef_version.starts_with(&version_prefix))
            .ok_or_else(|| Error::VersionNotFound(cef_version.to_string()))
    }

    pub fn latest(&self, channel: Channel) -> Result<&CefVersion> {
        static PATTERN: OnceLock<core::result::Result<Regex, regex::Error>> = OnceLock::new();
        let pattern = PATTERN
            .get_or_init(|| Regex::new(r"^([^+]+)(:?\+.+)?$"))
            .as_ref()
            .map_err(Clone::clone)?;

        self.versions
            .iter()
            .filter_map(|value| {
                if value.channel == channel {
                    let key = Version::parse(&pattern.replace(&value.cef_version, "$1")).ok()?;
                    Some((key, value))
                } else {
                    None
                }
            })
            .max_by(|(a, _), (b, _)| a.cmp(b))
            .map(|(_, v)| v)
            .ok_or_else(|| Error::VersionNotFound("latest".to_string()))
    }
}

#[derive(Deserialize, Serialize)]
pub struct CefVersion {
    pub channel: Channel,
    pub cef_version: String,
    pub files: Vec<CefFile>,
}

impl CefVersion {
    pub fn download_archive<P>(&self, location: P, show_progress: bool) -> Result<PathBuf>
    where
        P: AsRef<Path>,
    {
        self.download_archive_from(DEFAULT_CDN_URL, location, show_progress)
    }

    pub fn download_archive_from<P>(
        &self,
        url: &str,
        location: P,
        show_progress: bool,
    ) -> Result<PathBuf>
    where
        P: AsRef<Path>,
    {
        let file = self.minimal()?;
        let (file, sha) = (file.name.as_str(), file.sha1.as_str());

        fs_err::create_dir_all(&location)?;
        let download_file = location.as_ref().join(file);

        if download_file.exists() {
            if calculate_file_sha1(&download_file) == sha {
                if show_progress {
                    println!("Verified archive: {}", download_file.display());
                }
                return Ok(download_file);
            }

            if show_progress {
                println!("Cleaning corrupted archive: {}", download_file.display());
            }
            let corrupted_file = location.as_ref().join(format!("corrupted_{file}"));
            fs_err::rename(&download_file, &corrupted_file)?;
            fs_err::remove_file(&corrupted_file)?;
        }

        let cef_url = format!("{url}/{file}");
        if show_progress {
            println!("Using archive url: {cef_url}");
        }

        let mut file = File::create(&download_file)?;

        let resp = ureq::get(&cef_url).call()?;
        let expected = resp
            .headers()
            .get("Content-Length")
            .ok_or(Error::MissingContentLength)?;
        let expected = expected.to_str()?;
        let expected = expected
            .parse::<u64>()
            .map_err(|_| Error::InvalidContentLength(expected.to_owned()))?;

        let downloaded = if show_progress && io::stdout().is_terminal() {
            const DOWNLOAD_TEMPLATE: &str = "{msg} {spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})";

            let bar = indicatif::ProgressBar::new(expected);
            bar.set_style(
                indicatif::ProgressStyle::with_template(DOWNLOAD_TEMPLATE)
                    .expect("invalid template")
                    .progress_chars("##-"),
            );
            bar.set_message("Downloading");
            std::io::copy(
                &mut bar.wrap_read(resp.into_body().into_reader()),
                &mut file,
            )
        } else {
            let mut reader = resp.into_body().into_reader();
            std::io::copy(&mut reader, &mut file)
        }?;

        if downloaded != expected {
            return Err(Error::UnexpectedFileSize {
                downloaded,
                expected,
            });
        }

        if show_progress {
            println!("Verifying SHA1 hash: {sha}...");
        }
        if calculate_file_sha1(&download_file) != sha {
            return Err(Error::CorruptedFile(download_file.display().to_string()));
        }

        if show_progress {
            println!("Downloaded archive: {}", download_file.display());
        }
        Ok(download_file)
    }

    pub fn download_archive_with_retry<P>(
        &self,
        location: P,
        show_progress: bool,
        retry_delay: Duration,
        max_retries: u32,
    ) -> Result<PathBuf>
    where
        P: AsRef<Path>,
    {
        self.download_archive_with_retry_from(
            DEFAULT_CDN_URL,
            location,
            show_progress,
            retry_delay,
            max_retries,
        )
    }

    pub fn download_archive_with_retry_from<P>(
        &self,
        url: &str,
        location: P,
        show_progress: bool,
        retry_delay: Duration,
        max_retries: u32,
    ) -> Result<PathBuf>
    where
        P: AsRef<Path>,
    {
        let mut result = self.download_archive_from(url, &location, show_progress);

        let mut retry = 0;
        while let Err(Error::Io(_)) = &result {
            if retry >= max_retries {
                break;
            }

            retry += 1;
            thread::sleep(retry_delay * retry);

            result = self.download_archive_from(url, &location, show_progress);
        }

        result
    }

    pub fn minimal(&self) -> Result<&CefFile> {
        self.files
            .iter()
            .find(|f| f.file_type == "minimal")
            .ok_or_else(|| Error::VersionNotFound(self.cef_version.clone()))
    }

    pub fn write_archive_json<P>(&self, location: P) -> Result<()>
    where
        P: AsRef<Path>,
    {
        self.minimal()?.write_archive_json(location)
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct CefFile {
    #[serde(rename = "type")]
    pub file_type: String,
    pub name: String,
    pub sha1: String,
}

impl CefFile {
    pub fn write_archive_json<P>(&self, location: P) -> Result<()>
    where
        P: AsRef<Path>,
    {
        let archive_version = serde_json::to_string_pretty(self)?;
        let mut archive_json = File::create(archive_json_path(location))?;
        archive_json.write_all(archive_version.as_bytes())?;
        Ok(())
    }
}

impl TryFrom<&Path> for CefFile {
    type Error = Error;

    fn try_from(location: &Path) -> Result<Self> {
        let file_type = "minimal".to_string();
        let name = location
            .file_name()
            .map(|f| f.display().to_string())
            .ok_or_else(|| Error::InvalidArchiveFile(location.display().to_string()))?;
        let sha1 = calculate_file_sha1(location);
        Ok(Self {
            file_type,
            name,
            sha1,
        })
    }
}

pub fn download_target_archive<P>(
    target: &str,
    cef_version: &str,
    location: P,
    show_progress: bool,
) -> Result<PathBuf>
where
    P: AsRef<Path>,
{
    download_target_archive_from(
        DEFAULT_CDN_URL,
        target,
        cef_version,
        location,
        show_progress,
    )
}

pub fn download_target_archive_from<P>(
    url: &str,
    target: &str,
    cef_version: &str,
    location: P,
    show_progress: bool,
) -> Result<PathBuf>
where
    P: AsRef<Path>,
{
    if show_progress {
        println!("Downloading CEF archive for {target}...");
    }

    let index = CefIndex::download_from(url)?;
    let platform = index.platform(target)?;
    let version = platform.version(cef_version)?;

    version.download_archive_with_retry_from(
        url,
        location,
        show_progress,
        Duration::from_secs(15),
        3,
    )
}

pub fn extract_target_archive<P, Q>(
    target: &str,
    archive: P,
    location: Q,
    show_progress: bool,
) -> Result<PathBuf>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    if show_progress {
        println!("Extracting archive: {}", archive.as_ref().display());
    }
    let decoder = BzDecoder::new(BufReader::new(File::open(archive.as_ref())?));
    tar::Archive::new(decoder).unpack(&location)?;

    let extracted_dir = archive
        .as_ref()
        .file_name()
        .unwrap() // Safe here due to File::open check above
        .display()
        .to_string();
    let extracted_dir = extracted_dir
        .strip_suffix(".tar.bz2")
        .map(PathBuf::from)
        .ok_or(Error::InvalidArchiveFile(extracted_dir))?;
    let extracted_dir = location.as_ref().join(extracted_dir);

    let os_and_arch = OsAndArch::try_from(target)?;
    let OsAndArch { os, arch } = os_and_arch;
    let cef_dir = os_and_arch.to_string();
    let cef_dir = location.as_ref().join(cef_dir);

    if cef_dir.exists() {
        let old_dir = location.as_ref().join(format!("old_{os}_{arch}"));
        if show_progress {
            println!("Cleaning up: {}", old_dir.display());
        }
        fs_err::rename(&cef_dir, &old_dir)?;
        fs_err::remove_dir_all(old_dir)?;
    }
    const RELEASE_DIR: &str = "Release";
    fs_err::rename(extracted_dir.join(RELEASE_DIR), &cef_dir)?;

    if os != "macos" {
        let resources = extracted_dir.join("Resources");

        for entry in fs_err::read_dir(&resources)? {
            let entry = entry?;
            fs_err::rename(entry.path(), cef_dir.join(entry.file_name()))?;
        }
    }

    const CMAKE_LISTS_TXT: &str = "CMakeLists.txt";
    fs_err::rename(
        extracted_dir.join(CMAKE_LISTS_TXT),
        cef_dir.join(CMAKE_LISTS_TXT),
    )?;
    const CMAKE_DIR: &str = "cmake";
    fs_err::rename(extracted_dir.join(CMAKE_DIR), cef_dir.join(CMAKE_DIR))?;
    const INCLUDE_DIR: &str = "include";
    fs_err::rename(extracted_dir.join(INCLUDE_DIR), cef_dir.join(INCLUDE_DIR))?;
    const LIBCEF_DLL_DIR: &str = "libcef_dll";
    fs_err::rename(
        extracted_dir.join(LIBCEF_DLL_DIR),
        cef_dir.join(LIBCEF_DLL_DIR),
    )?;
    const CREDITS_HTML: &str = "CREDITS.html";
    fs_err::rename(extracted_dir.join(CREDITS_HTML), cef_dir.join(CREDITS_HTML))?;

    if show_progress {
        println!("Moved contents to: {}", cef_dir.display());
    }

    // Cleanup whatever is left in the extracted directory.
    let old_dir = extracted_dir
        .parent()
        .map(|parent| parent.join(format!("extracted_{os}_{arch}")))
        .ok_or_else(|| Error::InvalidArchiveFile(extracted_dir.display().to_string()))?;
    if show_progress {
        println!("Cleaning up: {}", old_dir.display());
    }
    fs_err::rename(&extracted_dir, &old_dir)?;
    fs_err::remove_dir_all(old_dir)?;

    Ok(cef_dir)
}

/// Installs CEF via Nix and `nixpkgs` into the specified location.
pub fn install_nix_cef<P>(cef_version: &str, location: P, show_progress: bool) -> Result<()>
where
    P: AsRef<Path>,
{
    use std::process::{Command, Stdio};

    let cef_version = format!(r#""{cef_version}""#);
    let nix_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("nix")
        .display()
        .to_string();
    let out_link = location.as_ref().display().to_string();
    let output = Command::new("nix-build")
        .args([
            &nix_dir,
            "--arg",
            "version",
            &cef_version,
            "--out-link",
            &out_link,
        ])
        .stdin(Stdio::null())
        .stdout(if show_progress {
            Stdio::inherit()
        } else {
            Stdio::null()
        })
        .stderr(Stdio::inherit())
        .output()?;

    if !output.status.success() {
        return Err(Error::NixFailed(output.status));
    }

    Ok(())
}

fn calculate_file_sha1(path: &Path) -> String {
    use std::io::Read;
    let mut file = BufReader::new(File::open(path).unwrap());
    let mut sha1 = Sha1::new();
    let mut buffer = [0; 8192];

    loop {
        let count = file.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        sha1.update(&buffer[..count]);
    }

    sha1.digest().to_string()
}

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
