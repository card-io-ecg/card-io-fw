use device_descriptor::*;

device! {
    /// The Status register maintains all flags related to
    /// alert thresholds and battery insertion or removal.
    Status(u16 @ 0x00, default = 0x0002) {
        br @ 15 => BatteryRemoval {
            BatteryRemoved = 1,
            NoRemovalEvent = 0
        },
        smx @ 14 => Alert {
            Alert = 1,
            NoAlert = 0
        },
        tmx @ 13 => Alert,
        vmx @ 12 => Alert,
        bi @ 11 => BatteryInsertion {
            BatteryInserted = 1,
            NoInsertionEvent = 0
        },
        smn @ 10 => Alert,
        tmn @ 9 => Alert,
        vmn @ 8 => Alert,
        dSOCi @ 7 => Alert,
        imx @ 6 => Alert,
        bst @ 3..6 => BatteryStatus {
            BatteryAbsent = 1,
            BatteryPresent = 0
        },
        imn @ 2 => Alert,
        por @ 1 => PowerOnReset {
            Reset = 1,
            NoReset = 0
        }
    }
    VAlrtTh(u16 @ 0x01) {}
    TAlrtTh(u16 @ 0x02) {}
    SAlrtTh(u16 @ 0x03) {}
    AtRate(u16 @ 0x04) {
        /// Host software should write the AtRate register with a negative two’s-complement 16-bit
        /// value of a theoretical load current prior to reading any of the at-rate output registers
        /// (AtTTE, AtAvSOC, AtAvCap).
        current @ 0..16 => u16
    }
    RepCap(u16 @ 0x05) {
        /// RepCap or reported remaining capacity in mAh.
        /// This register is protected from making sudden jumps during load changes.
        capacity @ 0..16 => u16
    }
    RepSOC(u16 @ 0x06) {
        /// RepSOC is the reported state-of-charge percentage output for use by the application GUI.
        percentage @ 0..16 => u16
    }
    Age(u16 @ 0x07) {
        /// The Age register contains a calculated percentage value of the application's present cell
        /// capacity compared to its original design capacity. The result can be used by the host to
        /// gauge the battery pack health as compared to a new pack of the same type.
        /// The equation for the register output is:
        /// Age Register(%) = 100% x (FullCapRep/DesignCap)
        /// For example, if DesignCap = 2000mAh and FullCapRep = 1800mAh, then Age = 90% (or 0x5A00)
        percentage @ 0..16 => u16
    }
    Temp(u16 @ 0x08) {}
    VCell(u16 @ 0x09) {
        voltage @ 0..16 => u16
    }
    Current(u16 @ 0x0A) {
        current @ 0..16 => u16
    }
    AvgCurrent(u16 @ 0x0B) {
        current @ 0..16 => u16
    }
    QResidual(u16 @ 0x0C) {
        /// The QResidual register provides the calculated amount of charge in mAh that is presently
        /// inside of, but cannot be removed from the cell under present application conditions
        /// (load and temperature). This value is subtracted from the MixCap value to determine
        /// capacity available to the user under present conditions (AvCap).
        capacity @ 0..16 => u16
    }
    MixSOC(u16 @ 0x0D) {
        /// The MixCap and MixSOC registers holds the calculated remaining capacity and percentage
        /// of the cell before any empty compensation adjustments are performed.
        percentage @ 0..16 => u16
    }
    AvSOC(u16 @ 0x0E) {
        /// The AvCap and AvSOC registers hold the calculated available capacity and percentage of
        /// the battery based on all inputs from the ModelGauge m5 algorithm including empty
        /// compensation. These registers provide unfiltered results. Jumps in the reported values
        /// can be caused by abrupt changes in load current or temperature.
        percentage @ 0..16 => u16
    }
    MixCap(u16 @ 0x0F, default = 0x0000) {
        /// The MixCap and MixSOC registers holds the calculated remaining capacity and percentage
        /// of the cell before any empty compensation adjustments are performed.
        capacity @ 0..16 => u16
    }

    FullCapRep(u16 @ 0x10, default = 0x0000) {
        /// This register reports the full capacity that goes with RepCap, generally used for
        /// reporting to the GUI. Most applications should only monitor FullCapRep, instead of
        /// FullCap or FullCapNom. A new full-capacity value is calculated at the end of every
        /// charge cycle in the application.
        capacity @ 0..16 => u16
    }
    TTE(u16 @ 0x11) {
        /// The TTE register holds the estimated time to empty for the application under present
        /// temperature and load conditions. The TTE value is determined by relating AvCap with
        /// AvgCurrent. The corresponding AvgCurrent filtering gives a delay in TTE,
        /// but provides more stable results.
        time @ 0..16 => u16
    }
    /// The QRTable00 to QRTable30 register locations contain characterization information
    /// regarding cell capacity under different application conditions.
    QRTable00(u16 @ 0x12) {}
    FullSocThr(u16 @ 0x13, default = 0x5F05) {
        /// The FullSOCThr register gates detection of end-of-charge. VFSOC must be larger than the
        /// FullSOCThr value before IChgTerm is compared to the AvgCurrent register value.
        /// The recommended FullSOCThr register setting for most custom characterized applications
        /// is 95% (default, 0x5F05).
        /// For EZ Performance applications the recommendation is 80%  @ 0x5005.
        /// See the IChgTerm register description and the End-Of-Charge Detection section for details
        percentage @ 0..16 => u16 // mask off 3 lowest bits
    }
    RCell(u16 @ 0x14, default = 0x0290) {
        /// The RCell register provides the calculated internal resistance of the cell. RCell is
        /// determined by comparing open-circuit voltage (VFOCV) against measured voltage (VCell)
        /// over a long time period while under load or charge current.
        resistance @ 0..16 => u16
    }
    // Reserved(u16 @ 0x15) {}
    AvgTA(u16 @ 0x16) {}
    Cycles(u16 @ 0x17, default = 0x0000) {
        /// The Cycles register maintains a total count of the number of charge/discharge cycles of
        /// the cell that have occurred. The result is stored as a percentage of a full cycle.
        /// For example, a full charge/discharge cycle results in the Cycles register incrementing
        /// by 100%. The Cycles register accumulates fractional or whole cycles. For example,
        /// if a battery is cycles 10% x 10 times, then it tracks 100% of a cycle. The Cycles
        /// register has a full range of 0 to 655.35 cycles with a 1% LSb.
        cycles_percentage @ 0..16 => u16
    }
    DesignCap(u16 @ 0x18, default = 0x0000) {
        /// The DesignCap register holds the expected capacity of the cell.
        /// This value is used to determine age and health of the cell by comparing
        /// against the measured present cell capacity.
        capacity @ 0..16 => u16
    }
    AvgVCell(u16 @ 0x19) {
        voltage @ 0..16 => u16
    }
    MaxMinTemp(u16 @ 0x1A) {}
    MaxMinVolt(u16 @ 0x1B) {}
    MaxMinCurr(u16 @ 0x1C) {}
    Config(u16 @ 0x1D, default = 0x2210) {
        /// Set to 0 to use internal die temperature.
        /// Set to 1 to measure temperature using external thermistor.
        /// Set ETHRM to 1 when TSel is 1.
        t_sel @ 15 => TempSelect {
            Internal = 0,
            External = 1
        },

        /// (SOC ALRT Sticky) => When SS = 1, SOC alerts can only be cleared through software.
        /// When SS = 0, SOC alerts are cleared automatically
        /// when the threshold is no longer exceeded.
        ss @ 14 => Clear {
            Software = 1,
            Automatic = 0
        },

        /// (Temperature ALRT Sticky) => When TS = 1, temperature alerts can only be cleared
        /// through software. When TS = 0, temperature alerts are cleared automatically
        /// when the threshold is no longer exceeded.
        ts @ 13 => Clear,

        /// (Voltage ALRT Sticky) => When VS = 1, voltage alerts can only be cleared through software.
        /// When VS = 0, voltage alerts are cleared automatically
        /// when the threshold is no longer exceeded.
        vs @ 12 => Clear,

        /// (Current ALRT Sticky) => When IS = 1, current alerts can only be cleared through software.
        /// When IS = 0, current alerts are cleared automatically
        /// when the threshold is no longer exceeded.
        is @ 11 => Clear,

        /// AIN shutdown
        ainsh @ 10 => Bit {
            Set = 1,
            NotSet = 0
        },

        /// (Enable Temperature Channel) => Set to 1 and set ETHRM or FTHRM to 1 to
        /// enable temperature measurements selected by Config.TSel
        ten @ 9 => Bit,

        /// (Temperature External) => Set to 1 to allow external temperature measurements
        /// to be written to Temp from the host. When set to 0, the IC's
        /// own measurements as used as selected by Config.TSEL.
        tex @ 8 => Bit,

        /// (Shutdown) => Write this bit to logic 1 to force a shutdown of the device after timeout
        /// of the ShdnTimer register (default 45s delay). SHDN is reset to 0 at power-up and upon
        /// exiting shutdown mode. To command shutdown within 22.5s, write ShdnTimer = 0x001E.
        shdn @ 7 => Bit,

        /// (Communication Shutdown) => Set to logic 1 to force the device to enter shutdown mode
        /// if both SDA and SCL are held low for more than timeout of the ShdnTimer register.
        /// This also configures the device to wake up on a rising edge of any communication.
        /// Note that if COMMSH and AINSH are both set to 0, the device wakes up
        /// on any edge of the SDA or SCL pins.
        commsh @ 6 => Bit,

        /// (Enable Thermistor Automatic Bias):. Set to logic 1 to enable
        /// the automatic THRM output bias and AIN measurement.
        ehtrm @ 4 => Bit,

        /// (Force Thermistor Bias Switch) => This allows the host to control the bias of
        /// the thermistor switch or enable fast detection of battery removal.
        /// Set FTHRM = 1 to always enable the thermistor bias switch.
        /// With a standard 10kΩ thermistor, this adds an additional
        /// 200μA to the current drain of the circuit.
        fthrm @ 3 => Bit,

        /// (Enable ALRT Pin Output) => When Aen = 1, violation of any of the alert threshold
        /// register values by temperature, voltage, current, or SOC triggers an alert.
        /// This bit affects the ALRT pin operation only.
        /// The Smx, Smn, Tmx, Tmn, Vmx, Vmn, Imx, and Imn bits
        /// of the Status register (00h) are not disabled.
        aen @ 2 => Bit,

        /// Enable alert on battery insertion when the IC is mounted on the host side.
        /// When Bei = 1, a battery-insertion condition, as detected by the
        /// AIN pin voltage, triggers an alert.
        bei @ 1 => Bit,

        /// Enable alert on battery removal when the IC is mounted on the host side.
        /// When Ber = 1, a battery-removal condition, as detected
        /// by the AIN pin voltage, triggers an alert.
        ber @ 0 => Bit
    }
    IChgTerm(u16 @ 0x1E, default = 0x0640) {
        /// The IChgTerm register allows the device to detect when a charge cycle of the cell has
        /// completed.
        /// IChgTerm should be programmed to the exact charge termination current used in the
        /// application. The device detects end of charge if all the following conditions are met:
        /// - VFSOC register > FullSOCThr register
        /// - AND IChgTerm x 0.125 < Current register < IChgTerm x 1.25
        /// - AND IChgTerm x 0.125 < AvgCurrent register < IChgTerm x 1.25
        current @ 0..16 => u16
    }
    AvCap(u16 @ 0x1F) {
        /// The AvCap and AvSOC registers hold the calculated available capacity and percentage of
        /// the battery based on all inputs from the ModelGauge m5 algorithm including empty
        /// compensation. These registers provide unfiltered results. Jumps in the reported values
        /// can be caused by abrupt changes in load current or temperature.
        capacity @ 0..16 => u16
    }

    TTF(u16 @ 0x20) {
        /// The TTF register holds the estimated time to full for the application under present
        /// conditions. The TTF value is determined by learning the constant current and constant
        /// voltage portions of the charge cycle based on experience of prior charge cycles. Time to
        /// full is then estimated by comparing present charge current to the charge termination
        /// current. Operation of the TTF register assumes all charge profiles are consistent
        /// in the application.
        time @ 0..16 => u16
    }
    DevName(u16 @ 0x21) {
        /// The DevName register holds revision information. The initial silicon is DevName = 0x4010.
        revision @ 0..16 => u16
    }
    /// The QRTable00 to QRTable30 register locations contain characterization information
    /// regarding cell capacity under different application conditions.
    QRTable10(u16 @ 0x22) {}
    FullCapNom(u16 @ 0x23, default = 0x0000) {
        /// This register holds the calculated full capacity of the cell, not including temperature
        /// and empty compensation. A new full-capacity nominal value is calculated each time a cell
        /// relaxation event is detected.
        /// This register is used to calculate other outputs of the ModelGauge m5 algorithm.
        capacity @ 0..16 => u16
    }
    // Reserved(u16 @ 0x24) {}
    // Reserved(u16 @ 0x25) {}
    // Reserved(u16 @ 0x26) {}
    AIN(u16 @ 0x27) {
        /// External temperature measurement on the AIN pin is compared to the THRM pin voltage.
        /// The MAX17055 stores the result as a ratio-metric value from 0% to 100% in the AIN
        /// register with an LSB of 0.0122%. The TGain, TOff, and Curve register values are then
        /// applied to this ratio-metric reading to convert the result to temperature.
        stage @ 0..16 => u16
    }
    LearnCfg(u16 @ 0x28, default = 0x4486) {
        /// Learn Stage then advances to 7h over the course of two full cell cycles to make the
        /// coulomb counter dominate. Host software can write the Learn Stage value to 7h to advance
        /// to the final stage at any time. Writing any value between 1h and 6h is ignored.
        stage @ 4..7 => u8
    }
    FilterCfg(u16 @ 0x29, default = 0xCEA4) {
        /// Sets the time constant for the AvgTA register.
        /// AvgTA time constant = 45s x 2^TEMP
        temp @ 11..14 => u8,

        /// Sets the time constant for the mixing algorithm.
        /// Mixing Period = 45s x 2^(MIX-3)
        mix @ 7..11 => u8,

        /// sets the time constant for the AvgVCell register
        /// AvgVCell time constant = 45s x 2^(VOLT-2)
        volt @ 4..7 => u8,

        /// Sets the time constant for the AvgCurrent register
        /// AvgCurrent time constant = 45s x 2^(CURR-7)
        curr @ 0..4 => u8
    }
    RelaxCfg(u16 @ 0x2A) {
        /// Sets the threshold, which the AvgCurrent and Current registers are compared against.
        /// The AvgCurrent and Current registers must remain below this threshold value for the cell
        /// to be considered unloaded. Load is an unsigned 7-bit value where 1 LSb = 50μV (5mA on
        /// 10mΩ).
        load @ 9..16 => u8,

        /// Sets the change threshold, which AvgVCell is compared against. If the cell voltage
        /// changes by less than dV over two consecutive periods set by dt, the cell is considered
        /// relaxed; dV has a range of 0 to 40mV where 1 LSb = 1.25mV
        dv @ 4..9 => u8,

        /// Sets the time period over which change in AvgVCell is compared against dV. If the cell
        /// voltage changes by less than dV over two consecutive periods set by dt, the cell is
        /// considered relaxed. The comparison period is calculated as:
        /// Relaxation Period = 45s x 2^(dt-8)
        dt @ 0..4 => u8
    }
    MiscCfg(u16 @ 0x2B, default = 0x3870) {
        /// (Full Update Slope) => This value prevents jumps in the RepSOC and FullCapRep registers by
        /// setting the rate of adjustment of FullCapRep near the end of a charge cycle. The update
        /// slope adjustment range is from 2% per 15 minutes (0000b) to a maximum of 32% per 15
        /// minutes (1111b).
        fus @ 12..16 => u8,

        /// This value sets the strength of the servo mixing rate after the final mixing state has
        /// been reached (> 2.08 complete cycles). The units are MR0 = 6.25μV, giving a range up to
        /// 19.375mA with a standard 10mΩ sense resistor. Setting this value to 00000b disables
        /// servo mixing and the MAX17055 continues with time-constant mixing indefinitely.
        mr @ 5..10 => u8,

        /// SOC Alert Config. SOC Alerts can be generated by monitoring any of the
        /// SOC registers as follows.
        sacfg @ 0..2 => SocAlertSource {
            RepSOC = 0,
            AvSOC = 1,
            MixSOC = 2,
            VFSOC = 3
        }
    }
    TGain(u16 @ 0x2C, default = 0xEE56) {
        /// The TGain, TOff, and Curve registers are used to calculate temperature from the measurement
        /// of the AIN pin with an accuracy of ±3°C over a range of -40°C to +85°C.
        gain @ 0..16 => u16
    }
    TOff(u16 @ 0x2D, default = 0x1DA4) {
        /// The TGain, TOff, and Curve registers are used to calculate temperature from the measurement
        /// of the AIN pin with an accuracy of ±3°C over a range of -40°C to +85°C.
        offset @ 0..16 => u16
    }
    CGain(u16 @ 0x2E, default = 0x0400) {
        /// The CGain register adjusts the gain and offset of the current measurement result.
        /// Current register = Current A/D reading × (CGain/0400h) + COff
        gain @ 0..16 => u16
    }
    COff(u16 @ 0x2F, default = 0x0000) {
        /// The COff register adjusts the gain and offset of the current measurement result.
        /// Current register = Current A/D reading × (CGain/0400h) + COff
        offset @ 0..16 => u16
    }

    // Reserved(u16 @ 0x30) {}
    // Reserved(u16 @ 0x31) {}
    /// The QRTable00 to QRTable30 register locations contain characterization information
    /// regarding cell capacity under different application conditions.
    QRTable20(u16 @ 0x32) {}
    // Reserved(u16 @ 0x33) {}
    DieTemp(u16 @ 0x34) {}
    FullCap(u16 @ 0x35) {
        /// FullCap is the full discharge capacity compensated according to the present conditions.
        /// A new full-capacity value is calculated continuously as application conditions change
        /// (temperature and load).
        capacity @ 0..16 => u16
    }
    // Reserved(u16 @ 0x36) {}
    // Reserved(u16 @ 0x37) {}
    /// The RComp0 register holds characterization information critical to computing the
    /// open-circuit voltage of a cell under loaded conditions.
    RComp0(u16 @ 0x38, default = 0x0000) {}
    /// The TempCo register holds temperature compensation information
    /// for the RComp0 register value.
    TempCo(u16 @ 0x39, default = 0x0000) {}
    VEmpty(u16 @ 0x3A, default = 0xA561) {
        /// (Empty Voltage Target, During Load) => The fuel gauge provides capacity and percentage
        /// relative to the empty voltage target, eventually declaring 0% at VE. A 10mV resolution
        /// gives a 0 to 5.11V range. This value is written to 3.3V after reset
        ve @ 7..16 => u16,
        /// (Recovery Voltage) => Sets the voltage level for clearing empty detection. Once the cell
        /// voltage rises above this point, empty voltage detection is reenabled. A 40mV resolution
        /// gives a 0 to 5.08V range. This value is written to 3.88V, which is recommended for most
        /// applications.
        vr @ 0..7 => u16
    }
    // Reserved(u16 @ 0x3B) {}
    // Reserved(u16 @ 0x3C) {}
    FStat(u16 @ 0x3D) {
        /// (Relaxed Cell Detection) => This bit is set to a 1 whenever the ModelGauge m5 algorithm
        /// detects that the cell is in a fully relaxed state. This bit is cleared to 0 whenever a
        /// current greater than the Load threshold is detected.
        rel_dt @ 9 => Bit,

        /// (Empty Detection) => This bit is set to 1 when the IC detects that the cell empty point
        /// has been reached. This bit is reset to 0 when the cell voltage rises above the recovery
        /// threshold.
        e_det @ 8 => Bit,

        /// (Full Qualified) => This bit is set when all charge termination conditions have been met.
        fq @ 7 => Bit,

        /// (Long Relaxation) => This bit is set to a 1 whenever the ModelGauge m5 algorithm detects
        /// that the cell has been relaxed for a period of 48 to 96 minutes or longer. This bit is
        /// cleared to 0 whenever the cell is no longer in a relaxed state.
        rel_dt_2 @ 6 => Bit,

        /// (Data Not Ready) => This bit is set to 1 at cell insertion and remains set until the output
        /// registers have been updated. Afterwards, the IC clears this bit indicating the fuel gauge
        /// calculations are now up to date. This takes 710ms from power-up.
        dnr @ 0 => DataNotReady {
            Ready = 0,
            NotReady = 1
        }
    }
    Timer(u16 @ 0x3E, default = 0x0000) {
        /// TimerH and Timer provide a long-duration time count since last POR. 3.2 hour LSb gives
        /// a full scale range for the register of up to 23.94 years. The Timer register LSb is
        /// 175.8ms giving a full-scale range of 0 to 3.2 hours.
        /// TimerH and Timer can be interpreted together as a 32-bit timer.
        timer_l @ 0..16 => u16
    }
    ShdnTimer(u16 @ 0x3F, default = 0x0000) {
        /// Sets the shutdown timeout period from a minimum of 45s to a maximum of 1.6h.
        /// The default POR value of 0h gives a shutdown delay of 45s.
        /// The equation setting the period is:
        /// Shutdown timeout period = 175.8ms x 2^(8+THR)
        thr @ 13..16 => u8,

        /// (Shutdown Counter) => This register counts the total amount of elapsed time since the
        /// shutdown trigger event. This counter value stops and resets to 0 when the shutdown
        /// timeout completes. The counter LSb is 1.4s.
        ctr @ 0..13 => u16
    }

    UserMem1(u16 @ 0x40) {}
    // Reserved(u16 @ 0x41) {}
    /// The QRTable00 to QRTable30 register locations contain characterization information
    /// regarding cell capacity under different application conditions.
    QRTable30(u16 @ 0x42) {}
    RGain(u16 @ 0x43, default = 0x8080) {
        /// Gain resistance used for peak current and power calculation.
        /// RGain1 = 80% + 0.15625% x RG1. The range of RGain1 is between 80~120%.
        r_gain_1 @ 8..16 => u8,

        /// Gain resistance used for peak current and power calculation.
        /// RGain2 = 60% + 5% x RG2. The range of RGain1 is between 60~140%.
        r_gain_2 @ 4..8 => u8,

        /// Used to calculate the maximum ratio between SPPCurrent to MPPCurrent.
        /// The maximum value of SPPCurrent = MPPCurrent x (0.75-SusToPeakRatio x 0.04).
        sus_to_max_ratio @ 0..4 => u8
    }
    // Reserved(u16 @ 0x44) {}
    dQAcc(u16 @ 0x45, default = 0x0017) {
        /// Capacity (16mAh/LSB). This register tracks change in battery charge between relaxation
        /// points. It is available to the user for debug purposes
        capacity @ 0..16 => u16
    }
    dPAcc(u16 @ 0x46, default = 0x0190) {
        /// Percentage (1/16% per LSB). This register tracks change in battery state of charge
        /// between relaxation points. It is available to the user for debug purposes.
        percentage @ 0..16 => u16
    }
    // Reserved(u16 @ 0x47) {}
    // Reserved(u16 @ 0x48) {}
    /// The ConvgCfg register configures operation of the converge-to-empty feature.
    /// The default and recommended value for ConvgCfg is 0x2241
    ConvgCfg(u16 @ 0x49, default = 0x2241) {}
    VFRemCap(u16 @ 0x4A) {
        /// The VFRemCap register holds the remaining capacity of the cell as determined by the
        /// voltage fuel gauge before any empty compensation adjustments are performed.
        capacity @ 0..16 => u16
    }
    // Reserved(u16 @ 0x4B) {}
    // Reserved(u16 @ 0x4C) {}
    QH(u16 @ 0x4D, default = 0x0000) {
        /// The QH register displays the raw coulomb count generated by the device.
        /// This register is used internally as an input to the mixing algorithm.
        /// Monitoring changes in QH over time can be useful for debugging device operation.
        capacity @ 0..16 => u16
    }
    // Reserved(u16 @ 0x4E) {}
    // Reserved(u16 @ 0x4F) {}

    Status2(u16 @ 0xB0, default = 0x0000) {
        /// If AtRateReady = 1, AtRate output registers are filled by
        /// the firmware and ready to be read by the host.
        at_rate_ready @ 13 => Bit,

        /// If DPReady = 1, Dynamic Power output registers are filled by
        /// the firmware and ready to be read by the host.
        dp_ready @ 12 => Bit,

        /// If SNReady = 1, the unique serial number is available over the I2C.
        /// This bit is set to 1 by firmware after serial number is read internally
        /// and placed into RAM.
        /// The serial number overwrites the Dynamic Power and AtRate output registers.
        sn_ready @ 8 => Bit,

        /// Full detected.
        full_det @ 5 => Bit,

        /// (Hibernate Status) => This bit is set to a 1 when the device is in hibernate mode or 0
        /// when the device is in active mode. Hib is set to 0 at power-up.
        hib @ 1 => Bit
    }
    Power(u16 @ 0xB1) {}
    ID_UserMem2(u16 @ 0xB2) {}
    AvgPower(u16 @ 0xB3) {}
    IAlrtTh(u16 @ 0xB4) {}
    // Reserved(u16 @ 0xB5) {}
    CVMixCap(u16 @ 0xB6) {}
    CVHalfTime(u16 @ 0xB7) {}
    CGTempCo(u16 @ 0xB8, default = 0x0000) {
        /// If CGTempCo is nonzero then CGTempCo is used to adjust current measurements for
        /// temperature. CGTempCo has a range of 0% to 3.1224% per °C with a step size of
        /// 3.1224/0x10000 percent per °C. If a copper trace is used to measure battery current,
        /// CGTempCo should be written to 0x20C8 or 0.4% per °C, which is the approximate
        /// temperature coefficient of a copper trace.
        temp_co @ 0..16 => u16
    }
    Curve(u16 @ 0xB9) {
        /// The upper half of the Curve register applies curvature correction current measurements
        /// made by the IC when using a copper trace as the sense resistor.
        metal_trace_curve @ 8..16 => u8,

        /// See the Temperature Measurements section for a description of the
        /// lower half of the register.
        thermistor @ 0..8 => u8
    }
    HibCfg(u16 @ 0xBA, default = 0x870C) {
        /// When set to 1 the IC enters hibernate mode if conditions are met.
        /// When set to 0 the IC always remains in active mode of operation.
        en_hib @ 15 => Bit,

        /// The HibCfg register controls hibernate mode functionality.
        /// The MAX17055 enters and exits hibernate when the battery current is
        /// less than about C/100. While in hibernate mode the MAX17055 reduces its
        /// operating current to 7µA by reducing ADC sampling to once every 5.625s.
        hib_config @ 0..15 => u16
    }
    Config2(u16 @ 0xBB, default = 0x3658) {
        /// (AtRate Enable) => When this bit is set to 0, AtRate calculations are disabled and
        /// registers AtQResidual/AtTTE/AtAvSOC/AtAvCap are not updated by AtRate calculations.
        at_rate_en @ 13 => Bit,

        /// (Dynamic Power Enable) => When this bit is set to 0, Dynamic Power calculations are
        /// disabled and registers MaxPeakPower/SusPeakPower/MPPCurrent/SPPCurrent are not updated
        /// by Dynamic Power calculations.
        dp_en @ 12 => Bit,

        /// Sets the time constant for the AvgPower register.
        /// The default POR value of 0100b gives a time constant of 11.25s.
        /// The equation setting the period is:
        /// AvgPower time constant = 45s x 2^(POWR-6)
        powr @ 8..12 => u8,

        /// (SOC Change Alert Enable) => Set this bit to 1 to enable alert output with
        /// the Status.dSOCi bit function. Write this bit to 0 to disable the dSOCi alert output.
        /// This bit is set to 0 at power-up.
        d_soc_en @ 7 => Bit,

        /// (Temperature Alert Enable:) => Set this bit to 1 to enable temperature based alerts.
        /// Write this bit to 0 to disable temperature alerts. This bit is set to 1 at power-up.
        t_alrt_en @ 6 => Bit,

        /// Host sets this bit to 1 to initiate firmware to finish processing a newly loaded model.
        /// Firmware clears this bit to zero to indicate that model loading is finished.
        ldm_dl @ 5 => Bit,

        /// (Constant-Power Mode) => Set to 1 to enable constant-power mode. If it is set to 0,
        /// all remaining capacity and remaining time calculations are estimated assuming a
        /// constant-current load. If it is set to 1, the remaining capacity and remaining
        /// time calculations are estimated assuming a constant-power load.
        cp_mode @ 1 => Bit
    }
    VRipple(u16 @ 0xBC, default = 0x0000) {
        /// The VRipple register holds the slow average RMS ripple value of VCell register reading
        /// variation compared to the AvgVCell register. The default filter time is 22.5 seconds.
        /// See RippleCfg register description. VRipple has an LSb weight of 1.25mV/128
        voltage @ 0..16 => u16
    }
    RippleCfg(u16 @ 0xBD, default = 0x0204) {
        /// (Ripple Empty Compensation Coefficient) => Configures MAX17055 to compensate the fuel
        /// gauge % according to the ripple
        kdv @ 3..16 => u8,

        /// Sets the filter magnitude for ripple observation as defined by the following equation
        /// giving a range of 1.4s to 180s.
        /// Ripple Time Range = 1.4 seconds x 2^NR
        nr @ 0..3 => u8
    }
    TimerH(u16 @ 0xBE, default = 0x0000) {
        /// TimerH and Timer provide a long-duration time count since last POR. 3.2 hour LSb gives
        /// a full scale range for the register of up to 23.94 years. The Timer register LSb is
        /// 175.8ms giving a full-scale range of 0 to 3.2 hours.
        /// TimerH and Timer can be interpreted together as a 32-bit timer.
        timer_h @ 0..16 => u16
    }
    // Reserved(u16 @ 0xBF) {}

    RSense_UserMem3(u16 @ 0xD0) {}
    ScOcvLim(u16 @ 0xD1, default = 0x479E) {
        /// Defines the lower limit for keep-out OCV region.
        /// A 5mV resolution gives a 2.56 to 5.12V range. Lower limit voltage of OCV
        /// keep-out region is calculated as 2.56V + OCV_Low_Lim x 5mV.
        ocv_low_lim @ 7..16 => u8,

        /// Defines the delta between lower and upper limits for keep-out OCV region.
        /// A 2.5mV resolution gives a 0 to 320mV range.
        /// Upper limit voltage of OCV keep-out region is calculated as
        /// 2.56V + OCV_Low_Lim x 5mV + OCV_Delta x 2.5mV.
        /// Default OCV_low is 3275mV and OCV_high is 3350mV
        ocv_delta @ 0..7 => u8
    }
    // Reserved(u16 @ 0xD2) {}
    SOCHold(u16 @ 0xD3) {
        /// Enable bit for 99% hold feature during charging. When enabled,
        /// RepSOC holds a maximum value of 99% until full qualified is reached.
        hold_en_99pc @ 12 => Bit,

        /// The positive voltage offset that is added to VEmpty.
        /// At VCell = VEmpty + EmptyVoltHold point, the empty detection/learning is occurred.
        /// EmptyVoltHold has an LSb of 10mV giving a range of 0 to 1270mV.
        empty_volt_hold @ 5..12 => u8,

        /// It is the RepSOC at which RepSOC is held constant until the EmptyVoltHold condition is
        /// crossed. After empty detection occurs, RepSOC update continues as expected. EmptySOCHold
        /// has an LSb of 0.5% with a full range of 0 to 15.5%
        empty_soc_hold @ 0..5 => u8
    }
    MaxPeakPower(u16 @ 0xD4) {
        /// The MAX17055 estimates the maximum instantaneous peak output power of the battery pack
        /// in mW, which the battery can support for up to 10ms, given the external resistance and
        /// required minimum voltage of the voltage regulator.
        /// The MaxPeakPower value is negative (discharge) and updates every 175ms.
        /// LSB is 0.8mW.
        /// Calculation:
        /// MaxPeakPower = MPPCurrent x AvgVCell
        power @ 0..16 => u16
    }
    SusPeakPower(u16 @ 0xD5) {
        /// The fuel gauge estimates the sustainable peak output power of the battery pack in mW,
        /// which the battery supports for up to 10s, given the external resistance and required
        /// minimum voltage of the voltage regulator.
        /// The SusPeakPower value is negative and updated each 175ms.
        /// LSB is 0.8mW.
        /// Calculation:
        /// SusPeakPower = SPPCurrent x AvgVCell
        power @ 0..16 => u16
    }
    PackResistance(u16 @ 0xD6) {
        /// When the MAX17055 is installed host-side, simply set PackResistance to zero, since the
        /// MAX17055 can observe the total resistance between it and the battery.
        ///
        /// However, when the MAX17055 is installed pack-side, configure PackResistance according to
        /// the total non-cell pack resistance. This should account for all resistances due to cell
        /// interconnect, sense resistor, FET, fuse, connector, and other resistance between the
        /// cells and output of the battery pack. The cell internal resistance should not be
        /// included and is estimated by the MAX17055.
        ///
        /// 0x1000 is 1000mΩ, which results in an LSB of 0.244140625mΩ per LSB.
        resistance @ 0..16 => u16
    }
    SysResistance(u16 @ 0xD7) {
        /// Set SysResistance according to the total system resistance. This should include any
        /// connector and PCB trace between the MAX17055 and the system at risk for dropout when
        /// the voltage falls below MinSysVolt.
        /// SysResistance is initialized to a default value upon removal or insertion of a
        /// battery pack. Writes with this function overwrite the default value.
        /// 0x1000 is 1000mΩ, which results in an LSB of 0.244140625mΩ per LSB.
        resistance @ 0..16 => u16
    }
    MinSysVoltage(u16 @ 0xD8) {
        /// Set MinSysVoltage according to the minimum operating voltage of the system. This is
        /// generally associated with a regulator dropout or other system failure/shutdown.
        /// The system should still operate normally until this voltage.
        /// MinSysVoltage is initialized to the default value (3.0V).
        voltage @ 0..16 => u16
    }
    MPPCurrent(u16 @ 0xD9) {
        /// The MAX17055 estimates the maximum instantaneous peak current of the battery pack in mA,
        /// which the battery can support for up to 10ms, given the external resistance and required
        /// minimum voltage of the voltage regulator.
        /// The MPPCurrent value is negative and updates every 175ms.
        current @ 0..16 => u16
    }
    SPPCurrent(u16 @ 0xDA) {
        /// The MAX17055 estimates the sustained peak current of the battery pack in mA, which the
        /// battery can support for up to 10s, given the external resistance and required minimum
        /// voltage of the voltage regulator.
        /// The SPPCurrent value is negative and updates every 175ms.
        current @ 0..16 => u16
    }
    ModelCfg(u16 @ 0xDB, default = 0x0000) {
        /// Set 1 to command the model refreshing.
        /// After firmware executes the command, it will be cleared by firmware
        refresh @ 15 => Bit,

        /// Set VChg = 1 for 4.35V or 4.4V models. Set VChg = 0 for 4.2V models.
        v_chg @ 10 => VChg {
            _4_4V = 1,
            _4_2V = 0
        },

        /// Choose from one of 3 common classifications of lithium cobalt batteries supported by EZ,
        /// without characterization.
        /// For the majority of batteries, use ModelID = 0.
        model_id @ 4..8 => ModelID {
            Default = 0,
            LiFePO = 6
        }
    }
    AtQResidual(u16 @ 0xDC) {
        /// The AtQResidual register provides the calculated amount of charge in mAh that is
        /// presently inside of, but cannot be removed from the cell under present temperature
        /// and hypothetical load (AtRate). This value is subtracted from the MixCap value to
        /// determine capacity available to the user (AtAvCap).
        capacity @ 0..16 => u16
    }
    AtTTE(u16 @ 0xDD) {
        /// The AtTTE register can be used to estimate time to empty for any theoretical load
        /// entered into the AtRate register.
        time @ 0..16 => u16
    }
    AtAvSOC(u16 @ 0xDE) {
        /// The AtAvSOC register holds the theoretical state of charge of the cell based on the
        /// theoretical load of the AtRate register. The register value is stored as a percentage
        /// with a resolution of 1/256 % per LSB. The high byte indicates 1% resolution.
        percentage @ 0..16 => u16
    }
    AtAvCap(u16 @ 0xDF) {
        /// The AtAvCap register holds the estimated remaining capacity of the cell based on the
        /// theoretical load current value of the AtRate register. The value is stored in terms of
        /// µVh and must be divided by the application sense-resistor value to determine
        /// the remaining capacity in mAh.
        capacity @ 0..16 => u16
    }

    VFOCV(u16 @ 0xFB) {
        /// The VFOCV register contains the calculated open-circuit voltage of the cell as determined
        /// by the voltage fuel gauge. This value is used in other internal calculations.
        voltage @ 0..16 => u16
    }
    VFSOC(u16 @ 0xFF) {
        /// The VFSOC register holds the calculated present state of charge of the battery according
        /// to the voltage fuel gauge.
        percentage @ 0..16 => u16
    }

    Command(u16 @ 0x60, default = 0x0000) {
        command @ 0..16 => CommandKind {
            Clear = 0x0000,
            SoftWakeup = 0x0090
        }
    }
}
