#[cfg(not(feature = "dox"))]
fn main() -> anyhow::Result<()> {
    use download_cef::OsAndArch;
    use std::{
        env, fs,
        path::{Path, PathBuf},
    };

    println!("cargo::rerun-if-changed=build.rs");

    let target = env::var("TARGET")?;
    let os_arch = OsAndArch::try_from(target.as_str())?;

    println!("cargo::rerun-if-env-changed=FLATPAK");
    println!("cargo::rerun-if-env-changed=NIX_CEF_BINARY");
    println!("cargo::rerun-if-env-changed=CEF_PATH");
    let package_version = env::var("CARGO_PKG_VERSION")?;
    let cef_version = download_cef::default_version(&package_version);

    let check_archive = |path: &Path| -> anyhow::Result<()> {
        download_cef::check_archive_json(&package_version, &path.to_string_lossy())?;
        Ok(())
    };

    let resolve_cef_dir = |location: &Path| -> anyhow::Result<PathBuf> {
        let cef_dir = location.join(os_arch.to_string());

        if !fs::exists(&cef_dir)? {
            if env::var("NIX_CEF_BINARY").is_ok() {
                download_cef::install_nix_cef(&cef_version, &cef_dir, false)?;
            } else {
                use download_cef::CefIndex;

                let download_url = download_cef::default_download_url();
                let index = CefIndex::download_from(&download_url)?;
                let platform = index.platform(&target)?;
                let version = platform.version(&cef_version)?;

                let archive = version.download_archive_from(&download_url, location, false)?;
                let extracted_dir =
                    download_cef::extract_target_archive(&target, &archive, location, false)?;
                let extracted_dir_canonical = fs::canonicalize(&extracted_dir)?;
                let cef_dir_canonical = fs::canonicalize(&cef_dir)?;
                if extracted_dir_canonical != cef_dir_canonical {
                    return Err(anyhow::anyhow!(
                        "extracted dir {extracted_dir_canonical:?} does not match cef_dir {cef_dir_canonical:?}",
                    ));
                }

                version.write_archive_json(extracted_dir)?;
            }
        }

        Ok(cef_dir)
    };

    let resolve_from_versioned = |configured_path: &Path| -> anyhow::Result<PathBuf> {
        let versioned_location = configured_path.join(&cef_version);
        let resolved = resolve_cef_dir(&versioned_location)?;
        println!(
            "Using versioned CEF path from environment: {}",
            resolved.display()
        );
        check_archive(&resolved)?;
        Ok(resolved)
    };

    let download_to_versioned = |configured_path: &Path, reason: &str| -> anyhow::Result<PathBuf> {
        let versioned_location = configured_path.join(&cef_version);
        println!(
            "{reason}, downloading archive to: {}",
            versioned_location.display()
        );
        let resolved = resolve_cef_dir(&versioned_location)?;
        println!("Using downloaded CEF path: {}", resolved.display());
        Ok(resolved)
    };

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    let cef_dir = if env::var("FLATPAK").is_ok() {
        let cef_path = String::from("/usr/lib");
        println!("Using CEF path from FLATPAK: {cef_path}");
        let cef_path = PathBuf::from(cef_path);
        check_archive(&cef_path)?;
        cef_path
    } else if let Ok(cef_path) = env::var("CEF_PATH") {
        let configured_path = PathBuf::from(cef_path);
        if fs::exists(&configured_path)? {
            let versioned_location = configured_path.join(&cef_version);
            if fs::exists(&versioned_location)? {
                resolve_from_versioned(&configured_path)?
            } else {
                println!(
                    "Using CEF path from environment: {}",
                    configured_path.display()
                );
                match check_archive(&configured_path) {
                    Ok(()) => configured_path,
                    Err(error) => download_to_versioned(
                        &configured_path,
                        &format!("CEF_PATH is invalid ({error})"),
                    )?,
                }
            }
        } else {
            download_to_versioned(&configured_path, "CEF_PATH does not exist")?
        }
    } else {
        resolve_cef_dir(&out_dir)?
    };

    // TODO: far from ideal, but there's no other way to get the target dir, see <https://github.com/rust-lang/cargo/issues/9661>
    let target_dir = out_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    let cef_dir_str = cef_dir.to_string_lossy().into_owned();

    // Re-run when the resolved CEF directory changes/deletes.
    println!("cargo::rerun-if-changed={cef_dir_str}");

    println!("cargo::metadata=CEF_DIR={cef_dir_str}");
    println!("cargo::rustc-link-search=native={cef_dir_str}");

    // Compile the wrapper against an explicit API version instead of the
    // experimental (unversioned) API that CEF selects by default, which its
    // own headers call "not back/forward compatible with different CEF
    // versions". It is the version the crate declares at run time through
    // `cef_api_hash(CEF_API_VERSION_LAST)`, so the two now agree; without it
    // the macOS loader in `libcef_dll_dylib.cc` also resolves experimental
    // entry points, and loading any libcef but this exact build fails on the
    // first one missing.
    let api_version = cef_api_version_last(&cef_dir)?;
    println!("cargo::metadata=CEF_API_VERSION={api_version}");

    let mut cef_dll_wrapper = cmake::Config::new(&cef_dir);
    cef_dll_wrapper
        .generator("Ninja")
        .profile("RelWithDebInfo")
        .build_target("libcef_dll_wrapper")
        // Seeds the list CEF's cmake appends its own defines to and applies
        // to the target; CMAKE_CXX_FLAGS would not survive, cef_variables
        // clears it for the Ninja generator on Windows.
        .define(
            "CEF_COMPILER_DEFINES",
            format!("CEF_API_VERSION={api_version}"),
        );

    let project_arch = match os_arch.arch {
        "aarch64" => "arm64",
        arch => arch,
    };

    let sandbox = if cfg!(feature = "sandbox") {
        "ON"
    } else {
        "OFF"
    };

    match os_arch.os {
        "linux" => {
            // On Windows and Linux the cef files usually have to be next to the main binary.
            // On macOS it's more complicated so we'll leave it to tools like tauri-cli for now.
            copy_cef_runtime_files(&cef_dir, target_dir)?;

            println!("cargo::rustc-link-lib=dylib=cef");
        }
        "windows" => {
            // On Windows and Linux the cef files usually have to be next to the main binary.
            // On macOS it's more complicated so we'll leave it to tools like tauri-cli for now.
            copy_cef_runtime_files(&cef_dir, target_dir)?;

            // Windows SDK import libraries used by the wrapper. These used to be merged into
            // libcef_dll_wrapper.lib through CMAKE_STATIC_LINKER_FLAGS, but llvm-lib (used when
            // cross-compiling with cargo-xwin) can't read the XFG hash map member of current SDK
            // import libraries, so pass them to the final link through cargo instead.
            let sdk_libs = [
                "comctl32",
                "delayimp",
                "mincore",
                "powrprof",
                "propsys",
                "runtimeobject",
                "setupapi",
                "shcore",
                "shell32",
                "shlwapi",
                "user32",
                "version",
                "wbemuuid",
                "winmm",
            ];
            for lib in sdk_libs {
                println!("cargo::rustc-link-lib=dylib={lib}");
            }

            cef_dll_wrapper
                .define("CMAKE_MSVC_RUNTIME_LIBRARY", "MultiThreaded")
                .define("CMAKE_OBJECT_PATH_MAX", "500")
                .define("PROJECT_ARCH", project_arch)
                .define("USE_SANDBOX", sandbox);
            configure_clang_cl(&mut cef_dll_wrapper, &cef_dir, &out_dir)?;

            let build_dir = cef_dll_wrapper.build().to_string_lossy().into_owned();

            println!("cargo::rustc-link-search=native={build_dir}/build/libcef_dll_wrapper");
            println!("cargo::rustc-link-lib=static=libcef_dll_wrapper");

            println!("cargo::rustc-link-lib=dylib=libcef");
        }
        "macos" => {
            println!("cargo::rustc-link-lib=framework=AppKit");

            let build_dir = cef_dll_wrapper
                .no_default_flags(true)
                .define("PROJECT_ARCH", project_arch)
                .define("USE_SANDBOX", sandbox)
                .build()
                .to_string_lossy()
                .into_owned();
            println!("cargo::rustc-link-search=native={build_dir}/build/libcef_dll_wrapper");
            println!("cargo::rustc-link-lib=static=cef_dll_wrapper");
        }
        os => unimplemented!("unknown target {os}"),
    }

    Ok(())
}

