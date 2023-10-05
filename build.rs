#[derive(Clone, Copy)]
enum Mcu {
    ESP32S2,
    ESP32S3,
}

impl Mcu {
    fn as_str(self) -> &'static str {
        match self {
            Self::ESP32S2 => "ESP32-S2",
            Self::ESP32S3 => "ESP32-S3",
        }
    }
}

#[derive(Clone, Copy)]
enum HwVersion {
    V1,
    V2,
    V4,
}

impl HwVersion {
    fn as_str(self) -> &'static str {
        match self {
            Self::V1 => "v1",
            Self::V2 => "v2",
            Self::V4 => "v4",
        }
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
        (cfg!(feature = "esp32s2"), Mcu::ESP32S2),
        (cfg!(feature = "esp32s3"), Mcu::ESP32S3),
    ];

    let Some(mcu) = get_unique(mcu_features) else {
        panic!("Exactly 1 MCU must be selected via its Cargo feature (esp32s2, esp32s3)");
    };

    // Ensure that only a single HW version
    let hw_features = [
        (cfg!(feature = "hw_v1"), HwVersion::V1),
        (cfg!(feature = "hw_v2"), HwVersion::V2),
        (cfg!(feature = "hw_v4"), HwVersion::V4),
    ];

    let Some(hw_version) = get_unique(hw_features) else {
        panic!("Exactly 1 hardware version must be selected via its Cargo feature (hw_v1, hw_v2, hw_v4)");
    };

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
    println!("cargo:rustc-env=HW_VERSION={}", hw_version.as_str());

    // Device info list items
    println!(
        "cargo:rustc-env=FW_VERSION_MENU_ITEM=FW {:>17}",
        format_args!("{pkg_version}-{git_hash_str}")
    );

    println!(
        "cargo:rustc-env=HW_VERSION_MENU_ITEM=HW {:>17}",
        format_args!("{}/{}", mcu.as_str(), hw_version.as_str())
    );
}
