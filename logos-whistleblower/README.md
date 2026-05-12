# Whistleblower — Basecamp UI

QML + C++ Basecamp UI plugin for the LP-17 censorship-resistant document
publishing flow. Drives the [`chronicle`](../logos-chronicle/) module:
pick a file, fill optional metadata, watch it upload to Logos Storage,
broadcast on Logos Delivery, and (optionally) anchor on-chain.

This module is a thin presentation layer — all storage, broadcast, and
anchoring logic lives in chronicle, talked to over `LogosAPI`.

---

## What's in it

```
logos-whistleblower/
  src/
    whistleblower_plugin.{h,cpp}       // C++ backend: holds publish state, proxies chronicle calls
    whistleblower.rep                  // Qt Remote Objects interface for QML
    whistleblower_interface.h          // Q_INVOKABLE method signatures
    qml/
      Main.qml                         // root screen
      Theme.qml                        // color/spacing tokens
      components/                      // Card, Pill, Field, GhostButton, PrimaryButton,
                                       //   AnchorConfigDialog
  icons/whistleblower.png              // launcher tile icon
  metadata.json                        // declares "chronicle" as the sole dependency
```

The C++ side is small: it batches chronicle calls, forwards `publishStatus`
polling into QML properties, and surfaces the anchor config dialog. All
heavy lifting (file staging, broadcast, FFI) is in chronicle.

---

## Run from a fresh clone

End-to-end recipe: get a working basecamp + install the LP-17 modules + launch.

### 1. Build the pinned basecamp

We test against `logos-basecamp` rev
[`064ef3f`](https://github.com/logos-co/logos-basecamp/commit/064ef3f168a061c77f8cc1bb6afc9f0a04f5b920)
(`0.1.2-RC1`). Later revs hit an upstream `liblogos_core` shutdown crash that
manifests as `corrupted size vs. prev_size` on graceful exit and `Module process
crashed: chronicle` in basecamp — neither is in our code; both go away on this rev.

```bash
nix build 'github:logos-co/logos-basecamp/064ef3f168a061c77f8cc1bb6afc9f0a04f5b920' \
  -o /tmp/logos-basecamp
```

### 2. Build and install our modules

```bash
# From the repo root:
nix build .#chronicle-install      -o /tmp/chronicle-install
nix build .#whistleblower-install  -o /tmp/whistleblower-install

# Copy chronicle (+ bundled FFI sibling) into the basecamp data dir.
DATA=~/.local/share/Logos/LogosBasecampDev
mkdir -p "$DATA/modules/chronicle" "$DATA/plugins/whistleblower"
cp -rf /tmp/chronicle-install/modules/chronicle/.       "$DATA/modules/chronicle/"
cp -rf /tmp/whistleblower-install/plugins/whistleblower/. "$DATA/plugins/whistleblower/"

# delivery_module + storage_module come from upstream; the basecamp
# pre-bundles them, so this is usually a no-op. If yours doesn't:
nix build 'github:logos-co/logos-storage-module#install'  -o /tmp/storage
nix build 'github:logos-co/logos-delivery-module#install' -o /tmp/delivery
cp -rf /tmp/storage/modules/.  "$DATA/modules/"
cp -rf /tmp/delivery/modules/. "$DATA/modules/"
```

For portable end-user installs (different basecamp variant), use `lgpm`:
```bash
nix build .#whistleblower-lgx-portable -o /tmp/whistleblower.lgx
lgpm install /tmp/whistleblower.lgx --ui-plugins-dir ~/.local/share/Logos/LogosBasecamp/plugins
```
`lgpm install` resolves chronicle + its deps automatically via `metadata.json`.

### 3. Launch basecamp

```bash
# WSL/Linux without working Wayland: force xcb.
QT_QPA_PLATFORM=xcb /tmp/logos-basecamp/bin/LogosBasecamp
```

Click the **whistleblower** tile to open the UI. First anchor attempt opens
the **Anchor settings** dialog (see below) — fill it in once and it persists.

---

## First-launch config

On first anchor attempt the **Anchor settings** dialog opens with the
required fields blank. Fill in:

- `program_id` — 32-byte hex from `spel deploy` of `chronicle-registry/`
- `signer_account_id` — base58 account ID from your wallet (`storage.json → accounts[].account_id`)
- `wallet_home` — absolute path to your `.scaffold/wallet`
- `sequencer_url` — defaults to `http://127.0.0.1:3040`

Settings are persisted by chronicle, not by the UI. Reopen the dialog any
time via the "Anchor settings" button in the header.

---

## How it talks to chronicle

QML side, via the LogosAPI proxy auto-generated from chronicle's interface:

```qml
readonly property var backend: logos.module("whistleblower")
// backend exposes properties: status, busy, cid, metadataHash, publishedRecordsJson, …
// and methods: publishFile(...), anchorPublished(publishId), setAnchorConfig(cfgJson)
```

C++ side, the plugin holds a `LogosAPIClient` for chronicle and forwards
each user action to a single chronicle method, then re-polls. See
`whistleblower_plugin.cpp` for the wiring — it's the canonical example of
consuming chronicle from another module.
