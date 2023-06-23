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
    Id(u8, addr=0x0) {
        id(pos = 0, width = 8): DeviceId {
            ADS1191 = 0x50,
            ADS1192 = 0x51,
            ADS1291 = 0x52,
            ADS1292 = 0x53,
            ADS1292R = 0x73
        }
    }

    Config1(u8, addr=0x01, default=0x02) {
        sampling(pos = 7, width = 1): Sampling {
            Continuous = 0,
            SingleShot = 1
        },
        data_rate(pos = 0, width = 3): DataRate {
            _125sps = 0,
            _250sps = 1,
            _500sps = 2,
            _1ksps = 3,
            _2ksps = 4,
            _4ksps = 5,
            _8ksps = 6
        }
    }

    Config2(u8, addr=0x02, default=0x80) {
        pdb_loff_comp(pos = 6, width = 1): Buffer {
            PowerDown = 0,
            Enabled = 1
        },
        ref_voltage(pos = 4, width = 2): ReferenceVoltage {
            External = 0,
            _2_42V = 2,
            _4_033V = 3
        },
        clock_pin(pos = 3, width = 1): ClockPin {
            Disabled = 0,
            Enabled = 1
        },
        test_signal(pos = 0, width = 2): TestSignal {
            Disabled = 0,
            Dc = 1,
            Ac = 2
        }
    }

    Loff(u8, addr=0x03, default=0x10) {
        comp_th(pos = 5, width = 3): ComparatorThreshold {
            _95 = 0,
            _92_5 = 1,
            _90 = 2,
            _87_5 = 3,
            _85 = 4,
            _80 = 5,
            _75 = 6,
            _70 = 7
        },
        leadoff_current(pos = 2, width = 2): LeadOffCurrent {
            _6nA = 0,
            _22nA = 1,
            _6uA = 2,
            _22uA = 3
        },
        leadoff_frequency(pos = 0, width = 1): LeadOffFrequency {
            DC = 0,
            AC = 1
        }
    }

    Ch1Set(u8, addr=0x04, default=0) {
        enabled(pos = 7, width = 1): Channel {
            Enabled = 0,
            PowerDown = 1
        },
        gain(pos = 4, width = 3): Gain {
            x6 = 0,
            x1 = 1,
            x2 = 2,
            x3 = 3,
            x4 = 4,
            x8 = 5,
            x12 = 6
        },
        mux(pos = 0, width = 4): Ch1Mux {
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

    Ch2Set(u8, addr=0x05, default=0) {
        enabled(pos = 7, width = 1): Channel,
        gain(pos = 4, width = 3): Gain,
        mux(pos = 0, width = 4): Ch2Mux {
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

    RldSens(u8, addr=0x06, default=0) {
        chop(pos = 6, width = 2): ChopFrequency {
            Fmod16 = 0,
            Fmod2 = 2,
            Fmod4 = 3
        },
        pdb_rld(pos = 5, width = 1): Buffer,
        loff_sense(pos = 4, width = 1): Input {
            NotConnected = 0,
            Connected = 1
        },
        rld2n(pos = 3, width = 1): Input,
        rld2p(pos = 2, width = 1): Input,
        rld1n(pos = 1, width = 1): Input,
        rld1p(pos = 0, width = 1): Input
    }

    LoffSens(u8, addr=0x07, default=0) {
        flip2(pos = 5, width = 1): CurrentDirection {
            Normal = 0,
            Flipped = 1
        },
        flip1(pos = 4, width = 1): CurrentDirection,
        loff2n(pos = 3, width = 1): Input,
        loff2p(pos = 2, width = 1): Input,
        loff1n(pos = 1, width = 1): Input,
        loff1p(pos = 0, width = 1): Input
    }

    LoffStat(u8, addr=0x08, default=0) {
        clk_div(pos = 6, width = 1): ClockDivider {
            External512kHz = 0,
            External2MHz = 1
        },
        rld(pos = 4, width = 1): LeadStatus {
            Connected = 0,
            NotConnected = 1
        },
        in2n(pos = 3, width = 1): LeadStatus,
        in2p(pos = 2, width = 1): LeadStatus,
        in1n(pos = 1, width = 1): LeadStatus,
        in1p(pos = 0, width = 1): LeadStatus
    }

    Resp1(u8, addr=0x09, default=0x02) {
        demod_en(pos = 7, width = 1): Respiration {
            Disabled = 0,
            Enabled = 1
        },
        mod_en(pos = 6, width = 1): Respiration,
        phase(pos = 2, width = 4): Phase {
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
        clock(pos = 0, width = 1): RespirationClock {
            Internal = 0,
            External = 1
        }
    }

    Resp2(u8, addr=0x0A, default=0x05) {
        calibration(pos = 7, width = 1): Calibration {
            Disabled = 0,
            Enabled = 1
        },
        frequency(pos = 2, width = 1): RespirationFrequency {
            _32kHz = 0,
            _64kHz = 1
        },
        rld_reference(pos = 1, width = 1): RldReference {
            External = 0,
            MidSupply = 1
        }
    }

    Gpio(u8, addr=0x0B, default=0x0C) {
        c2(pos = 3, width = 1): PinDirection {
            Output = 0,
            Input = 1
        },
        c1(pos = 2, width = 1): PinDirection,
        d2(pos = 1, width = 1): PinState {
            Low = 0,
            High = 1
        },
        d1(pos = 0, width = 1): PinState
    }
}
