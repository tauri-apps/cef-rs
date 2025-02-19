#[cfg(not(feature = "dox"))]
fn main() -> anyhow::Result<()> {
    use download_cef::{CefIndex, OsAndArch};
    use std::{
        env,
        fs::{self, File},
        io::Write,
        path::PathBuf,
    };

    println!("cargo::rerun-if-changed=build.rs");

    let target = env::var("TARGET")?;
    let os_arch = OsAndArch::try_from(target.as_str())?;

    println!("cargo::rerun-if-env-changed=FLATPAK");
    println!("cargo::rerun-if-env-changed=CEF_PATH");
    let cef_path_env = env::var("FLATPAK")
        .map(|_| String::from("/usr/lib"))
        .or_else(|_| env::var("CEF_PATH"));

    let cef_dir = match cef_path_env {
        Ok(cef_path) => {
            // Allow overriding the CEF path with environment variables.
            println!("Using CEF path from environment: {cef_path}");
            PathBuf::from(cef_path)
        }
        Err(_) => {
            let out_dir = PathBuf::from(env::var("OUT_DIR")?);
            let cef_dir = os_arch.to_string();
            let cef_dir = out_dir.join(&cef_dir);

            if !fs::exists(&cef_dir)? {
                let cef_version = env::var("CARGO_PKG_VERSION")?;
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

                let archive_version = serde_json::to_string_pretty(version.minimal()?)?;
                let mut archive_json = File::create(extracted_dir.join("archive.json"))?;
                archive_json.write_all(archive_version.as_bytes())?;
            }

            cef_dir
        }
    };

    let cef_dir = cef_dir.display().to_string();

    println!("cargo::metadata=CEF_DIR={cef_dir}");
    println!("cargo::rustc-link-search=native={cef_dir}");

    let mut cef_dll_wrapper = cmake::Config::new(&cef_dir);
    cef_dll_wrapper.generator("Ninja").no_build_target(true);

    match os_arch.os {
        "linux" => {
            println!("cargo::rustc-link-lib=dylib=cef");
        }
        "windows" => {
            // These libraries consist of two CMake variables, ${CEF_STANDARD_LIBS} and ${CEF_SANDBOX_STANDARD_LIBS}.
            let sdk_libs = "comctl32.lib;gdi32.lib;rpcrt4.lib;shlwapi.lib;ws2_32.lib;Advapi32.lib;dbghelp.lib;Delayimp.lib;ntdll.lib;OleAut32.lib;PowrProf.lib;Propsys.lib;psapi.lib;SetupAPI.lib;Shell32.lib;Shcore.lib;Userenv.lib;version.lib;wbemuuid.lib;WindowsApp.lib;winmm.lib".replace(";", " ");
            let build_dir = cef_dll_wrapper
                .static_crt(true)
                .define("CMAKE_OBJECT_PATH_MAX", "500")
                .define("CMAKE_STATIC_LINKER_FLAGS", &sdk_libs)
                .build()
                .display()
                .to_string();

            println!("cargo::rustc-link-search=native={build_dir}/build/libcef_dll_wrapper");
            println!("cargo::rustc-link-lib=static=libcef_dll_wrapper");

            println!("cargo::rustc-link-lib=dylib=libcef");

            println!("cargo::rustc-link-lib=static=cef_sandbox");
        }
        "macos" => {
            println!("cargo::rustc-link-lib=framework=AppKit");

            let build_dir = cef_dll_wrapper
                .no_default_flags(true)
                .build()
                .display()
                .to_string();
            println!("cargo::rustc-link-search=native={build_dir}/build/libcef_dll_wrapper");
            println!("cargo::rustc-link-lib=static=cef_dll_wrapper");

            println!("cargo::rustc-link-lib=static=cef_sandbox");
            println!("cargo::rustc-link-lib=sandbox");
        }
        os => unimplemented!("unknown target {os}"),
    }

    Ok(())
}

#[cfg(feature = "dox")]
fn main() {}
