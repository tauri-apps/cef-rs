# cef-rs

Use CEF in Rust.

## Supported Targets

| Target | Linux | macOS | Windows |
| ------ | ----- | ----- | ------- |
| x86_64 | ✅    | ✅    | ✅      |
| ARM64  | ✅    | ✅    | ✅      |

## Usage

### Nix (on Linux)

If you are using Nix on Linux with the `nixpkgs` channel configured, you can take advantage of the `cef-binary` package to share the CEF binaries across your system. Just set the `NIX_CEF_BINARY` environment variable before running any of these `cargo` commands.

```sh
export NIX_CEF_BINARY=1
```

You can still run `export-cef-dir` and set the `CEF_PATH` environment variable if you prefer, e.g., for build output caching, but it is not necessary. If you combine `NIX_CEF_BINARY` with `export-cef-dir`, even `export-cef-dir` will use Nix instead of directly downloading the CEF binaries.

### Install Shared CEF Binaries

This step is optional, but it will make all other builds of the `cef` crate much faster (when not using `NIX_CEF_BINARY`). If you don't do this, the `cef-dll-sys` crate `build.rs` script will download and extract the same files under its `OUT_DIR` directory. You should repeat this step each time you upgrade to a new version of the `cef` crate.

#### Linux or macOS:

```sh
cargo run -p export-cef-dir -- --force $HOME/.local/share/cef
```

#### Windows (using PowerShell)

```pwsh
cargo run -p export-cef-dir -- --force $env:USERPROFILE/.local/share/cef
```

### Set Environment Variables

#### Linux

```sh
export CEF_PATH="$HOME/.local/share/cef"
export LD_LIBRARY_PATH="$LD_LIBRARY_PATH:$CEF_PATH"
```

#### macOS

```sh
export CEF_PATH="$HOME/.local/share/cef"
export DYLD_FALLBACK_LIBRARY_PATH="$DYLD_FALLBACK_LIBRARY_PATH:$CEF_PATH:$CEF_PATH/Chromium Embedded Framework.framework/Libraries"
```

#### Windows (using PowerShell)

```pwsh
$env:CEF_PATH="$env:USERPROFILE/.local/share/cef"
$env:PATH="$env:PATH;$env:CEF_PATH"
```

### Run the `cefsimple` Example

This command should work with each platform:
```sh
cargo run --bin bundle-cef-app -- cefsimple -o target/bundle
```

You can configure the name of the macOS helper in the `Cargo.toml` file, as well as a resource directory that will be copied into the bundle in a platform-appropriate location:
```toml
[package.metadata.cef.bundle]
helper_name = "cefsimple_helper"
resources_path = "resources"
```

#### Linux

There's an extra `--release` flag to build a much smaller bundle on Linux:
```sh
cargo run --bin bundle-cef-app -- cefsimple -o target/bundle --release
./target/bundle/cefsimple.exe
```

#### macOS

The macOS utility creates an application bundle directory at the target location, you can run it with the `open` command:
```sh
cargo run --bin bundle-cef-app -- cefsimple -o target/bundle
open target/bundle/cefsimple.app
```

On macOS, the `bundle-cef-app` utility also supports several additional bundle options, most of which default to the name of the application (e.g. `cefsimple`):
```
Usage: bundle-cef-app [OPTIONS] <NAME>

Arguments:
  <NAME>

Options:
  -o, --output <OUTPUT>
  -i, --identifier <IDENTIFIER>
  -d, --display-name <DISPLAY_NAME>
  -r, --region <REGION>              [default: English]
  -v, --version <VERSION>            [default: 1.0.0]
  -h, --help                         Print help
```

#### Windows (using PowerShell)

The Windows utility supports the `--release` flag, but it makes much less difference in the binary size than on Linux. It also does not copy the resources directory to the bundle, because the preferred mechanism on Windows is to link binary resources directly into the executable.

However, the utility will emit an executable manifest file, and if the `sandbox` feature is enabled, it will build the DLL (cdylib) target instead of the executable (bin) target, and copy that with a renamed `bootstrap.exe` file to the bundle directory, so you can run it from there directly:
```pwsh
cargo run --bin bundle-cef-app -- cefsimple -o ./target/bundle
./target/bundle/cefsimple.exe
```

### Cross-compiling to Windows

The `cef-dll-sys` crate can be cross-compiled to `x86_64-pc-windows-msvc` from Linux with [cargo-xwin](https://github.com/rust-cross/cargo-xwin), which downloads the Windows SDK and sets up a `clang-cl` toolchain for both Rust and CMake. Install `clang`, `lld`, `llvm` and `ninja` from your package manager (the MSVC STL headers require Clang 19 or newer; on Ubuntu 24.04 use [apt.llvm.org](https://apt.llvm.org/)), then:

```sh
rustup target add x86_64-pc-windows-msvc
cargo install cargo-xwin
cargo xwin build --target x86_64-pc-windows-msvc
```

The `libcef_dll_wrapper` static library is built with `clang-cl` and the CEF runtime files are copied next to the output binaries, exactly like a native Windows build. The `CEF_PATH` environment variable works the same way as well: the Windows CEF binaries are downloaded into their own `cef_windows_x86_64` directory next to the host ones.

## Contributing

Please see [CONTRIBUTING.md](CONTRIBUTING.md) for details.
