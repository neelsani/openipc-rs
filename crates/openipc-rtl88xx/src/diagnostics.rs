//! Structured diagnostics retained independently from the rolling log.

use std::future::Future;

use openipc_core::RxDescriptorKind;

use crate::{
    async_efuse::EfuseInfo,
    device::RealtekDevice,
    time::monotonic_micros,
    types::{ChipInfo, DriverError, MonitorOptions, RadioConfig},
};

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const MAX_REGISTER_TRACE_ENTRIES: usize = 65_536;

/// Raw inputs and resulting chip selection from the hardware probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeDiagnostics {
    /// USB vendor identifier.
    pub vendor_id: u16,
    /// USB product identifier.
    pub product_id: u16,
    /// Raw `SYS_CFG` value used for cut and RF-path detection.
    pub sys_cfg: u32,
    /// Raw `SYS_CFG2` chip identifier used for shared USB product IDs.
    pub sys_cfg2_chip_id: u8,
    /// Chip selected from the probe inputs.
    pub chip: ChipInfo,
    /// RX descriptor layout selected for the chip.
    pub rx_descriptor: RxDescriptorKind,
}

/// Sanitized interpretation of the adapter's EFUSE map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EfuseDiagnostics {
    /// EEPROM identifier at the start of the logical map.
    pub eeprom_id: u16,
    /// Whether the EEPROM identifier indicates a valid autoloaded map.
    pub autoload_valid: bool,
    /// FNV-1a fingerprint of the complete logical map.
    pub map_fingerprint: u64,
    /// Number of logical-map bytes that are not erased (`0xff`).
    pub programmed_bytes: usize,
    /// Selected RFE front-end type.
    pub rfe_type: u8,
    /// Board/amplifier type encoded for PHY table conditions.
    pub board_type: u8,
    /// Whether an external 2.4 GHz PA was detected.
    pub external_pa_2g: bool,
    /// Whether an external 5 GHz PA was detected.
    pub external_pa_5g: bool,
    /// Whether an external 2.4 GHz LNA was detected.
    pub external_lna_2g: bool,
    /// Whether an external 5 GHz LNA was detected.
    pub external_lna_5g: bool,
    /// Crystal-cap calibration value.
    pub crystal_cap: u8,
    /// Primary thermal-meter baseline.
    pub thermal_meter: u8,
    /// Per-path thermal-meter baselines where available.
    pub thermal_meter_paths: [u8; 2],
    /// Whether IC defaults replaced invalid or blank TX-power calibration.
    pub tx_power_defaults: bool,
    /// Whether the logical map contained a valid MAC address.
    pub mac_present: bool,
}

/// Aggregate register-I/O evidence for an initialization run or stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterIoDiagnostics {
    /// Successful register reads.
    pub reads: u64,
    /// Successful register writes.
    pub writes: u64,
    /// Bytes returned by successful reads.
    pub read_bytes: u64,
    /// Bytes sent by successful writes.
    pub write_bytes: u64,
    /// Failed register operations.
    pub failures: u64,
    /// Ordered FNV-1a fingerprint of operation, address, length, and data.
    pub fingerprint: u64,
}

/// One ordered register operation retained for initialization forensics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterTraceEntry {
    /// Zero-based operation sequence within this initialization attempt.
    pub sequence: u64,
    /// Monotonic offset from initialization start.
    pub offset_us: u64,
    /// Initialization stage active when the operation occurred.
    pub stage: Option<String>,
    /// `read` or `write`.
    pub operation: &'static str,
    /// Realtek vendor-register address.
    pub register: u16,
    /// Actual bytes read or written. Empty for a failed operation.
    pub bytes: Vec<u8>,
    /// Whether the USB control operation succeeded.
    pub success: bool,
    /// Failure text retained for unsuccessful operations.
    pub error: Option<String>,
}

impl Default for RegisterIoDiagnostics {
    fn default() -> Self {
        Self {
            reads: 0,
            writes: 0,
            read_bytes: 0,
            write_bytes: 0,
            failures: 0,
            fingerprint: FNV_OFFSET,
        }
    }
}

impl RegisterIoDiagnostics {
    fn record(&mut self, operation: u8, register: u16, bytes: &[u8], success: bool) {
        match (operation, success) {
            (b'R', true) => {
                self.reads = self.reads.saturating_add(1);
                self.read_bytes = self.read_bytes.saturating_add(bytes.len() as u64);
            }
            (b'W', true) => {
                self.writes = self.writes.saturating_add(1);
                self.write_bytes = self.write_bytes.saturating_add(bytes.len() as u64);
            }
            _ => self.failures = self.failures.saturating_add(1),
        }
        for byte in [operation]
            .into_iter()
            .chain(register.to_le_bytes())
            .chain((bytes.len() as u64).to_le_bytes())
            .chain([u8::from(success)])
            .chain(bytes.iter().copied())
        {
            self.fingerprint = (self.fingerprint ^ u64::from(byte)).wrapping_mul(FNV_PRIME);
        }
    }
}

