---
sidebar_position: 6
---

# Preset Packs

Nebulus preset packs are portable JSON files for sharing presentation and
processing choices. They are not complete receiver profiles. A receiver
profile contains local operational state such as selected adapters, radio
channel, Link ID, keys, recording directory, and network destinations; those
values never belong in a community preset.

The format is deliberately data-only. Nebulus does not execute scripts, load
plugins, fetch assets, or interpret shaders from a pack.

## How The Pieces Fit

| Object | Lifetime | Contains |
| --- | --- | --- |
| Receiver profile | Local and editable | Hardware, radio, secrets, routes, telemetry, audio, VPN, decoder policy, and a reference to an OSD profile |
| OSD profile | Local and editable | Indicator layout, visibility, graphs, scale, and overlay appearance |
| Application theme | Global | Nebulus interface palette |
| Preset pack | Installed, immutable, and versioned | Any combination of portable OSD, theme, route, telemetry, and performance components |

Installing an OSD component creates or refreshes a local OSD profile carrying
the pack ID and version as provenance. A receiver profile references that OSD
profile by local ID. This allows several aircraft to reuse one layout while
still switching to the correct OSD when a receiver profile is selected.

Themes remain global. A pack may include one, but applying an OSD does not
change the theme unless the theme component is selected in the preview.

## Install And Apply

Open **Settings → Preset packs** and select **Install file**. Nebulus validates
the file before showing it and presents each available component as a separate
checkbox. Nothing is applied until **Install and apply** is pressed.

Installed versions are kept side-by-side. Updating from `1.0.0` to `1.1.0`
does not modify a receiver profile automatically: install the new file, inspect
its components, and apply it explicitly. Removing an installed pack keeps any
derived local OSD and receiver profiles.

| Component | Result |
| --- | --- |
| OSD | Creates or refreshes a local OSD profile and selects it for the active receiver profile |
| Theme | Changes the global GUI theme |
| Routes | Replaces the active profile's auxiliary payload routes |
| Telemetry | Replaces protocol and filter policy while preserving the local MAVLink signing key |
| Performance | Applies codec preference and RTP reorder policy |

UDP route templates never carry a destination. They are installed disabled
with a loopback placeholder and must be configured locally before use.

### Install From A URL

The preset manager also accepts an HTTPS URL. Paste either of these into
**Preset or registry URL** and select **Open URL**:

- a direct `.nebulus-preset.json` URL
- a registry index containing several preset versions
- a normal GitHub `blob` URL; Nebulus converts it to the corresponding raw URL

Downloads run outside the receiver/UI thread and are limited to 512 KiB. HTTP
is rejected except for `localhost`, `127.0.0.1`, and `::1`, which keeps local
development convenient without allowing an Internet download to downgrade to
plaintext. Browser builds additionally require the host to allow CORS. GitHub
raw-content URLs already do.

Opening a URL still leads to the same component preview as a local file. A
download never applies a profile on its own.

## Export A Pack

Select **Export current** in the same section. Enter a namespaced ID such as
`pilotname.race-osd`, a SemVer version, author, and license, then choose the
components to include. Nebulus writes a `.nebulus-preset.json` file.

Export is structural rather than a redaction pass: the public Rust types have
no fields capable of holding these values:

- WFB and MAVLink signing keys
- USB device identities
- radio channel, width, offset, or Link ID
- recording and filesystem paths
- VPN credentials
- UDP hosts and ports
- logs, history, or runtime metrics

The schema is available at
[`apps/nebulus/presets/schema-v1.json`](https://github.com/neelsani/openipc-rs/blob/master/apps/nebulus/presets/schema-v1.json).
The repository also includes an
[`openipc-standard` example](https://github.com/neelsani/openipc-rs/blob/master/apps/nebulus/presets/openipc-standard.nebulus-preset.json).

## Pack Shape

```json
{
  "$schema": "https://raw.githubusercontent.com/neelsani/openipc-rs/master/apps/nebulus/presets/schema-v1.json",
  "schemaVersion": 1,
  "id": "pilotname.race-osd",
  "version": "1.0.0",
  "name": "Race OSD",
  "author": "Pilot Name",
  "license": "MIT",
  "minimumNebulusVersion": "0.1.29",
  "components": {
    "osd": { "name": "Race OSD" },
    "theme": "macchiato",
    "performance": {
      "codecPreference": "auto",
      "rtpReorder": false
    }
  }
}
```

`schemaVersion` governs the JSON structure. `version` identifies the immutable
pack release. Nebulus rejects unsupported schema versions, malformed SemVer,
packs requiring a newer app, unknown fields, oversized files, and invalid
component ranges.

OSD telemetry indicators consume Nebulus's normalized telemetry state rather
than a route ID. A selected route with the **Telemetry to OSD** action can feed
that state from MAVLink, MSP, or CRSF, so an OSD layout remains reusable when a
pilot changes radio ports or telemetry protocols.

## Community Distribution

A pack can be hosted as an unchanged JSON file in a GitHub repository, release,
or any HTTPS static host. A registry is another static JSON file; it does not
run a server-side API or install code.

```json
{
  "$schema": "https://raw.githubusercontent.com/neelsani/openipc-rs/master/apps/nebulus/presets/registry-schema-v1.json",
  "schemaVersion": 1,
  "name": "Community race presets",
  "description": "OSD and routing presets maintained by our group.",
  "homepage": "https://github.com/example/nebulus-presets",
  "presets": [
    {
      "id": "example.race-osd",
      "version": "1.2.0",
      "name": "Race OSD",
      "author": "Example FPV",
      "license": "MIT",
      "description": "Compact race overlay.",
      "downloadUrl": "race-osd.nebulus-preset.json",
      "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    }
  ]
}
```

`downloadUrl` may be absolute or relative to the registry file. IDs and
versions must match the downloaded pack. When `sha256` is present, Nebulus also
verifies the exact downloaded bytes before parsing them. Registries can list
several versions of one ID; installing a newer version remains an explicit
choice.

For GitHub, commit the registry and packs together, then give users either the
raw registry URL or its ordinary `github.com/.../blob/...` page URL. Pinning
`downloadUrl` to a commit SHA makes the advertised version immutable. The
repository includes a working
[`registry.json`](https://github.com/neelsani/openipc-rs/blob/master/apps/nebulus/presets/registry.json)
and its
[`registry-schema-v1.json`](https://github.com/neelsani/openipc-rs/blob/master/apps/nebulus/presets/registry-schema-v1.json).

Author and license fields are descriptive metadata, not proof of identity.
Nebulus does not currently sign packs or attest their publisher, so a community
registry should publish the SHA-256 field and distribute its registry URL from a
trusted page. A checksum detects a changed pack, but it does not protect against
an attacker who can replace both the registry and its checksums. Even an
untrusted pack cannot execute code or carry the local-only fields listed above,
but users should still inspect its component preview before applying it.
