#![doc = include_str!("../README.md")]

use clap::Parser;
use download_cef::{CefFile, CefIndex, OsAndArch, DEFAULT_TARGET};
use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};

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
    force: bool,
    #[arg(short, long)]
    save_archive: bool,
    #[arg(short, long, default_value_t = env::var("NIX_CEF_BINARY").is_ok())]
    nix: bool,
    #[arg(short, long, default_value = DEFAULT_TARGET)]
    target: String,
    #[arg(short, long, default_value = default_version())]
    version: String,
    #[arg(short, long, default_value = default_download_url())]
    mirror_url: String,
    #[arg(short, long)]
    archive: Option<String>,
    output: String,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let output = PathBuf::from(args.output);
    let url = args.mirror_url.as_str();

    let parent = PathBuf::from(
        output
            .parent()
            .ok_or_else(|| anyhow::anyhow!("invalid target directory: {}", output.display()))?,
    );

    if fs::exists(&output)? {
        if !args.force {
            return Err(anyhow::anyhow!(
                "target directory already exists: {}",
                output.display()
            ));
        }

        let dir = output
            .file_name()
            .and_then(|dir| dir.to_str())
            .ok_or_else(|| anyhow::anyhow!("invalid target directory: {}", output.display()))?;
        let old_output = parent.join(format!("old_{dir}"));
        fs::rename(&output, &old_output)?;
        println!("Cleaning up: {}", old_output.display());
        fs::remove_dir_all(old_output)?
    }

    let target = args.target.as_str();
    let os_arch = OsAndArch::try_from(target)?;
    let cef_dir = os_arch.to_string();
    let cef_dir = parent.join(&cef_dir);

    if fs::exists(&cef_dir)? {
        let dir = cef_dir
            .file_name()
            .and_then(|dir| dir.to_str())
            .ok_or_else(|| anyhow::anyhow!("invalid target directory: {}", output.display()))?;
        let old_cef_dir = parent.join(format!("old_{dir}"));
        fs::rename(&cef_dir, &old_cef_dir)?;
        println!("Cleaning up: {}", old_cef_dir.display());
        fs::remove_dir_all(old_cef_dir)?
    }

    let (archive, extracted_dir) = match args.archive {
        Some(archive) => {
            let extracted_dir =
                download_cef::extract_target_archive(target, &archive, &parent, true)?;
            let archive = CefFile::try_from(Path::new(&archive))?;
            (archive, extracted_dir)
        }
        None => {
            let cef_version = args.version.as_str();

            if args.nix {
                return Ok(download_cef::install_nix_cef(&cef_version, &output, false)?);
            } else {
                let index = CefIndex::download_from(url)?;
                let platform = index.platform(target)?;
                let version = platform.version(cef_version)?;

                let archive = version.download_archive_with_retry_from(
                    url,
                    &parent,
                    true,
                    Duration::from_secs(15),
                    3,
                )?;
                let extracted_dir =
                    download_cef::extract_target_archive(target, &archive, &parent, true)?;

                if !args.save_archive {
                    println!("Cleaning up: {}", archive.display());
                    fs::remove_file(archive)?;
                }

                let archive = version.minimal()?.clone();
                (archive, extracted_dir)
            }
        }
    };

    if extracted_dir != cef_dir {
        return Err(anyhow::anyhow!(
            "extracted dir {extracted_dir:?} does not match cef_dir {cef_dir:?}",
        ));
    }

    archive.write_archive_json(extracted_dir)?;

    if output != cef_dir {
        println!("Renaming: {}", output.display());
        fs::rename(cef_dir, output)?;
    }

    Ok(())
}
