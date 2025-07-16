#[cfg(not(feature = "dox"))]
fn main() -> anyhow::Result<()> {
    use download_cef::{CefIndex, OsAndArch};
    use std::{env, fs, path::PathBuf};

    println!("cargo::rerun-if-changed=build.rs");

    let target = env::var("TARGET")?;
    let os_arch = OsAndArch::try_from(target.as_str())?;

    println!("cargo::rerun-if-env-changed=FLATPAK");
    println!("cargo::rerun-if-env-changed=CEF_PATH");
    let cef_path_env = env::var("FLATPAK")
        .map(|_| String::from("/usr/lib"))
        .or_else(|_| env::var("CEF_PATH"));
    
    println!("cargo::rerun-if-env-changed=CEF_PATH_NO_CHECK");
    let cef_path_no_check = match env::var("CEF_PATH_NO_CHECK") {
        Ok(v) if v == "true" || v == "1" => true,
        _ => false
    };

    let cef_dir = match cef_path_env {
        Ok(cef_path) => {
            // Allow overriding the CEF path with environment variables.
            println!("Using CEF path from environment: {cef_path}");
            if !cef_path_no_check {
                download_cef::check_archive_json(&env::var("CARGO_PKG_VERSION")?, &cef_path)?;
            }
            PathBuf::from(cef_path)
        }
        Err(_) => {
            let out_dir = PathBuf::from(env::var("OUT_DIR")?);
            let cef_dir = os_arch.to_string();
            let cef_dir = out_dir.join(&cef_dir);

            if !fs::exists(&cef_dir)? {
                let cef_version = download_cef::default_version(&env::var("CARGO_PKG_VERSION")?);
                let index = CefIndex::download()?;
                let platform = index.platform(&target)?;
                let version = platform.version(&cef_version)?;

                let archive = version.download_archive(&out_dir, false)?;
                let extracted_dir =
                    download_cef::extract_target_archive(&target, &archive, &out_dir, false)?;
                if extracted_dir != cef_dir {
                    return Err(anyhow::anyhow!(
                        "extracted dir {extracted_dir:?} does not match cef_dir {cef_dir:?}",
                    ));
                }

                version.write_archive_json(extracted_dir)?;
            }

            cef_dir
        }
    };

    let cef_dir = cef_dir.display().to_string();

    println!("cargo::metadata=CEF_DIR={cef_dir}");
    println!("cargo::rustc-link-search=native={cef_dir}");

    let mut cef_dll_wrapper = cmake::Config::new(&cef_dir);
    cef_dll_wrapper
        .generator("Ninja")
        .profile("RelWithDebInfo")
        .build_target("libcef_dll_wrapper");

    let project_arch = match os_arch.arch {
        "aarch64" => "arm64",
        arch => arch,
    };

    match os_arch.os {
        "linux" => {
            println!("cargo::rustc-link-lib=dylib=cef");
        }
        "windows" => {
            let sdk_libs = [
                "comctl32.lib",
                "delayimp.lib",
                "mincore.lib",
                "powrprof.lib",
                "propsys.lib",
                "runtimeobject.lib",
                "setupapi.lib",
                "shcore.lib",
                "shell32.lib",
                "shlwapi.lib",
                "user32.lib",
                "version.lib",
                "wbemuuid.lib",
                "winmm.lib",
            ]
            .join(" ");

            let build_dir = cef_dll_wrapper
                .define("CMAKE_MSVC_RUNTIME_LIBRARY", "MultiThreaded")
                .define("CMAKE_OBJECT_PATH_MAX", "500")
                .define("CMAKE_STATIC_LINKER_FLAGS", &sdk_libs)
                .define("PROJECT_ARCH", project_arch)
                .build()
                .display()
                .to_string();

            println!("cargo::rustc-link-search=native={build_dir}/build/libcef_dll_wrapper");
            println!("cargo::rustc-link-lib=static=libcef_dll_wrapper");

            println!("cargo::rustc-link-lib=dylib=libcef");
        }
        "macos" => {
            println!("cargo::rustc-link-lib=framework=AppKit");

            let build_dir = cef_dll_wrapper
                .no_default_flags(true)
                .define("PROJECT_ARCH", project_arch)
                .build()
                .display()
                .to_string();
            println!("cargo::rustc-link-search=native={build_dir}/build/libcef_dll_wrapper");
            println!("cargo::rustc-link-lib=static=cef_dll_wrapper");

            println!("cargo::rustc-link-search=native={cef_dir}/Chromium Embedded Framework.framework/Libraries");
            println!("cargo::rustc-link-lib=dylib=cef_sandbox");
            println!("cargo::rustc-link-lib=sandbox");
        }
        os => unimplemented!("unknown target {os}"),
    }

    Ok(())
}

#[cfg(feature = "dox")]
fn main() {}