/// Cross-compiling to Windows from another host (e.g. with `cargo xwin`) uses `clang-cl`, which
/// needs a few adjustments to CEF's MSVC-oriented CMake configuration. This is a no-op when the
/// C++ compiler for the target isn't `clang-cl`.
#[cfg(not(feature = "dox"))]
fn configure_clang_cl(
    config: &mut cmake::Config,
    cef_dir: &std::path::Path,
    out_dir: &std::path::Path,
) -> anyhow::Result<()> {
    use std::{fs, path::PathBuf};

    // Only the flags from the CXXFLAGS environment (e.g. cargo-xwin's `--target` and `/imsvc`
    // include paths) are needed here; the cmake crate takes care of everything else.
    let compiler = cc::Build::new()
        .cpp(true)
        .no_default_flags(true)
        .try_get_compiler()?;
    if !compiler.is_like_clang_cl() {
        return Ok(());
    }

    // CMake de-duplicates compile options, which would drop the repeated `/imsvc` of options that
    // take their value as a separate argument (`/imsvc <dir>`), so join those into one token.
    let mut flags: Vec<String> = Vec::new();
    let mut args = compiler
        .args()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned());
    while let Some(arg) = args.next() {
        let flag = match arg.as_str() {
            "/imsvc" | "-imsvc" | "/I" | "-I" | "-isystem" | "/external:I" => match args.next() {
                Some(value) => format!("{arg}{value}"),
                None => arg,
            },
            _ => arg,
        };
        flags.push(flag);
    }

    // clang-cl reports diagnostics that MSVC doesn't (`/MP` is unsupported, `/W4` enables extra
    // warnings), which CEF's `/WX` would turn into errors.
    for flag in [
        "-Wno-unused-command-line-argument",
        "-Wno-missing-field-initializers",
        "-Wno-undefined-var-template",
        "/WX-",
    ] {
        if !flags.iter().any(|existing| existing == flag) {
            flags.push(flag.to_owned());
        }
    }

    // The Windows SDK relies on case-insensitive file names, which doesn't hold on other hosts:
    // e.g. the wrapper includes `Softpub.h` while the SDK ships `SoftPub.h`. Provide copies with
    // the expected casing in an extra include directory.
    let include_dirs: Vec<PathBuf> = flags
        .iter()
        .filter_map(|flag| {
            flag.strip_prefix("/imsvc")
                .or_else(|| flag.strip_prefix("-imsvc"))
        })
        .map(PathBuf::from)
        .collect();
    let shim_dir = out_dir.join("include-shim");
    fs::create_dir_all(&shim_dir)?;
    for header in system_includes(cef_dir)? {
        if include_dirs.iter().any(|dir| dir.join(&header).is_file()) {
            continue;
        }
        if let Some(found) = include_dirs
            .iter()
            .find_map(|dir| find_case_insensitive(dir, &header))
        {
            fs::copy(found, shim_dir.join(&header))?;
        }
    }
    flags.push(format!("/imsvc{}", shim_dir.display()));

    // CEF clears CMAKE_CXX_FLAGS when using the Ninja generator (see cef_variables.cmake), which
    // would drop all of the above, so pass them through CEF's own flag lists instead. Those are
    // appended after CEF's flags, so they can also override `/WX`.
    let flags = flags.join(";");
    config
        .define("CEF_C_COMPILER_FLAGS", &flags)
        .define("CEF_CXX_COMPILER_FLAGS", &flags);

    Ok(())
}

