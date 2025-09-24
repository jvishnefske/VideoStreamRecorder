use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    // Tell cargo to rerun this build script if package.json or frontend files change
    println!("cargo:rerun-if-changed=package.json");
    println!("cargo:rerun-if-changed=frontend/src");

    // Only build frontend if we're not in a cross-compilation or docs build
    let target = env::var("TARGET").unwrap_or_default();
    let host = env::var("HOST").unwrap_or_default();

    if env::var("DOCS_RS").is_ok() || target != host {
        println!("cargo:warning=Skipping frontend build for cross-compilation or docs");
        return;
    }

    let frontend_dist = Path::new("frontend/dist");
    let package_json = Path::new("package.json");
    let node_modules = Path::new("node_modules");

    // Check if we need to build the frontend
    let needs_build = !frontend_dist.exists()
        || !frontend_dist.join("index.html").exists()
        || needs_rebuild(package_json, frontend_dist)
        || needs_rebuild(Path::new("frontend/src"), frontend_dist);

    if !needs_build {
        println!("cargo:warning=Frontend is up to date, skipping build");
        return;
    }

    println!("cargo:warning=Building frontend...");

    // Check if node_modules exists, if not run npm install
    if !node_modules.exists() {
        println!("cargo:warning=Installing npm dependencies...");
        let output = Command::new("npm")
            .arg("install")
            .output()
            .expect("Failed to run npm install. Make sure Node.js and npm are installed.");

        if !output.status.success() {
            panic!(
                "npm install failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    // Build the frontend
    let output = Command::new("npm")
        .arg("run")
        .arg("build")
        .output()
        .expect("Failed to run npm run build");

    if !output.status.success() {
        panic!(
            "Frontend build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    println!("cargo:warning=Frontend build completed successfully");

    // Tell cargo where to find the static files
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("frontend_dist");

    if dest_path.exists() {
        fs::remove_dir_all(&dest_path).unwrap();
    }

    copy_dir_all(frontend_dist, &dest_path).unwrap();
    println!("cargo:rustc-env=FRONTEND_DIST_PATH={}", dest_path.display());
}

fn needs_rebuild(source: &Path, target: &Path) -> bool {
    if !target.exists() {
        return true;
    }

    let target_modified = get_last_modified(target);

    if source.is_file() {
        get_last_modified(source) > target_modified
    } else if source.is_dir() {
        // Check if any file in source directory is newer than target
        check_dir_modified(source, target_modified)
    } else {
        false
    }
}

fn get_last_modified(path: &Path) -> std::time::SystemTime {
    if path.is_file() {
        fs::metadata(path).unwrap().modified().unwrap()
    } else if path.is_dir() {
        // For directories, get the latest modification time of any contained file
        get_dir_last_modified(path)
    } else {
        std::time::UNIX_EPOCH
    }
}

fn get_dir_last_modified(dir: &Path) -> std::time::SystemTime {
    let mut latest = std::time::UNIX_EPOCH;

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let modified = if path.is_dir() {
                get_dir_last_modified(&path)
            } else {
                fs::metadata(&path).map(|m| m.modified().unwrap_or(std::time::UNIX_EPOCH)).unwrap_or(std::time::UNIX_EPOCH)
            };

            if modified > latest {
                latest = modified;
            }
        }
    }

    latest
}

fn check_dir_modified(source: &Path, target_time: std::time::SystemTime) -> bool {
    if let Ok(entries) = fs::read_dir(source) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Ok(metadata) = fs::metadata(&path) {
                    if let Ok(modified) = metadata.modified() {
                        if modified > target_time {
                            return true;
                        }
                    }
                }
            } else if path.is_dir() && check_dir_modified(&path, target_time) {
                return true;
            }
        }
    }
    false
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}