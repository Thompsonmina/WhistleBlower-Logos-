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

## Build and install

From the repo root:

```bash
# Portable .lgx (for end-user LogosBasecamp installs)
nix build .#whistleblower-lgx-portable -o /tmp/whistleblower.lgx
lgpm install /tmp/whistleblower.lgx --ui-plugins-dir ~/.local/share/Logos/LogosBasecamp/plugins

# Dev variant (writes to LogosBasecampDev)
nix build .#whistleblower-install -o /tmp/whistleblower-new
cp -rf /tmp/whistleblower-new/plugins/whistleblower/. \
       ~/.local/share/Logos/LogosBasecampDev/plugins/whistleblower/
```

`lgpm install` automatically pulls in chronicle and its deps
(`storage_module`, `delivery_module`) since whistleblower lists chronicle in
`metadata.json`. Tested against `logos-basecamp` rev
[`064ef3f`](https://github.com/logos-co/logos-basecamp/commit/064ef3f168a061c77f8cc1bb6afc9f0a04f5b920)


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
