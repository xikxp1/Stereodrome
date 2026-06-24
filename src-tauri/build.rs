fn main() {
    println!("cargo:rerun-if-env-changed=LASTFM_API_KEY");
    println!("cargo:rerun-if-env-changed=LASTFM_SHARED_SECRET");
    stage_cef_framework_for_raw_macos_dev();
    tauri_build::build()
}

#[cfg(target_os = "macos")]
fn stage_cef_framework_for_raw_macos_dev() {
    use std::path::{Path, PathBuf};

    const CEF_FRAMEWORK: &str = "Chromium Embedded Framework.framework";

    fn profile_dir_from_out_dir(out_dir: &Path) -> Option<PathBuf> {
        out_dir.parent()?.parent()?.parent().map(Path::to_path_buf)
    }

    fn cef_arch_dir() -> Option<&'static str> {
        match std::env::var("CARGO_CFG_TARGET_ARCH").ok()?.as_str() {
            "aarch64" => Some("cef_macos_aarch64"),
            "x86_64" => Some("cef_macos_x86_64"),
            _ => None,
        }
    }

    fn find_cef_framework(build_dir: &Path, arch_dir: &str) -> Option<PathBuf> {
        let entries = std::fs::read_dir(build_dir).ok()?;
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if !file_name.starts_with("cef-dll-sys-") {
                continue;
            }

            let framework = entry.path().join("out").join(arch_dir).join(CEF_FRAMEWORK);
            if framework.is_dir() {
                return Some(framework);
            }
        }

        None
    }

    fn stage_symlink(source: &Path, destination: &Path, label: &str) -> bool {
        if let Ok(metadata) = std::fs::symlink_metadata(destination) {
            let destination_target = destination.canonicalize().ok();
            let source_target = source.canonicalize().ok();
            if metadata.file_type().is_symlink() && destination_target == source_target {
                return true;
            }

            let remove_result = if metadata.is_dir() && !metadata.file_type().is_symlink() {
                std::fs::remove_dir_all(destination)
            } else {
                std::fs::remove_file(destination)
            };
            if let Err(error) = remove_result {
                println!(
                    "cargo:warning=failed to remove stale CEF {label} at {}: {error}",
                    destination.display()
                );
                return false;
            }
        }

        if let Some(parent) = destination.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            println!(
                "cargo:warning=failed to create CEF {label} directory {}: {error}",
                parent.display()
            );
            return false;
        }

        if let Err(error) = std::os::unix::fs::symlink(source, destination) {
            println!(
                "cargo:warning=failed to stage CEF {label} from {} to {}: {error}",
                source.display(),
                destination.display()
            );
            return false;
        }

        true
    }

    let Some(out_dir) = std::env::var_os("OUT_DIR").map(PathBuf::from) else {
        return;
    };
    let Some(profile_dir) = profile_dir_from_out_dir(&out_dir) else {
        return;
    };
    let Some(arch_dir) = cef_arch_dir() else {
        return;
    };
    let build_dir = profile_dir.join("build");
    let Some(source) = find_cef_framework(&build_dir, arch_dir) else {
        println!(
            "cargo:warning=CEF framework not found in {}; raw macOS dev runs may fail until cef-dll-sys has unpacked CEF",
            build_dir.display()
        );
        return;
    };

    let Some(target_dir) = profile_dir.parent() else {
        return;
    };

    stage_symlink(
        &source,
        &target_dir.join("Frameworks").join(CEF_FRAMEWORK),
        "framework",
    );

    let resources = source.join("Resources");
    if resources.is_dir() {
        stage_symlink(&resources, &target_dir.join("Resources"), "resources");
        stage_symlink(
            &resources,
            &profile_dir.join("Resources"),
            "profile resources",
        );
    }
}

#[cfg(not(target_os = "macos"))]
fn stage_cef_framework_for_raw_macos_dev() {}
