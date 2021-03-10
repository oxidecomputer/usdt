#[cfg(
    all(
        any(
            target_os = "macos",
            target_os = "illumos",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "dragonfly",
            target_os = "windows",
        ),
        feature = "asm",
    )
)]
mod asm;

#[cfg(
    all(
        any(
            target_os = "macos",
            target_os = "illumos",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "dragonfly",
            target_os = "windows",
        ),
        feature = "asm",
    )
)]
pub use crate::asm::{compile_providers, register_probes};

#[cfg(
    not(
        all(
            any(
                target_os = "macos",
                target_os = "illumos",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "openbsd",
                target_os = "dragonfly",
                target_os = "windows",
            ),
            feature = "asm",
        )
    )
)]
mod empty;

#[cfg(
    not(
        all(
            any(
                target_os = "macos",
                target_os = "illumos",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "openbsd",
                target_os = "dragonfly",
                target_os = "windows",
            ),
            feature = "asm",
        )
    )
)]
pub use crate::empty::{compile_providers, register_probes};
