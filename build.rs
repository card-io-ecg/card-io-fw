#[derive(Clone, Copy)]
enum Mcu {
    ESP32S3,
    ESP32C6,
}

impl Mcu {
    fn as_str(self) -> &'static str {
        match self {
            Self::ESP32S3 => "ESP32-S3",
            Self::ESP32C6 => "ESP32-C6",
        }
    }
}

#[derive(Clone, Copy, PartialEq, PartialOrd)]
enum HwVersion {
    V4,
    V6,
}

impl HwVersion {
    fn as_str(self) -> &'static str {
        match self {
            Self::V4 => "v4",
            Self::V6 => "v6",
        }
    }
}

struct BuildConfig {
    mcu: Mcu,
    hw_version: HwVersion,
}

impl BuildConfig {
    fn as_str(&self) -> String {
        let mcu = if self.hw_version >= HwVersion::V6 {
            match self.mcu {
                Mcu::ESP32S3 => "s3",
                Mcu::ESP32C6 => "c6",
            }
        } else {
            ""
        };
        format!("{}{}", self.hw_version.as_str(), mcu)
    }
}

fn get_unique<T, const N: usize>(values: [(bool, T); N]) -> Option<T> {
    let mut count = 0;
    let mut result = None;
    for (cfg, value) in values.into_iter() {
        if cfg {
            count += 1;
            result = Some(value);
        }
    }
    if count > 1 {
        None
    } else {
        result
    }
}

fn main() {
    // Ensure that only a single MCU is specified.
    let mcu_features = [
        (cfg!(feature = "esp32s3"), Mcu::ESP32S3),
        (cfg!(feature = "esp32c6"), Mcu::ESP32C6),
    ];

    let Some(mcu) = get_unique(mcu_features) else {
        panic!("Exactly 1 MCU must be selected via its Cargo feature (esp32s3, esp32c6)");
    };

    // Ensure that only a single HW version
    let hw_features = [
        (cfg!(feature = "hw_v4"), HwVersion::V4),
        (cfg!(feature = "hw_v6"), HwVersion::V6),
    ];

    let Some(hw_version) = get_unique(hw_features) else {
        panic!("Exactly 1 hardware version must be selected via its Cargo feature (hw_v4, hw_v6)");
    };

    let build_config = BuildConfig { mcu, hw_version };

    if cfg!(feature = "defmt") {
        println!("cargo:rustc-link-arg=-Tdefmt.x");
    }

    let pkg_version = env!("CARGO_PKG_VERSION");
    let git_hash_bytes = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .expect("Failed to execute git command")
        .stdout;

    let git_hash_str = std::str::from_utf8(&git_hash_bytes)
        .expect("Not a valid utf8 string")
        .trim();

    println!("cargo:rustc-env=COMMIT_HASH={git_hash_str}");
    println!("cargo:rustc-env=FW_VERSION={pkg_version}-{git_hash_str}");

    println!("cargo:rustc-env=MCU_MODEL={}", mcu.as_str());
    println!("cargo:rustc-env=HW_VERSION={}", build_config.as_str());

    // Device info list items
    println!(
        "cargo:rustc-env=FW_VERSION_MENU_ITEM=FW {:>17}",
        format!("{pkg_version}-{git_hash_str}")
    );

    println!(
        "cargo:rustc-env=HW_VERSION_MENU_ITEM=HW {:>17}",
        format!("{}/{}", mcu.as_str(), build_config.as_str())
    );
}
