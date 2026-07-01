#![allow(dead_code)]

use std::fs;
use std::path::Path;

#[cfg(not(target_os = "android"))]
use nusb::MaybeFuture;
use openipc_core::{
    ChannelBandwidth, ChannelId, FrameLayout, RadioPort, TxMode, TxModeKind, TxRadioParams,
    WfbKeypair, WfbTxKeypair, FRAME_TYPE_DATA, FRAME_TYPE_RTS,
};
use openipc_rtl88xx::{
    is_supported_id, ChannelWidth, ChipFamily, DriverOptions, Firmware8814Mode, MonitorOptions,
    RadioConfig, RealtekDevice, RealtekTxDescriptor, RealtekTxOptions,
};

pub type CliResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Clone)]
pub struct RadioDeviceConfig {
    pub initialize_hardware: bool,
    pub driver_options: DriverOptions,
    pub monitor_options: MonitorOptions,
    pub radio: RadioConfig,
    pub tx_legacy_8812_descriptor: bool,
}

impl Default for RadioDeviceConfig {
    fn default() -> Self {
        Self {
            initialize_hardware: true,
            driver_options: DriverOptions::from_env(),
            monitor_options: MonitorOptions::from_env(),
            radio: RadioConfig::default(),
            tx_legacy_8812_descriptor: std::env::var_os("DEVOURER_TX_LEGACY_8812_DESC").is_some(),
        }
    }
}

pub struct OpenedRadio {
    pub device: RealtekDevice,
    pub chip_family: ChipFamily,
}

pub fn open_radio(config: &RadioDeviceConfig) -> CliResult<OpenedRadio> {
    let mut driver_options = config.driver_options;
    driver_options.initialize_hardware = config.initialize_hardware;
    let device = RealtekDevice::open_first(driver_options)?;
    open_claimed_radio(config, device)
}

pub fn open_radios(config: &RadioDeviceConfig) -> CliResult<Vec<OpenedRadio>> {
    #[cfg(target_os = "android")]
    {
        let _ = config;
        return Err("Android USB discovery must use UsbManager and nusb::Device::from_fd".into());
    }

    #[cfg(not(target_os = "android"))]
    {
        let mut driver_options = config.driver_options;
        driver_options.initialize_hardware = config.initialize_hardware;
        let infos: Vec<_> = nusb::list_devices()
            .wait()
            .map_err(|err| format!("list_devices failed: {err}"))?
            .filter(|device| {
                device_matches_options(device.vendor_id(), device.product_id(), driver_options)
            })
            .collect();

        if infos.is_empty() {
            return Err("no supported Realtek adapters found".into());
        }

        let mut opened = Vec::new();
        let mut errors = Vec::new();
        for info in infos {
            let vendor_id = info.vendor_id();
            let product_id = info.product_id();
            match info.open().wait() {
                Ok(device) => match RealtekDevice::from_nusb_device(device, driver_options) {
                    Ok(device) => match open_claimed_radio(config, device) {
                        Ok(radio) => opened.push(radio),
                        Err(err) => errors.push(format!("{vendor_id:04x}:{product_id:04x}: {err}")),
                    },
                    Err(err) => errors.push(format!("{vendor_id:04x}:{product_id:04x}: {err}")),
                },
                Err(err) => errors.push(format!("{vendor_id:04x}:{product_id:04x}: {err}")),
            }
        }

        if opened.is_empty() {
            return Err(format!(
                "no matching Realtek adapters could be opened: {}",
                errors.join("; ")
            )
            .into());
        }

        Ok(opened)
    }
}

#[cfg(not(target_os = "android"))]
fn device_matches_options(vendor_id: u16, product_id: u16, options: DriverOptions) -> bool {
    match (options.target_vendor_id, options.target_product_id) {
        (Some(vid), Some(pid)) => vendor_id == vid && product_id == pid,
        (Some(vid), None) => vendor_id == vid && is_supported_id(vendor_id, product_id),
        (None, Some(pid)) => product_id == pid && is_supported_id(vendor_id, product_id),
        (None, None) => is_supported_id(vendor_id, product_id),
    }
}

