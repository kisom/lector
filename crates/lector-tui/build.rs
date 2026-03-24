fn git_version() -> String {
    let output = std::process::Command::new("git")
        .args(["describe", "--tags", "--match", "v*"])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        }
        _ => "v0.0.0-dev".to_string(),
    }
}

fn main() {
    println!("cargo::rustc-env=LECTOR_VERSION={}", git_version());
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/tags");
}
