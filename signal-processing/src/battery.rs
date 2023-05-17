trait RangeExt {
    // Transforms a value from the self range to 0..1024
    fn to_relative(&self, value: u16) -> u16;
}

impl RangeExt for (u16, u16) {
    fn to_relative(&self, value: u16) -> u16 {
        let (min, max) = *self;

        if value < min {
            return 0;
        }

        if value >= max {
            return MAX_VALUE;
        }

        (((value - min) as u32 * MAX_VALUE as u32) / (max - min) as u32) as u16
    }
}

const MAX_VALUE: u16 = 1024;

#[derive(Clone, Copy)]
pub struct BatteryModel {
    pub voltage: (u16, u16),
    pub charge_current: (u16, u16),
}

impl BatteryModel {
    pub fn estimate(&self, voltage: u16, carging_current: Option<u16>) -> u8 {
        let relative_voltage = self.voltage.to_relative(voltage);
        if let Some(current) = carging_current {
            let relative_current = self.charge_current.to_relative(current);

            let partial_from_voltage = relative_voltage * 50 / MAX_VALUE;
            let partial_from_current = 50 - (relative_current * 50 / MAX_VALUE).min(50);

            (partial_from_voltage + partial_from_current).min(100) as u8
        } else {
            let breakpoint_voltages = &[0, MAX_VALUE / 4, MAX_VALUE * 3 / 4, MAX_VALUE];
            let breakpoint_percentages = &[0, 40, 60, 100];

            match breakpoint_voltages
                .iter()
                .position(|&voltage| voltage > relative_voltage)
            {
                Some(0) => 0,
                None => 100,
                Some(idx) => {
                    let min = breakpoint_voltages[idx - 1];
                    let max = breakpoint_voltages[idx];

                    let out_min = breakpoint_percentages[idx - 1];
                    let out_max = breakpoint_percentages[idx];

                    (((relative_voltage - min) * (out_max - out_min)) / (max - min) + out_min) as u8
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use crate::battery::BatteryModel;

    #[test]
    fn test_discharging() {
        #[rustfmt::skip]
        let table = [
            (3300, 0),
            (3750, 50),
            (4200, 100),
        ];

        let estimator = BatteryModel {
            voltage: (3300, 4200),
            charge_current: (0, 1000),
        };

        for (voltage, expected_percentage) in table {
            assert_eq!(estimator.estimate(voltage, None), expected_percentage);
        }
    }

    #[test]
    fn test_charging() {
        #[rustfmt::skip]
        let table = [
            (3300, 1000, 0),
            (4200, 1000, 50),
            (4200, 500, 75),
            (4200, 0, 100),
        ];

        let estimator = BatteryModel {
            voltage: (3300, 4200),
            charge_current: (0, 1000),
        };

        for (voltage, current, expected_percentage) in table {
            assert_eq!(
                estimator.estimate(voltage, Some(current)),
                expected_percentage
            );
        }
    }
}
