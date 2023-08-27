#![allow(non_camel_case_types)]

use device_descriptor::*;

#[derive(Debug, Copy, Clone)]
pub enum Command {
    WAKEUP,
    STANDBY,
    RESET,
    START,
    STOP,
    OFFSETCAL,
    RDATAC,
    SDATAC,
    RDATA,
    RREG(u8, u8),
    WREG(u8, u8),
}

impl From<Command> for ([u8; 2], usize) {
    fn from(val: Command) -> Self {
        match val {
            Command::WAKEUP => ([0x02, 0], 1),
            Command::STANDBY => ([0x04, 0], 1),
            Command::RESET => ([0x06, 0], 1),
            Command::START => ([0x08, 0], 1),
            Command::STOP => ([0x0A, 0], 1),
            Command::OFFSETCAL => ([0x1A, 0], 1),
            Command::RDATAC => ([0x10, 0], 1),
            Command::SDATAC => ([0x11, 0], 1),
            Command::RDATA => ([0x12, 0], 1),
            Command::RREG(reg, len) => ([0x20 | reg, len - 1], 2),
            Command::WREG(reg, len) => ([0x40 | reg, len - 1], 2),
        }
    }
}

device! {
    /// Device ID
    Id(u8 @ 0x0) {
        id @ 0..8 => DeviceId {
            ADS1191 = 0x50,
            ADS1192 = 0x51,
            ADS1291 = 0x52,
            ADS1292 = 0x53,
            ADS1292R = 0x73
        }
    }

    Config1(u8 @ 0x01, default=0x02) {
        sampling @ 7 => Sampling {
            Continuous = 0,
            SingleShot = 1
        },
        data_rate @ 0..3 => DataRate {
            _125sps = 0,
            _250sps = 1,
            _500sps = 2,
            _1ksps = 3,
            _2ksps = 4,
            _4ksps = 5,
            _8ksps = 6
        }
    }

    Config2(u8 @ 0x02, default=0x80) {
        pdb_loff_comp @ 6 => Buffer {
            PowerDown = 0,
            Enabled = 1
        },
        ref_voltage @ 4..6 => ReferenceVoltage {
            External = 0,
            _2_42V = 2,
            _4_033V = 3
        },
        clock_pin @ 3 => ClockPin {
            Disabled = 0,
            Enabled = 1
        },
        test_signal @ 0..2 => TestSignal {
            Disabled = 0,
            Dc = 1,
            Ac = 2
        }
    }

    Loff(u8 @ 0x03, default=0x10) {
        comp_th @ 5..8 => ComparatorThreshold {
            _95 = 0,
            _92_5 = 1,
            _90 = 2,
            _87_5 = 3,
            _85 = 4,
            _80 = 5,
            _75 = 6,
            _70 = 7
        },
        leadoff_current @ 2..4 => LeadOffCurrent {
            _6nA = 0,
            _22nA = 1,
            _6uA = 2,
            _22uA = 3
        },
        leadoff_frequency @ 0 => LeadOffFrequency {
            DC = 0,
            AC = 1
        }
    }

    Ch1Set(u8 @ 0x04, default=0) {
        enabled @ 7 => Channel {
            Enabled = 0,
            PowerDown = 1
        },
        gain @ 4..7 => Gain {
            x6 = 0,
            x1 = 1,
            x2 = 2,
            x3 = 3,
            x4 = 4,
            x8 = 5,
            x12 = 6
        },
        mux @ 0..4 => Ch1Mux {
            Normal = 0,
            Shorted = 1,
            Rld = 2,
            HalfAvdd = 3,
            Temperature = 4,
            TestSignal = 5,
            RldDrp = 6,
            RldDrm = 7,
            RldDrpm = 8,
            In3 = 9
        }
    }

    Ch2Set(u8 @ 0x05, default=0) {
        enabled @ 7 => Channel,
        gain @ 4..7 => Gain,
        mux @ 0..4 => Ch2Mux {
            Normal = 0,
            Shorted = 1,
            Rld = 2,
            QuarterDvdd = 3,
            Temperature = 4,
            TestSignal = 5,
            RldDrp = 6,
            RldDrm = 7,
            RldDrpm = 8,
            In3 = 9
        }
    }

    RldSens(u8 @ 0x06, default=0) {
        chop @ 6..8 => ChopFrequency {
            Fmod16 = 0,
            Fmod2 = 2,
            Fmod4 = 3
        },
        pdb_rld @ 5 => Buffer,
        loff_sense @ 4 => Input {
            NotConnected = 0,
            Connected = 1
        },
        rld2n @ 3 => Input,
        rld2p @ 2 => Input,
        rld1n @ 1 => Input,
        rld1p @ 0 => Input
    }

    LoffSens(u8 @ 0x07, default=0) {
        flip2 @ 5 => CurrentDirection {
            Normal = 0,
            Flipped = 1
        },
        flip1 @ 4 => CurrentDirection,
        loff2n @ 3 => Input,
        loff2p @ 2 => Input,
        loff1n @ 1 => Input,
        loff1p @ 0 => Input
    }

    LoffStat(u8 @ 0x08, default=0) {
        clk_div @ 6 => ClockDivider {
            External512kHz = 0,
            External2MHz = 1
        },
        rld @ 4 => LeadStatus {
            Connected = 0,
            NotConnected = 1
        },
        in2n @ 3 => LeadStatus,
        in2p @ 2 => LeadStatus,
        in1n @ 1 => LeadStatus,
        in1p @ 0 => LeadStatus
    }

    Resp1(u8 @ 0x09, default=0x02) {
        demod_en @ 7 => Respiration {
            Disabled = 0,
            Enabled = 1
        },
        mod_en @ 6 => Respiration,
        phase @ 2..6 => Phase {
            _0deg = 0,
            _11deg = 1,
            _22deg = 2,
            _33deg = 3,
            _45deg = 4,
            _56deg = 5,
            _67deg = 6,
            _78deg = 7,
            _90deg = 8,
            _101deg = 9,
            _112deg = 10,
            _123deg = 11,
            _135deg = 12,
            _146deg = 13,
            _157deg = 14,
            _168deg = 15
        },
        clock @ 0 => RespirationClock {
            Internal = 0,
            External = 1
        }
    }

    Resp2(u8 @ 0x0A, default=0x05) {
        calibration @ 7 => Calibration {
            Disabled = 0,
            Enabled = 1
        },
        frequency @ 2 => RespirationFrequency {
            _32kHz = 0,
            _64kHz = 1
        },
        rld_reference @ 1 => RldReference {
            External = 0,
            MidSupply = 1
        }
    }

    Gpio(u8 @ 0x0B, default=0x0C) {
        c2 @ 3 => PinDirection {
            Output = 0,
            Input = 1
        },
        c1 @ 2 => PinDirection,
        d2 @ 1 => PinState {
            Low = 0,
            High = 1
        },
        d1 @ 0 => PinState
    }
}
