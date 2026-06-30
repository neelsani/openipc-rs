use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use openipc_core::wfb::{MAX_FEC_PAYLOAD, WPACKET_HDR_LEN};
use openipc_core::{FecCode, PlainAssembler};

type ReferenceVectors = BTreeMap<(String, String), Vec<u8>>;

#[derive(Debug, Clone, Copy)]
struct CaseSpec {
    name: &'static str,
    k: usize,
    n: usize,
    block_size: usize,
    seed: u32,
    missing: &'static [usize],
    wfb_mode: bool,
}

const HARNESS_C: &str = r#"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "zfex.h"

static void *xaligned(size_t size) {
    void *ptr = NULL;
    if (posix_memalign(&ptr, ZFEX_SIMD_ALIGNMENT, ZFEX_ROUND_UP_SIMD(size)) != 0) {
        return NULL;
    }
    memset(ptr, 0, ZFEX_ROUND_UP_SIMD(size));
    return ptr;
}

static uint8_t pattern(unsigned seed, unsigned fragment, size_t offset) {
    return (uint8_t)((seed + fragment * 31u + offset * 17u + (offset % 251u)) & 0xffu);
}

static void fill_raw(uint8_t *fragment, size_t block_size, unsigned seed, unsigned fragment_idx) {
    for (size_t i = 0; i < block_size; i++) {
        fragment[i] = pattern(seed, fragment_idx, i);
    }
}

static void fill_wfb_plain(uint8_t *fragment, size_t block_size, unsigned fragment_idx) {
    static const char *payloads[] = {"alpha", "bravo", "charlie"};
    const char *payload = payloads[fragment_idx % 3u];
    size_t len = strlen(payload);
    memset(fragment, 0, block_size);
    fragment[0] = 0;
    fragment[1] = (uint8_t)((len >> 8) & 0xffu);
    fragment[2] = (uint8_t)(len & 0xffu);
    memcpy(fragment + 3, payload, len);
}

static void print_hex(const uint8_t *bytes, size_t len) {
    static const char lut[] = "0123456789abcdef";
    for (size_t i = 0; i < len; i++) {
        putchar(lut[bytes[i] >> 4]);
        putchar(lut[bytes[i] & 0x0f]);
    }
}

static int is_missing(const unsigned *missing, unsigned missing_count, unsigned idx) {
    for (unsigned i = 0; i < missing_count; i++) {
        if (missing[i] == idx) {
            return 1;
        }
    }
    return 0;
}

static int run_case(const char *name, unsigned k, unsigned n, size_t block_size, unsigned seed,
                    int wfb_mode, const unsigned *missing, unsigned missing_count) {
    fec_t *fec = NULL;
    if (fec_new((uint16_t)k, (uint16_t)n, &fec) != ZFEX_SC_OK || fec == NULL) {
        fprintf(stderr, "fec_new failed for %s\n", name);
        return 1;
    }

    uint8_t **blocks = calloc(n, sizeof(uint8_t *));
    if (blocks == NULL) {
        return 1;
    }
    for (unsigned i = 0; i < n; i++) {
        blocks[i] = xaligned(block_size);
        if (blocks[i] == NULL) {
            return 1;
        }
        if (i < k) {
            if (wfb_mode) {
                fill_wfb_plain(blocks[i], block_size, i);
            } else {
                fill_raw(blocks[i], block_size, seed, i);
            }
        }
    }

    if (fec_encode_simd((const fec_t *)fec, (const gf **)blocks, (gf **)(blocks + k), block_size) != ZFEX_SC_OK) {
        fprintf(stderr, "fec_encode_simd failed for %s\n", name);
        return 1;
    }

    printf("%s parity ", name);
    for (unsigned i = k; i < n; i++) {
        print_hex(blocks[i], block_size);
    }
    putchar('\n');

    uint8_t **in_blocks = calloc(k, sizeof(uint8_t *));
    uint8_t **out_blocks = calloc(missing_count ? missing_count : 1u, sizeof(uint8_t *));
    unsigned *index = calloc(k, sizeof(unsigned));
    if (in_blocks == NULL || out_blocks == NULL || index == NULL) {
        return 1;
    }

    unsigned parity_idx = k;
    unsigned out_idx = 0;
    for (unsigned i = 0; i < k; i++) {
        if (!is_missing(missing, missing_count, i)) {
            in_blocks[i] = blocks[i];
            index[i] = i;
        } else {
            while (parity_idx < n && is_missing(missing, missing_count, parity_idx)) {
                parity_idx++;
            }
            if (parity_idx >= n) {
                fprintf(stderr, "not enough parity in %s\n", name);
                return 1;
            }
            in_blocks[i] = blocks[parity_idx];
            index[i] = parity_idx;
            memset(blocks[i], 0, block_size);
            out_blocks[out_idx++] = blocks[i];
            parity_idx++;
        }
    }

    if (fec_decode_simd((const fec_t *)fec, (const gf **)in_blocks, (gf **)out_blocks, index, block_size) != ZFEX_SC_OK) {
        fprintf(stderr, "fec_decode_simd failed for %s\n", name);
        return 1;
    }

    printf("%s recovered ", name);
    for (unsigned i = 0; i < missing_count; i++) {
        print_hex(blocks[missing[i]], block_size);
    }
    putchar('\n');

    for (unsigned i = 0; i < n; i++) {
        free(blocks[i]);
    }
    free(blocks);
    free(in_blocks);
    free(out_blocks);
    free(index);
    fec_free(fec);
    return 0;
}