/// One timed initialization stage and its register-I/O evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitStageDiagnostics {
    /// Stable stage name.
    pub name: String,
    /// Offset from initialization start.
    pub started_us: u64,
    /// Stage duration.
    pub duration_us: u64,
    /// Whether the stage completed successfully.
    pub success: bool,
    /// Error text retained when the stage failed.
    pub error: Option<String>,
    /// Register operations performed by this stage.
    pub register_io: RegisterIoDiagnostics,
}

/// One post-initialization register value or read failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterSnapshot {
    /// Human-readable register name.
    pub name: &'static str,
    /// Register address.
    pub address: u16,
    /// Requested register width in bytes.
    pub width: u16,
    /// Little-endian value rendered as hexadecimal.
    pub value: Option<String>,
    /// Read failure, if any.
    pub error: Option<String>,
}

/// Full structured evidence for the most recent monitor initialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverDiagnostics {
    /// Schema version for support-bundle consumers.
    pub schema_version: u32,
    /// Monotonic timestamp at initialization start.
    pub started_us: u64,
    /// Total initialization duration once complete or failed.
    pub duration_us: Option<u64>,
    /// Requested channel.
    pub channel: u8,
    /// Requested channel width.
    pub channel_width_mhz: u16,
    /// Requested primary-channel offset.
    pub channel_offset: u8,
    /// Effective monitor options, including environment overrides.
    pub effective_options: String,
    /// Hardware probe evidence.
    pub probe: Option<ProbeDiagnostics>,
    /// Decoded EFUSE evidence.
    pub efuse: Option<EfuseDiagnostics>,
    /// Ordered initialization stages.
    pub stages: Vec<InitStageDiagnostics>,
    /// Aggregate register-I/O evidence across the run.
    pub register_io: RegisterIoDiagnostics,
    /// Ordered register operations with actual values for trace comparison.
    pub register_trace: Vec<RegisterTraceEntry>,
    /// Operations omitted after the bounded trace reached its limit.
    pub register_trace_dropped: u64,
    /// Final register snapshot collected without failing initialization.
    pub post_init_registers: Vec<RegisterSnapshot>,
    /// Final initialization error, if the run failed.
    pub error: Option<String>,
    /// Whether initialization reached its final success marker.
    pub completed: bool,
}

