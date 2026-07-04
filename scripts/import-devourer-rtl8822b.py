#!/usr/bin/env python3
"""Import RTL8822B firmware and PHY data from a devourer checkout.

The generated Rust file is checked in. Building openipc-rs never reads the
reference checkout; this script only makes future parity updates reproducible.
"""

from __future__ import annotations

import argparse
import re
from pathlib import Path


NUMERIC_ARRAYS = (
    ("hal/hal8822b_fw.c", "array_mp_8822b_fw_nic", "u8", "RTL8822B_FW_NIC"),
    (
        "hal/phydm/rtl8822b/Hal8822b_PhyTables.c",
        "array_mp_8822b_mac_reg",
        "u32",
        "RTL8822B_MAC_REG",
    ),
    (
        "hal/phydm/rtl8822b/Hal8822b_PhyTables.c",
        "array_mp_8822b_phy_reg",
        "u32",
        "RTL8822B_PHY_REG",
    ),
    (
        "hal/phydm/rtl8822b/Hal8822b_PhyTables.c",
        "array_mp_8822b_agc_tab",
        "u32",
        "RTL8822B_AGC_TAB",
    ),
    (
        "hal/phydm/rtl8822b/Hal8822b_PhyTables.c",
        "array_mp_8822b_radioa",
        "u32",
        "RTL8822B_RADIO_A",
    ),
    (
        "hal/phydm/rtl8822b/Hal8822b_PhyTables.c",
        "array_mp_8822b_radiob",
        "u32",
        "RTL8822B_RADIO_B",
    ),
)

LIMIT_ARRAYS = (
    ("mp_8822b_txpwr_lmt_ww", "RTL8822B_TX_POWER_LIMITS_WW"),
    ("mp_8822b_txpwr_lmt_type3_ww", "RTL8822B_TX_POWER_LIMITS_TYPE3_WW"),
)


def array_body(source: str, name: str) -> str:
    match = re.search(rf"\b{name}\s*\[\s*\]\s*=\s*\{{", source)
    if not match:
        raise ValueError(f"could not find {name}")
    start = match.end()
    end = source.find("};", start)
    if end < 0:
        raise ValueError(f"unterminated array {name}")
    body = re.sub(r"/\*.*?\*/", "", source[start:end], flags=re.S)
    return re.sub(r"//.*", "", body)


def numeric_values(source: str, name: str) -> list[int]:
    return [int(value, 0) for value in re.findall(r"0[xX][0-9a-fA-F]+|\b\d+\b", array_body(source, name))]


def limit_values(source: str, name: str) -> list[tuple[int, ...]]:
    rows = []
    for row in re.findall(r"\{([^{}]+)\}", array_body(source, name)):
        values = tuple(int(value.strip(), 0) for value in row.split(",") if value.strip())
        if len(values) != 6:
            raise ValueError(f"{name} row has {len(values)} values instead of 6")
        rows.append(values)
    return rows


def format_numeric(name: str, rust_type: str, values: list[int]) -> str:
    width = 2 if rust_type == "u8" else 8
    per_line = 12 if rust_type == "u8" else 6
    lines = [f"pub static {name}: &[{rust_type}] = &["]
    for offset in range(0, len(values), per_line):
        line = ", ".join(f"0x{value:0{width}x}" for value in values[offset : offset + per_line])
        lines.append(f"    {line},")
    lines.append("];\n")
    return "\n".join(lines)


def format_limits(name: str, rows: list[tuple[int, ...]]) -> str:
    lines = [f"pub static {name}: &[TxPowerLimit8822b] = &["]
    for band, bandwidth, section, streams, channel, limit in rows:
        lines.append(
            "    TxPowerLimit8822b { "
            f"band: {band}, bandwidth: {bandwidth}, section: {section}, "
            f"streams: {streams}, channel: {channel}, limit: {limit} "
            "},"
        )
    lines.append("];\n")
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("devourer", type=Path, help="path to a devourer checkout")
    parser.add_argument("output", type=Path, help="generated Rust output path")
    args = parser.parse_args()

    chunks = [
        "// Vendored RTL8822B/RTL8812BU Jaguar2 firmware and register tables.\n",
        "// Generated from OpenIPC/devourer; do not edit by hand.\n\n",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n",
        "pub struct TxPowerLimit8822b {\n",
        "    pub band: u8,\n",
        "    pub bandwidth: u8,\n",
        "    pub section: u8,\n",
        "    pub streams: u8,\n",
        "    pub channel: u8,\n",
        "    pub limit: i8,\n",
        "}\n\n",
    ]
    cache: dict[Path, str] = {}
    for relative, c_name, rust_type, rust_name in NUMERIC_ARRAYS:
        path = args.devourer / relative
        source = cache.setdefault(path, path.read_text())
        chunks.append(format_numeric(rust_name, rust_type, numeric_values(source, c_name)))

    limits_path = args.devourer / "hal/phydm/rtl8822b/Hal8822b_TxpwrLmt.h"
    limits_source = limits_path.read_text()
    for c_name, rust_name in LIMIT_ARRAYS:
        chunks.append(format_limits(rust_name, limit_values(limits_source, c_name)))

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text("".join(chunks))


if __name__ == "__main__":
    main()