int main(void) {
    const unsigned small_missing[] = {1};
    const unsigned multi_missing[] = {4, 5, 6, 7};
    const unsigned wfb_missing[] = {1};

    if (run_case("small", 3, 5, 64, 17, 0, small_missing, 1) != 0) {
        return 1;
    }
    if (run_case("multi", 8, 12, 271, 93, 0, multi_missing, 4) != 0) {
        return 1;
    }
    if (run_case("wfb", 3, 5, OPENIPC_MAX_FEC_PAYLOAD, 0, 1, wfb_missing, 1) != 0) {
        return 1;
    }
    return 0;
}
"#;

#[test]
#[ignore]
fn pixelpilot_zfex_matches_rust_fec_and_wfb_assembler() -> Result<(), Box<dyn Error>> {
    let pixelpilot = env::var("OPENIPC_PIXELPILOT_REF")
        .map(PathBuf::from)
        .map_err(|_| "set OPENIPC_PIXELPILOT_REF to the PixelPilot checkout")?;
    let wfb_src = pixelpilot.join("app/wfbngrtl8812/src/main/cpp/wfb-ng/src");
    if !wfb_src.join("zfex.c").is_file() {
        return Err(format!(
            "PixelPilot wfb-ng source not found at {}",
            wfb_src.display()
        )
        .into());
    }

    let output = compile_and_run_reference(&wfb_src)?;
    let reference = parse_reference_output(&output)?;

    assert_case_matches_reference(
        &reference,
        CaseSpec {
            name: "small",
            k: 3,
            n: 5,
            block_size: 64,
            seed: 17,
            missing: &[1],
            wfb_mode: false,
        },
    )?;
    assert_case_matches_reference(
        &reference,
        CaseSpec {
            name: "multi",
            k: 8,
            n: 12,
            block_size: 271,
            seed: 93,
            missing: &[4, 5, 6, 7],
            wfb_mode: false,
        },
    )?;
    assert_case_matches_reference(
        &reference,
        CaseSpec {
            name: "wfb",
            k: 3,
            n: 5,
            block_size: MAX_FEC_PAYLOAD,
            seed: 0,
            missing: &[1],
            wfb_mode: true,
        },
    )?;
    assert_plain_assembler_recovers_with_pixelpilot_parity(&reference)?;

    Ok(())
}

