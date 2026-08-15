#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod x86_64_unknown_linux_gnu;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub use x86_64_unknown_linux_gnu::*;

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
mod aarch64_unknown_linux_gnu;
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub use aarch64_unknown_linux_gnu::*;

#[cfg(all(target_os = "linux", target_arch = "arm"))]
mod arm_unknown_linux_gnueabi;
#[cfg(all(target_os = "linux", target_arch = "arm"))]
pub use arm_unknown_linux_gnueabi::*;

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
mod x86_64_pc_windows_msvc;
#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
pub use x86_64_pc_windows_msvc::*;

#[cfg(all(target_os = "windows", target_arch = "x86"))]
mod i686_pc_windows_msvc;
#[cfg(all(target_os = "windows", target_arch = "x86"))]
pub use i686_pc_windows_msvc::*;

#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
mod aarch64_pc_windows_msvc;
#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
pub use aarch64_pc_windows_msvc::*;

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
mod x86_64_apple_darwin;
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
pub use x86_64_apple_darwin::*;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod aarch64_apple_darwin;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub use aarch64_apple_darwin::*;
