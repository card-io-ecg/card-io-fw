fn main() {
    // Ensure that only a single MCU is specified.
    let mcu_features = [cfg!(feature = "esp32s2"), cfg!(feature = "esp32s3")];

    match mcu_features.iter().filter(|&&f| f).count() {
        1 => {}
        n => panic!("Exactly 1 MCU must be selected via its Cargo feature, {n} provided"),
    }
}
