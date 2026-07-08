#!/usr/bin/env python3
"""Verify every vendored Realtek payload against a Devourer checkout.

This is a maintainer audit, not a build dependency. Normal openipc-rs builds
remain standalone. Symbol matching is deliberately exact so a name such as
``array_mp_8812a_phy_reg`` cannot resolve to ``array_mp_8812a_phy_reg_mp``.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "crates/openipc-rtl88xx/src/data"


@dataclass(frozen=True)
class NumericArray:
    label: str
    c_path: str
    c_name: str
    rust_path: str
    rust_name: str
    expected_len: int | None = None


ARRAYS = (
    NumericArray("RTL8812 firmware", "hal/hal8812a_fw.c", "array_mp_8812a_fw_nic", "rtl_reference_data.rs", "RTL8812_FW_NIC", 27_054),
    NumericArray("RTL8812 MAC", "src/jaguar1/HalModule.cpp", "array_mp_8812a_mac_reg", "rtl_reference_data.rs", "RTL8812_MAC_REG", 224),
    NumericArray("RTL8812 PHY", "src/jaguar1/HalModule.cpp", "array_mp_8812a_phy_reg", "rtl_reference_data.rs", "RTL8812_PHY_REG", 470),
    NumericArray("RTL8812 AGC", "src/jaguar1/HalModule.cpp", "array_mp_8812a_agc_tab", "rtl_reference_data.rs", "RTL8812_AGC_TAB", 668),
    NumericArray("RTL8812 radio A", "src/jaguar1/HalModule.cpp", "array_mp_8812a_radioa", "rtl_reference_data.rs", "RTL8812_RADIO_A", 864),
    NumericArray("RTL8812 radio B", "src/jaguar1/HalModule.cpp", "array_mp_8812a_radiob", "rtl_reference_data.rs", "RTL8812_RADIO_B", 848),
    NumericArray("RTL8821 firmware", "hal/hal8821a_fw.c", "array_mp_8821a_fw_nic", "rtl_reference_data.rs", "RTL8821_FW_NIC", 31_834),
    NumericArray("RTL8821 MAC", "hal/Hal8821PhyReg.h", "array_mp_8821a_mac_reg", "rtl_reference_data.rs", "RTL8821_MAC_REG", 196),
    NumericArray("RTL8821 PHY", "hal/Hal8821PhyReg.h", "array_mp_8821a_phy_reg", "rtl_reference_data.rs", "RTL8821_PHY_REG", 344),
    NumericArray("RTL8821 AGC", "hal/Hal8821PhyReg.h", "array_mp_8821a_agc_tab", "rtl_reference_data.rs", "RTL8821_AGC_TAB", 504),
    NumericArray("RTL8821 radio A", "hal/Hal8821PhyReg.h", "array_mp_8821a_radioa", "rtl_reference_data.rs", "RTL8821_RADIO_A", 1_734),
    NumericArray("RTL8814 firmware", "hal/hal8814a_fw.c", "array_mp_8814a_fw_nic", "rtl_reference_data.rs", "RTL8814_FW_NIC", 68_320),
    NumericArray("RTL8814 MAC", "hal/phydm/rtl8814a/Hal8814_PhyTables.c", "array_mp_8814a_mac_reg", "rtl_reference_data.rs", "RTL8814_MAC_REG", 286),
    NumericArray("RTL8814 PHY", "hal/phydm/rtl8814a/Hal8814_PhyTables.c", "array_mp_8814a_phy_reg", "rtl_reference_data.rs", "RTL8814_PHY_REG", 4_622),
    NumericArray("RTL8814 AGC", "hal/phydm/rtl8814a/Hal8814_PhyTables.c", "array_mp_8814a_agc_tab", "rtl_reference_data.rs", "RTL8814_AGC_TAB", 6_280),
    NumericArray("RTL8814 radio A", "hal/phydm/rtl8814a/Hal8814_PhyTables.c", "array_mp_8814a_radioa", "rtl_reference_data.rs", "RTL8814_RADIO_A", 4_634),
    NumericArray("RTL8814 radio B", "hal/phydm/rtl8814a/Hal8814_PhyTables.c", "array_mp_8814a_radiob", "rtl_reference_data.rs", "RTL8814_RADIO_B", 4_396),
    NumericArray("RTL8814 radio C", "hal/phydm/rtl8814a/Hal8814_PhyTables.c", "array_mp_8814a_radioc", "rtl_reference_data.rs", "RTL8814_RADIO_C", 4_524),
    NumericArray("RTL8814 radio D", "hal/phydm/rtl8814a/Hal8814_PhyTables.c", "array_mp_8814a_radiod", "rtl_reference_data.rs", "RTL8814_RADIO_D", 4_600),
    NumericArray("RTL8822C firmware", "hal/hal8822c_fw.c", "array_mp_8822c_fw_nic", "rtl8822c_reference_data.rs", "RTL8822C_FW_NIC", 200_624),
    NumericArray("RTL8822C AGC", "hal/phydm/rtl8822c/Hal8822c_PhyTables.c", "array_mp_8822c_agc_tab", "rtl8822c_reference_data.rs", "RTL8822C_AGC_TAB", 3_734),
    NumericArray("RTL8822C PHY", "hal/phydm/rtl8822c/Hal8822c_PhyTables.c", "array_mp_8822c_phy_reg", "rtl8822c_reference_data.rs", "RTL8822C_PHY_REG", 3_020),
    NumericArray("RTL8822C radio A", "hal/phydm/rtl8822c/Hal8822c_PhyTables.c", "array_mp_8822c_radioa", "rtl8822c_reference_data.rs", "RTL8822C_RADIO_A", 40_130),
    NumericArray("RTL8822C radio B", "hal/phydm/rtl8822c/Hal8822c_PhyTables.c", "array_mp_8822c_radiob", "rtl8822c_reference_data.rs", "RTL8822C_RADIO_B", 40_766),
    NumericArray("RTL8822C calibration", "hal/phydm/rtl8822c/Hal8822c_PhyTables.c", "array_mp_8822c_cal_init", "rtl8822c_reference_data.rs", "RTL8822C_CAL_INIT", 4_928),
    NumericArray("RTL8822C IQK NCTL", "hal/phydm/rtl8822c/Hal8822c_IqkNctl.c", "array_mp_8822c_iqk_nctl", "rtl8822c_reference_data.rs", "RTL8822C_IQK_NCTL", 5_403),
    NumericArray("RTL8822E firmware", "hal/hal8822e_fw.c", "array_mp_8822e_fw_nic", "rtl8822e_reference_data.rs", "RTL8822E_FW_NIC", 199_928),
    NumericArray("RTL8822E AGC", "hal/phydm/rtl8822e/Hal8822e_PhyTables.c", "array_mp_8822e_agc_tab", "rtl8822e_reference_data.rs", "RTL8822E_AGC_TAB", 14_628),
    NumericArray("RTL8822E PHY", "hal/phydm/rtl8822e/Hal8822e_PhyTables.c", "array_mp_8822e_phy_reg", "rtl8822e_reference_data.rs", "RTL8822E_PHY_REG", 3_082),
    NumericArray("RTL8822E PHY PG", "hal/phydm/rtl8822e/Hal8822e_PhyTables.c", "array_mp_8822e_phy_reg_pg", "rtl8822e_reference_data.rs", "RTL8822E_PHY_REG_PG", 276),
    NumericArray("RTL8822E PHY PG type 5", "hal/phydm/rtl8822e/Hal8822e_PhyTables.c", "array_mp_8822e_phy_reg_pg_type5", "rtl8822e_reference_data.rs", "RTL8822E_PHY_REG_PG_TYPE5", 276),
    NumericArray("RTL8822E radio A", "hal/phydm/rtl8822e/Hal8822e_PhyTables.c", "array_mp_8822e_radioa", "rtl8822e_reference_data.rs", "RTL8822E_RADIO_A", 10_622),
    NumericArray("RTL8822E radio B", "hal/phydm/rtl8822e/Hal8822e_PhyTables.c", "array_mp_8822e_radiob", "rtl8822e_reference_data.rs", "RTL8822E_RADIO_B", 12_050),
    NumericArray("RTL8822E calibration", "hal/phydm/rtl8822e/Hal8822e_PhyTables.c", "array_mp_8822e_cal_init", "rtl8822e_reference_data.rs", "RTL8822E_CAL_INIT", 5_222),
)


def without_comments(source: str) -> str:
    source = re.sub(r"/\*.*?\*/", "", source, flags=re.S)
    return re.sub(r"//[^\n]*", "", source)


def balanced_body(source: str, match: re.Match[str], opening: str, closing: str) -> str:
    start = match.end()
    depth = 1
    for index in range(start, len(source)):
        if source[index] == opening:
            depth += 1
        elif source[index] == closing:
            depth -= 1
            if depth == 0:
                return source[start:index]
    raise ValueError(f"unterminated array after offset {match.start()}")


def c_array_body(source: str, name: str) -> str:
    clean = without_comments(source)
    match = re.search(rf"\b{re.escape(name)}\s*\[\s*(?:\d+)?\s*\]\s*=\s*\{{", clean)
    if not match:
        raise ValueError(f"could not find exact C array {name}")
    return balanced_body(clean, match, "{", "}")


def rust_array_body(source: str, name: str) -> str:
    clean = without_comments(source)
    match = re.search(rf"\b{re.escape(name)}\s*:[^=]+?=\s*&\[", clean)
    if not match:
        raise ValueError(f"could not find exact Rust array {name}")
    return balanced_body(clean, match, "[", "]")


def numbers(body: str) -> list[int]:
    return [int(value, 0) for value in re.findall(r"(?<![\w])-?(?:0[xX][0-9a-fA-F]+|\d+)", body)]


def assert_exact_symbol_matching() -> None:
    fixture = "uint32_t target_mp[] = { 1, 2 }; uint32_t target[] = { 3, 4 };"
    assert numbers(c_array_body(fixture, "target")) == [3, 4]
    rust = "pub static TARGET_MP: &[u32] = &[1, 2]; pub static TARGET: &[u32] = &[3, 4];"
    assert numbers(rust_array_body(rust, "TARGET")) == [3, 4]


def compare_numeric_arrays(devourer: Path) -> None:
    c_cache: dict[Path, str] = {}
    rust_cache: dict[Path, str] = {}
    for item in ARRAYS:
        c_path = devourer / item.c_path
        rust_path = DATA / item.rust_path
        c_source = c_cache.setdefault(c_path, c_path.read_text())
        rust_source = rust_cache.setdefault(rust_path, rust_path.read_text())
        expected = numbers(c_array_body(c_source, item.c_name))
        actual = numbers(rust_array_body(rust_source, item.rust_name))
        if item.expected_len is not None and len(actual) != item.expected_len:
            raise ValueError(
                f"{item.label}: expected reviewed length {item.expected_len}, got {len(actual)}"
            )
        if actual != expected:
            mismatch = next(
                (index for index, pair in enumerate(zip(actual, expected)) if pair[0] != pair[1]),
                min(len(actual), len(expected)),
            )
            raise ValueError(
                f"{item.label}: mismatch at scalar {mismatch}; "
                f"Rust length={len(actual)}, Devourer length={len(expected)}"
            )
        print(f"PASS  {item.label:<29} {len(actual):>7} scalars")


def compare_generated_jaguar2(devourer: Path) -> None:
    generators = (
        ("RTL8822B generated payload", "import-devourer-rtl8822b.py", "rtl8822b_reference_data.rs"),
        ("RTL8821C generated payload", "import-devourer-rtl8821c.py", "rtl8821c_reference_data.rs"),
    )
    with tempfile.TemporaryDirectory(prefix="openipc-rs-audit-") as directory:
        temp = Path(directory)
        for label, script, checked_in in generators:
            output = temp / checked_in
            subprocess.run(
                [sys.executable, str(ROOT / "scripts" / script), str(devourer), str(output)],
                check=True,
            )
            expected = (DATA / checked_in).read_bytes()
            actual = output.read_bytes()
            if actual != expected:
                raise ValueError(f"{label}: regenerated file differs from {checked_in}")
            print(f"PASS  {label:<29} byte-identical")


def compare_8812_power_tables(devourer: Path) -> None:
    c_pg = numbers(c_array_body((devourer / "hal/Hal8812a_PhyRegPg.h").read_text(), "kHal8812aPhyRegPg"))
    rust_source = (DATA / "rtl8812_tx_power_tables.rs").read_text()
    rust_pg = numbers(rust_array_body(rust_source, "RTL8812A_PHY_REG_PG"))
    if c_pg != rust_pg:
        raise ValueError("RTL8812 PHY PG table differs from Devourer")
    print(f"PASS  {'RTL8812 PHY PG':<29} {len(rust_pg) // 6:>7} rows")

    c_body = c_array_body((devourer / "hal/Hal8812a_TxpwrLmt.h").read_text(), "kHal8812aTxpwrLmt")
    strings = re.findall(r'"([^\"]+)"', c_body)
    if len(strings) % 7:
        raise ValueError("RTL8812 C TX-power limit table is not a multiple of seven")
    regulation = {"FCC": "Fcc", "ETSI": "Etsi", "MKK": "Mkk", "WW": "Worldwide"}
    band = {"2.4G": "Ghz2", "5G": "Ghz5"}
    bandwidth = {"20M": "Mhz20", "40M": "Mhz40", "80M": "Mhz80", "160M": "Mhz160"}
    c_rows = []
    for offset in range(0, len(strings), 7):
        reg, freq, width, section, ntx, channel, limit = strings[offset : offset + 7]
        c_rows.append((regulation[reg], band[freq], bandwidth[width], section.title(), int(ntx[:-1]) - 1, int(channel), int(limit)))

    rust_rows = []
    limits_body = rust_array_body(rust_source, "RTL8812A_TX_POWER_LIMITS")
    for row in re.findall(r"TxPowerLimitRow\s*\{([^}]+)\}", limits_body):
        def field(pattern: str) -> str:
            match = re.search(pattern, row)
            if not match:
                raise ValueError(f"could not parse RTL8812 TX-power row field: {pattern}")
            return match.group(1)

        rust_rows.append((
            field(r"regulation:\s*TxPowerRegulation::(\w+)"),
            field(r"band:\s*TxPowerBand::(\w+)"),
            field(r"bandwidth:\s*TxPowerLimitBandwidth::(\w+)"),
            field(r"rate_section:\s*TxPowerLimitRateSection::(\w+)"),
            int(field(r"ntx_idx:\s*(\d+)")),
            int(field(r"channel:\s*(\d+)")),
            int(field(r"limit:\s*(-?\d+)")),
        ))
    if c_rows != rust_rows:
        raise ValueError("RTL8812 TX-power limits differ from Devourer")
    print(f"PASS  {'RTL8812 TX-power limits':<29} {len(rust_rows):>7} rows")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("devourer", type=Path, help="path to a current Devourer checkout")
    args = parser.parse_args()
    devourer = args.devourer.resolve()
    if not (devourer / "src/jaguar1/HalModule.cpp").is_file():
        parser.error(f"not a Devourer checkout: {devourer}")

    assert_exact_symbol_matching()
    compare_numeric_arrays(devourer)
    compare_generated_jaguar2(devourer)
    compare_8812_power_tables(devourer)
    print(f"\nAll vendored Realtek reference payloads match {devourer}.")


if __name__ == "__main__":
    main()