fn compile_and_run_reference(wfb_src: &Path) -> Result<String, Box<dyn Error>> {
    let tmp = env::temp_dir().join(format!(
        "openipc-pixelpilot-reference-{}",
        std::process::id()
    ));
    fs::create_dir_all(&tmp)?;
    let harness = tmp.join("pixelpilot_zfex_reference.c");
    let exe = tmp.join("pixelpilot_zfex_reference");
    fs::write(&harness, HARNESS_C)?;

    let compiler = env::var("CC").unwrap_or_else(|_| "cc".to_owned());
    let status = Command::new(&compiler)
        .arg("-std=c11")
        .arg("-O2")
        .arg("-D_POSIX_C_SOURCE=200112L")
        .arg(format!("-DOPENIPC_MAX_FEC_PAYLOAD={MAX_FEC_PAYLOAD}"))
        .arg("-I")
        .arg(wfb_src)
        .arg(&harness)
        .arg(wfb_src.join("zfex.c"))
        .arg("-o")
        .arg(&exe)
        .status()?;
    if !status.success() {
        return Err(format!("failed to compile PixelPilot zfex harness with {compiler}").into());
    }

    let output = Command::new(&exe).output()?;
    if !output.status.success() {
        return Err(format!(
            "PixelPilot zfex harness failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    Ok(String::from_utf8(output.stdout)?)
}

fn parse_reference_output(output: &str) -> Result<ReferenceVectors, Box<dyn Error>> {
    let mut parsed = BTreeMap::new();
    for line in output.lines() {
        let mut parts = line.split_whitespace();
        let Some(case) = parts.next() else {
            continue;
        };
        let Some(kind) = parts.next() else {
            return Err(format!("bad reference line: {line}").into());
        };
        let Some(hex) = parts.next() else {
            return Err(format!("missing hex in reference line: {line}").into());
        };
        parsed.insert((case.to_owned(), kind.to_owned()), decode_hex(hex)?);
    }
    Ok(parsed)
}

fn assert_case_matches_reference(
    reference: &ReferenceVectors,
    spec: CaseSpec,
) -> Result<(), Box<dyn Error>> {
    let primary = primary_fragments(spec.k, spec.block_size, spec.seed, spec.wfb_mode);
    let fec = FecCode::new(spec.k, spec.n)?;

    let parity = fec.encode(&primary, spec.block_size)?;
    let parity_concat = parity.concat();
    assert_eq!(
        parity_concat,
        *reference
            .get(&(spec.name.to_owned(), "parity".to_owned()))
            .ok_or("missing parity reference")?,
        "{} parity differs from PixelPilot zfex",
        spec.name
    );

    let mut fragments = vec![None; spec.n];
    for idx in 0..spec.k {
        if spec.missing.contains(&idx) {
            continue;
        }
        fragments[idx] = Some(primary[idx].clone());
    }
    for (offset, parity_fragment) in parity.into_iter().enumerate() {
        fragments[spec.k + offset] = Some(parity_fragment);
    }

    fec.recover_primary(&mut fragments, spec.block_size)?;
    let mut recovered = Vec::new();
    for &idx in spec.missing {
        recovered.extend_from_slice(
            fragments[idx]
                .as_deref()
                .ok_or("missing recovered primary fragment")?,
        );
    }
    assert_eq!(
        recovered,
        *reference
            .get(&(spec.name.to_owned(), "recovered".to_owned()))
            .ok_or("missing recovered reference")?,
        "{} recovered data differs from PixelPilot zfex",
        spec.name
    );

    Ok(())
}

fn assert_plain_assembler_recovers_with_pixelpilot_parity(
    reference: &ReferenceVectors,
) -> Result<(), Box<dyn Error>> {
    let primary = primary_fragments(3, MAX_FEC_PAYLOAD, 0, true);
    let parity = reference
        .get(&("wfb".to_owned(), "parity".to_owned()))
        .ok_or("missing wfb parity reference")?;
    let parity_0 = &parity[..MAX_FEC_PAYLOAD];

    let mut assembler = PlainAssembler::new(3, 5)?;
    let first = assembler.push_decrypted_fragment(0, &primary[0])?;
    assert_eq!(payloads(first), vec![b"alpha".to_vec()]);

    let gap = assembler.push_decrypted_fragment(2, &primary[2])?;
    assert!(gap.is_empty());

    let recovered = assembler.push_decrypted_fragment(3, parity_0)?;
    assert_eq!(
        payloads(recovered),
        vec![b"bravo".to_vec(), b"charlie".to_vec()]
    );

    Ok(())
}

fn primary_fragments(k: usize, block_size: usize, seed: u32, wfb_mode: bool) -> Vec<Vec<u8>> {
    (0..k)
        .map(|fragment_idx| {
            if wfb_mode {
                wfb_plain_fragment(block_size, fragment_idx)
            } else {
                raw_fragment(block_size, seed, fragment_idx)
            }
        })
        .collect()
}

fn raw_fragment(block_size: usize, seed: u32, fragment_idx: usize) -> Vec<u8> {
    (0..block_size)
        .map(|offset| {
            (seed + fragment_idx as u32 * 31 + offset as u32 * 17 + (offset as u32 % 251)) as u8
        })
        .collect()
}

fn wfb_plain_fragment(block_size: usize, fragment_idx: usize) -> Vec<u8> {
    let payload = match fragment_idx % 3 {
        0 => b"alpha".as_slice(),
        1 => b"bravo".as_slice(),
        _ => b"charlie".as_slice(),
    };
    let mut fragment = vec![0; block_size];
    fragment[0] = 0;
    fragment[1..WPACKET_HDR_LEN].copy_from_slice(&(payload.len() as u16).to_be_bytes());
    fragment[WPACKET_HDR_LEN..WPACKET_HDR_LEN + payload.len()].copy_from_slice(payload);
    fragment
}

fn payloads(outputs: Vec<openipc_core::WfbOutput>) -> Vec<Vec<u8>> {
    outputs.into_iter().map(|output| output.payload).collect()
}

fn decode_hex(hex: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    if !hex.len().is_multiple_of(2) {
        return Err("odd-length hex string".into());
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for idx in (0..hex.len()).step_by(2) {
        bytes.push(u8::from_str_radix(&hex[idx..idx + 2], 16)?);
    }
    Ok(bytes)
}
