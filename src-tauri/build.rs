fn main() {
    // Only invoke `tauri_build::build()` when the `gui` feature is on.
    // With it off, the crate compiles as a headless-only library and
    // tauri-build is not even a dep.
    #[cfg(feature = "gui")]
    tauri_build::build();
}