fn open_claimed_radio(config: &RadioDeviceConfig, device: RealtekDevice) -> CliResult<OpenedRadio> {
    let chip = device.probe_chip()?;

    eprintln!(
        "claimed Realtek adapter speed={:?} bulk_in=0x{:02x} bulk_out=0x{:02x}",
        device.device_speed(),
        device.bulk_in_ep,
        device.bulk_out_ep
    );

    if config.initialize_hardware {
        let report =
            device.initialize_monitor_with_options(config.radio, config.monitor_options)?;
        eprintln!(
            "Realtek init: chip={} rf_paths={} cut={} status={:?} firmware_downloaded={}",
            report.chip.family.name(),
            report.chip.total_rf_paths(),
            report.chip.cut_version,
            report.status,
            report.firmware_downloaded
        );
    } else {
        eprintln!("Realtek init skipped");
    }

    Ok(OpenedRadio {
        device,
        chip_family: chip.family,
    })
}

pub fn tx_options(config: &RadioDeviceConfig, chip_family: ChipFamily) -> RealtekTxOptions {
    RealtekTxOptions {
        current_channel: config.radio.channel,
        descriptor: RealtekTxDescriptor::for_chip_family(chip_family),
        legacy_8812_descriptor: config.tx_legacy_8812_descriptor,
        ..RealtekTxOptions::default()
    }
}

pub fn parse_common_radio_option(
    arg: &str,
    args: &mut std::iter::Peekable<impl Iterator<Item = String>>,
    config: &mut RadioDeviceConfig,
) -> CliResult<bool> {
    match arg {
        "--vid" => {
            config.driver_options.target_vendor_id = Some(parse_u16(&next_arg(args, "--vid")?)?);
        }
        "--pid" => {
            config.driver_options.target_product_id = Some(parse_u16(&next_arg(args, "--pid")?)?);
        }
        "--tx-ep" => {
            config.driver_options.tx_endpoint_override =
                Some(parse_u8(&next_arg(args, "--tx-ep")?)?);
        }
        "--skip-reset" => config.driver_options.skip_reset = true,
        "--no-init" => config.initialize_hardware = false,
        "--accept-bad-fcs" => config.monitor_options.accept_bad_fcs = true,
        "--skip-txpwr" => config.monitor_options.skip_tx_power = true,
        "--force-iqk" => config.monitor_options.force_iqk = true,
        "--disable-iqk" => config.monitor_options.disable_iqk = true,
        "--fwdl-8814" => {
            let mode = next_arg(args, "--fwdl-8814")?;
            config.monitor_options.firmware_8814_mode = Firmware8814Mode::from_env_value(&mode)
                .ok_or_else(|| {
                    format!("unsupported --fwdl-8814 value {mode}; expected kernel or rtw88")
                })?;
        }
        "--fwdl-8814-chunk" => {
            config.monitor_options.firmware_8814_chunk =
                Some(parse_u64(&next_arg(args, "--fwdl-8814-chunk")?)? as usize);
        }
        "--tx-legacy-8812-desc" => config.tx_legacy_8812_descriptor = true,
        "--rf-channel" => config.radio.channel = parse_u8(&next_arg(args, "--rf-channel")?)?,
        "--rf-width" => {
            config.radio.channel_width = parse_channel_width(&next_arg(args, "--rf-width")?)?;
        }
        "--rf-offset" => config.radio.channel_offset = parse_u8(&next_arg(args, "--rf-offset")?)?,
        _ => return Ok(false),
    }
    Ok(true)
}

pub fn channel_id_from_parts(link_id: u32, radio_port: u8) -> ChannelId {
    ChannelId::from_link_port(link_id & 0x00ff_ffff, RadioPort::Custom(radio_port))
}

pub fn frame_layout() -> FrameLayout {
    FrameLayout::WithFcs
}

pub fn load_rx_keypair(path: &Path) -> CliResult<WfbKeypair> {
    Ok(WfbKeypair::from_bytes(&fs::read(path)?)?)
}