/// Collects the file names used in `#include <...>` directives (without directory components) by
/// the CEF headers and wrapper sources.
#[cfg(not(feature = "dox"))]
fn system_includes(
    cef_dir: &std::path::Path,
) -> Result<std::collections::BTreeSet<String>, std::io::Error> {
    fn visit(
        dir: &std::path::Path,
        includes: &mut std::collections::BTreeSet<String>,
    ) -> Result<(), std::io::Error> {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                visit(&path, includes)?;
                continue;
            }
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            for line in source.lines() {
                let Some(directive) = line.trim_start().strip_prefix("#include") else {
                    continue;
                };
                let Some((name, _)) = directive
                    .trim_start()
                    .strip_prefix('<')
                    .and_then(|rest| rest.split_once('>'))
                else {
                    continue;
                };
                if !name.contains('/') {
                    includes.insert(name.to_owned());
                }
            }
        }
        Ok(())
    }

    let mut includes = std::collections::BTreeSet::new();
    for dir in ["include", "libcef_dll"] {
        visit(&cef_dir.join(dir), &mut includes)?;
    }
    Ok(includes)
}

#[cfg(not(feature = "dox"))]
fn find_case_insensitive(dir: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|file_name| file_name.to_str())
                .is_some_and(|file_name| file_name.eq_ignore_ascii_case(name))
        })
}

/// `CEF_API_VERSION_LAST` from the distribution's generated
/// `include/cef_api_versions.h`: the newest versioned (non-experimental) API
/// it supports, written there as `#define CEF_API_VERSION_LAST
/// CEF_API_VERSION_15101`.
#[cfg(not(feature = "dox"))]
fn cef_api_version_last(cef_dir: &std::path::Path) -> anyhow::Result<u32> {
    let header = cef_dir.join("include").join("cef_api_versions.h");
    let contents = std::fs::read_to_string(&header)?;

    contents
        .lines()
        .find_map(|line| {
            line.strip_prefix("#define CEF_API_VERSION_LAST ")?
                .trim()
                .strip_prefix("CEF_API_VERSION_")?
                .parse()
                .ok()
        })
        .ok_or_else(|| anyhow::anyhow!("no CEF_API_VERSION_LAST in {}", header.display()))
}

#[cfg(not(feature = "dox"))]
fn copy_directory(src: &std::path::Path, dest: &std::path::Path) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        if entry.path().is_file() {
            let dest = dest.join(entry.file_name());
            if dest.is_file() {
                std::fs::remove_file(&dest)?;
            }
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

#[cfg(not(feature = "dox"))]
fn copy_cef_runtime_files(
    cef_dir: &std::path::Path,
    target_dir: &std::path::Path,
) -> Result<(), std::io::Error> {
    copy_directory(cef_dir, target_dir)?;

    const LOCALES_DIR: &str = "locales";
    copy_directory(&cef_dir.join(LOCALES_DIR), &target_dir.join(LOCALES_DIR))?;

    Ok(())
}

#[cfg(feature = "dox")]
fn main() {}
