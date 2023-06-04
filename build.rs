fn main() {
    // Ensure that only a single MCU is specified.
    let mcu_features = [cfg!(feature = "esp32s2"), cfg!(feature = "esp32s3")];

    match mcu_features.iter().filter(|&&f| f).count() {
        1 => {}
        n => panic!("Exactly 1 MCU must be selected via its Cargo feature, {n} provided"),
    }

    let pkg_version = env!("CARGO_PKG_VERSION");
    let git_hash_bytes = std::process::Command::new("git")
        .args(&["rev-parse", "--short", "HEAD"])
        .output()
        .expect("Failed to execute git command")
        .stdout;

    let git_hash_str = std::str::from_utf8(&git_hash_bytes)
        .expect("Not a valid utf8 string")
        .trim();

    println!("cargo:rustc-env=FW_VERSION={pkg_version}-{git_hash_str}");
}