pub fn load_tx_keypair(path: &Path) -> CliResult<WfbTxKeypair> {
    Ok(WfbTxKeypair::from_bytes(&fs::read(path)?)?)
}

pub fn radio_params_from_mode(mode: TxMode, frame_type: u8) -> TxRadioParams {
    match mode.kind {
        TxModeKind::Legacy => TxRadioParams {
            frame_type,
            ..TxRadioParams::default()
        },
        TxModeKind::Ht => TxRadioParams {
            mcs_index: mode.ht_mcs,
            nss: 1,
            bandwidth: mode.bandwidth,
            short_gi: mode.short_gi,
            stbc: u8::from(mode.stbc),
            ldpc: mode.ldpc,
            vht: false,
            frame_type,
        },
        TxModeKind::Vht => TxRadioParams {
            mcs_index: mode.vht_mcs,
            nss: mode.vht_nss,
            bandwidth: mode.bandwidth,
            short_gi: mode.short_gi,
            stbc: u8::from(mode.stbc),
            ldpc: mode.ldpc,
            vht: true,
            frame_type,
        },
    }
}

pub fn parse_tx_mode_flags(
    bandwidth_mhz: u16,
    short_gi: bool,
    stbc: bool,
    ldpc: bool,
    mcs_index: u8,
    vht_nss: u8,
    vht_mode: bool,
) -> TxMode {
    let bandwidth = match bandwidth_mhz {
        40 => ChannelBandwidth::Mhz40,
        80 => ChannelBandwidth::Mhz80,
        160 => ChannelBandwidth::Mhz160,
        _ => ChannelBandwidth::Mhz20,
    };
    let mut mode = if vht_mode || bandwidth_mhz >= 80 {
        TxMode::vht(vht_nss.max(1), mcs_index)
    } else {
        TxMode::ht(mcs_index)
    };
    mode.bandwidth = bandwidth;
    mode.short_gi = short_gi;
    mode.stbc = stbc;
    mode.ldpc = ldpc;
    mode
}

pub fn parse_frame_type(value: &str) -> CliResult<u8> {
    match value {
        "data" => Ok(FRAME_TYPE_DATA),
        "rts" => Ok(FRAME_TYPE_RTS),
        _ => Err(format!("invalid frame type {value}; expected data or rts").into()),
    }
}

pub fn next_arg(
    args: &mut std::iter::Peekable<impl Iterator<Item = String>>,
    option: &str,
) -> CliResult<String> {
    args.next()
        .ok_or_else(|| format!("{option} requires a value").into())
}

pub fn parse_u32(value: &str) -> CliResult<u32> {
    Ok(
        if let Some(hex) = value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
        {
            u32::from_str_radix(hex, 16)?
        } else {
            value.parse()?
        },
    )
}

pub fn parse_u64(value: &str) -> CliResult<u64> {
    Ok(
        if let Some(hex) = value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
        {
            u64::from_str_radix(hex, 16)?
        } else {
            value.parse()?
        },
    )
}

pub fn parse_u16(value: &str) -> CliResult<u16> {
    let parsed = parse_u32(value)?;
    Ok(u16::try_from(parsed).map_err(|_| format!("{value} is outside u16 range"))?)
}

pub fn parse_u8(value: &str) -> CliResult<u8> {
    let parsed = parse_u32(value)?;
    Ok(u8::try_from(parsed).map_err(|_| format!("{value} is outside u8 range"))?)
}

pub fn parse_channel_width(value: &str) -> CliResult<ChannelWidth> {
    match value {
        "5" => Ok(ChannelWidth::Mhz5),
        "10" => Ok(ChannelWidth::Mhz10),
        "20" => Ok(ChannelWidth::Mhz20),
        "40" => Ok(ChannelWidth::Mhz40),
        "80" => Ok(ChannelWidth::Mhz80),
        _ => {
            Err(format!("unsupported channel width {value}; expected 5, 10, 20, 40, or 80").into())
        }
    }
}

pub fn usage_common_radio() -> &'static str {
    "--vid <id> --pid <id> --tx-ep <ep> --skip-reset --no-init --rf-channel <n> --rf-width <5|10|20|40|80> --rf-offset <n>"
}