impl Default for DriverDiagnostics {
    fn default() -> Self {
        Self {
            schema_version: 2,
            started_us: 0,
            duration_us: None,
            channel: 0,
            channel_width_mhz: 0,
            channel_offset: 0,
            effective_options: String::new(),
            probe: None,
            efuse: None,
            stages: Vec::new(),
            register_io: RegisterIoDiagnostics::default(),
            register_trace: Vec::new(),
            register_trace_dropped: 0,
            post_init_registers: Vec::new(),
            error: None,
            completed: false,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct DriverDiagnosticsState {
    pub(crate) snapshot: DriverDiagnostics,
    active_stage: Option<ActiveStage>,
}

#[derive(Debug)]
struct ActiveStage {
    name: String,
    started_us: u64,
    register_io: RegisterIoDiagnostics,
}

impl RealtekDevice {
    pub(crate) fn begin_diagnostics(&self, radio: RadioConfig, options: MonitorOptions) {
        let started_us = monotonic_micros();
        let mut state = self
            .diagnostics
            .lock()
            .expect("driver diagnostics poisoned");
        state.snapshot = DriverDiagnostics {
            started_us,
            channel: radio.channel,
            channel_width_mhz: match radio.channel_width {
                crate::ChannelWidth::Mhz5 => 5,
                crate::ChannelWidth::Mhz10 => 10,
                crate::ChannelWidth::Mhz20 => 20,
                crate::ChannelWidth::Mhz40 => 40,
                crate::ChannelWidth::Mhz80 => 80,
            },
            channel_offset: radio.channel_offset,
            effective_options: format!("{options:?}"),
            ..DriverDiagnostics::default()
        };
        state.active_stage = None;
    }

    pub(crate) async fn diagnostic_stage<T>(
        &self,
        name: impl Into<String>,
        operation: impl Future<Output = Result<T, DriverError>>,
    ) -> Result<T, DriverError> {
        let name = name.into();
        let started_us = monotonic_micros();
        {
            let mut state = self
                .diagnostics
                .lock()
                .expect("driver diagnostics poisoned");
            state.active_stage = Some(ActiveStage {
                name: name.clone(),
                started_us,
                register_io: RegisterIoDiagnostics::default(),
            });
        }
        log::debug!(target: "openipc_rtl88xx::init", "initialization stage started: {name}");
        let result = operation.await;
        let finished_us = monotonic_micros();
        let mut state = self
            .diagnostics
            .lock()
            .expect("driver diagnostics poisoned");
        let active = state.active_stage.take().unwrap_or(ActiveStage {
            name: name.clone(),
            started_us,
            register_io: RegisterIoDiagnostics::default(),
        });
        let error = result.as_ref().err().map(ToString::to_string);
        let success = error.is_none();
        let relative_start = active.started_us.saturating_sub(state.snapshot.started_us);
        state.snapshot.stages.push(InitStageDiagnostics {
            name: active.name,
            started_us: relative_start,
            duration_us: finished_us.saturating_sub(active.started_us),
            success,
            error: error.clone(),
            register_io: active.register_io,
        });
        if let Some(error) = error {
            state.snapshot.error = Some(error.clone());
            state.snapshot.duration_us =
                Some(finished_us.saturating_sub(state.snapshot.started_us));
            log::warn!(target: "openipc_rtl88xx::init", "initialization stage failed: {name}: {error}");
        } else {
            log::debug!(target: "openipc_rtl88xx::init", "initialization stage complete: {name} duration_us={}", finished_us.saturating_sub(started_us));
        }
        drop(state);
        result
    }

    pub(crate) fn record_probe_diagnostics(
        &self,
        sys_cfg: u32,
        sys_cfg2_chip_id: u8,
        chip: ChipInfo,
    ) {
        let descriptor = descriptor_for_chip(chip);
        let probe = ProbeDiagnostics {
            vendor_id: self.vendor_id,
            product_id: self.product_id,
            sys_cfg,
            sys_cfg2_chip_id,
            chip,
            rx_descriptor: descriptor,
        };
        self.diagnostics
            .lock()
            .expect("driver diagnostics poisoned")
            .snapshot
            .probe = Some(probe);
        log::info!(
            target: "openipc_rtl88xx::probe",
            "probe vid={:04x} pid={:04x} SYS_CFG=0x{sys_cfg:08x} SYS_CFG2=0x{sys_cfg2_chip_id:02x} family={} cut={} rf={:?} descriptor={descriptor:?}",
            self.vendor_id,
            self.product_id,
            chip.family.name(),
            chip.cut_version,
            chip.rf_type,
        );
    }

    pub(crate) fn record_efuse_diagnostics(&self, map: &[u8; 512], efuse: EfuseInfo) {
        let eeprom_id = u16::from_le_bytes([map[0], map[1]]);
        let summary = EfuseDiagnostics {
            eeprom_id,
            autoload_valid: eeprom_id == crate::REALTEK_EEPROM_ID,
            map_fingerprint: fnv1a(map),
            programmed_bytes: map.iter().filter(|byte| **byte != 0xff).count(),
            rfe_type: efuse.rfe_type,
            board_type: efuse.board_type,
            external_pa_2g: efuse.external_pa_2g,
            external_pa_5g: efuse.external_pa_5g,
            external_lna_2g: efuse.external_lna_2g,
            external_lna_5g: efuse.external_lna_5g,
            crystal_cap: efuse.crystal_cap,
            thermal_meter: efuse.thermal_meter,
            thermal_meter_paths: efuse.thermal_meter_paths,
            tx_power_defaults: efuse.tx_power_defaults,
            mac_present: efuse.mac.is_some(),
        };
        log::info!(
            target: "openipc_rtl88xx::efuse",
            "EFUSE id=0x{eeprom_id:04x} valid={} fingerprint=0x{:016x} programmed={} rfe={} board=0x{:02x} pa2g={} pa5g={} lna2g={} lna5g={} crystal={} thermal={:?} tx_power_defaults={}",
            summary.autoload_valid,
            summary.map_fingerprint,
            summary.programmed_bytes,
            summary.rfe_type,
            summary.board_type,
            summary.external_pa_2g,
            summary.external_pa_5g,
            summary.external_lna_2g,
            summary.external_lna_5g,
            summary.crystal_cap,
            summary.thermal_meter_paths,
            summary.tx_power_defaults,
        );
        self.diagnostics
            .lock()
            .expect("driver diagnostics poisoned")
            .snapshot
            .efuse = Some(summary);
    }

    pub(crate) fn record_register_read(&self, register: u16, bytes: &[u8]) {
        self.record_register_io(b'R', register, bytes, true, None);
    }

    pub(crate) fn record_register_write(&self, register: u16, bytes: &[u8]) {
        self.record_register_io(b'W', register, bytes, true, None);
    }

    pub(crate) fn record_register_failure(
        &self,
        operation: u8,
        register: u16,
        error: impl Into<String>,
    ) {
        self.record_register_io(operation, register, &[], false, Some(error.into()));
    }

    fn record_register_io(
        &self,
        operation: u8,
        register: u16,
        bytes: &[u8],
        success: bool,
        error: Option<String>,
    ) {
        let mut state = self
            .diagnostics
            .lock()
            .expect("driver diagnostics poisoned");
        state
            .snapshot
            .register_io
            .record(operation, register, bytes, success);
        if state.snapshot.register_trace.len() < MAX_REGISTER_TRACE_ENTRIES {
            let sequence = state.snapshot.register_trace.len() as u64;
            let stage = state.active_stage.as_ref().map(|stage| stage.name.clone());
            let started_us = state.snapshot.started_us;
            state.snapshot.register_trace.push(RegisterTraceEntry {
                sequence,
                offset_us: monotonic_micros().saturating_sub(started_us),
                stage,
                operation: if operation == b'R' { "read" } else { "write" },
                register,
                bytes: bytes.to_vec(),
                success,
                error,
            });
        } else {
            state.snapshot.register_trace_dropped =
                state.snapshot.register_trace_dropped.saturating_add(1);
        }
        if let Some(stage) = state.active_stage.as_mut() {
            stage
                .register_io
                .record(operation, register, bytes, success);
        }
    }

    pub(crate) async fn capture_post_init_registers(&self) {
        const REGISTERS: &[(&str, u16, u16)] = &[
            ("9346CR", 0x000a, 1),
            ("MCUFWDL", 0x0080, 4),
            ("SYS_CFG", 0x00f0, 4),
            ("SYS_CFG2", 0x00fc, 1),
            ("CR", 0x0100, 2),
            ("RXDMA_STATUS", 0x0288, 4),
            ("TXPAUSE", 0x0522, 1),
            ("RCR", 0x0608, 4),
            ("RXFLTMAP0", 0x06a0, 2),
            ("RXFLTMAP1", 0x06a2, 2),
            ("RXFLTMAP2", 0x06a4, 2),
            ("BB_RX_PATH", 0x0808, 4),
        ];
        let mut registers = Vec::with_capacity(REGISTERS.len());
        for &(name, address, width) in REGISTERS {
            match self.read_register_async(address, width).await {
                Ok(bytes) => registers.push(RegisterSnapshot {
                    name,
                    address,
                    width,
                    value: Some(format_le_hex(&bytes)),
                    error: None,
                }),
                Err(error) => registers.push(RegisterSnapshot {
                    name,
                    address,
                    width,
                    value: None,
                    error: Some(error.to_string()),
                }),
            }
        }
        self.diagnostics
            .lock()
            .expect("driver diagnostics poisoned")
            .snapshot
            .post_init_registers = registers;
    }

    pub(crate) fn finish_diagnostics(&self, result: Result<(), &DriverError>) {
        let now = monotonic_micros();
        let mut state = self
            .diagnostics
            .lock()
            .expect("driver diagnostics poisoned");
        state.snapshot.duration_us = Some(now.saturating_sub(state.snapshot.started_us));
        match result {
            Ok(()) => {
                state.snapshot.completed = true;
                state.snapshot.error = None;
            }
            Err(error) => state.snapshot.error = Some(error.to_string()),
        }
    }

    /// Return structured evidence for the most recent initialization attempt.
    pub fn diagnostics_snapshot(&self) -> DriverDiagnostics {
        self.diagnostics
            .lock()
            .expect("driver diagnostics poisoned")
            .snapshot
            .clone()
    }
}

pub(crate) const fn descriptor_for_chip(chip: ChipInfo) -> RxDescriptorKind {
    if chip.family.is_jaguar3() {
        RxDescriptorKind::Jaguar3
    } else if chip.family.is_jaguar2() {
        RxDescriptorKind::Jaguar2
    } else {
        RxDescriptorKind::Jaguar1
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(FNV_OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
    })
}

fn format_le_hex(bytes: &[u8]) -> String {
    let mut output = String::from("0x");
    for byte in bytes.iter().rev() {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_fingerprint_is_order_and_value_sensitive() {
        let mut first = RegisterIoDiagnostics::default();
        first.record(b'W', 0x0100, &[1, 2], true);
        let mut second = RegisterIoDiagnostics::default();
        second.record(b'W', 0x0100, &[2, 1], true);
        assert_ne!(first.fingerprint, second.fingerprint);
    }

    #[test]
    fn little_endian_registers_are_rendered_as_values() {
        assert_eq!(format_le_hex(&[0x78, 0x56, 0x34, 0x12]), "0x12345678");
    }
}
