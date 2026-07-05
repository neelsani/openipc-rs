#!/usr/bin/env python3
"""Import RTL8821C firmware and PHY data from a devourer checkout.

The generated Rust file is checked in so normal builds remain standalone.
"""

from __future__ import annotations

import argparse
import re
from pathlib import Path


ARRAYS = (
    ("hal/hal8821c_fw.c", "array_mp_8821c_fw_nic", "u8", "RTL8821C_FW_NIC"),
    ("hal/phydm/rtl8821c/Hal8821c_PhyTables.c", "array_mp_8821c_mac_reg", "u32", "RTL8821C_MAC_REG"),
    ("hal/phydm/rtl8821c/Hal8821c_PhyTables.c", "array_mp_8821c_phy_reg", "u32", "RTL8821C_PHY_REG"),
    ("hal/phydm/rtl8821c/Hal8821c_PhyTables.c", "array_mp_8821c_agc_tab", "u32", "RTL8821C_AGC_TAB"),
    ("hal/phydm/rtl8821c/Hal8821c_PhyTables.c", "array_mp_8821c_radioa", "u32", "RTL8821C_RADIO_A"),
    ("hal/phydm/rtl8821c/Hal8821c_PhyTables.c", "array_mp_8821c_phy_reg_pg", "u32", "RTL8821C_PHY_REG_PG"),
)


def array_values(source: str, name: str) -> list[int]:
    match = re.search(rf"\b{name}\s*\[\s*\]\s*=\s*\{{", source)
    if not match:
        raise ValueError(f"could not find {name}")
    end = source.find("};", match.end())
    if end < 0:
        raise ValueError(f"unterminated array {name}")
    body = re.sub(r"/\*.*?\*/|//.*", "", source[match.end() : end], flags=re.S)
    return [int(value, 0) for value in re.findall(r"0[xX][0-9a-fA-F]+|\b\d+\b", body)]


def format_array(name: str, rust_type: str, values: list[int]) -> str:
    width = 2 if rust_type == "u8" else 8
    per_line = 12 if rust_type == "u8" else 6
    lines = [f"pub static {name}: &[{rust_type}] = &["]
    for offset in range(0, len(values), per_line):
        values_on_line = values[offset : offset + per_line]
        lines.append("    " + ", ".join(f"0x{value:0{width}x}" for value in values_on_line) + ",")
    lines.append("];\n")
    return "\n".join(lines)


def format_limits(source: str) -> str:
    match = re.search(r"\bmp_8821c_txpwr_lmt_ww\s*\[\s*\]\s*=\s*\{", source)
    if not match:
        raise ValueError("could not find mp_8821c_txpwr_lmt_ww")
    end = source.find("};", match.end())
    rows = re.findall(r"\{([^{}]+)\}", source[match.end() : end])
    output = [
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n",
        "pub struct TxPowerLimit8821c {\n",
        "    pub band: u8, pub bandwidth: u8, pub section: u8,\n",
        "    pub streams: u8, pub channel: u8, pub limit: i8,\n",
        "}\n\n",
        "pub static RTL8821C_TX_POWER_LIMITS_WW: &[TxPowerLimit8821c] = &[\n",
    ]
    for row in rows:
        values = [int(value.strip(), 0) for value in row.split(",") if value.strip()]
        if len(values) != 6:
            raise ValueError(f"TX power limit row has {len(values)} values")
        band, bandwidth, section, streams, channel, limit = values
        output.append(
            "    TxPowerLimit8821c { "
            f"band: {band}, bandwidth: {bandwidth}, section: {section}, "
            f"streams: {streams}, channel: {channel}, limit: {limit} "
            "},\n"
        )
    output.append("];\n")
    return "".join(output)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("devourer", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    chunks = [
        "// Vendored RTL8821C/RTL8811CU Jaguar2 firmware and register tables.\n",
        "// Generated from OpenIPC/devourer; do not edit by hand.\n\n",
    ]
    cache: dict[Path, str] = {}
    for relative, c_name, rust_type, rust_name in ARRAYS:
        path = args.devourer / relative
        source = cache.setdefault(path, path.read_text())
        chunks.append(format_array(rust_name, rust_type, array_values(source, c_name)))

    limits = args.devourer / "hal/phydm/rtl8821c/Hal8821c_TxpwrLmt.h"
    chunks.append(format_limits(limits.read_text()))

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text("".join(chunks))


if __name__ == "__main__":
    main()
