# Chronicle — Document-Indexing Module for Logos

Chronicle is the standalone, reusable **document-indexing module** for LP-17.
It packages the upload → broadcast → anchor pipeline as a Logos Core module
so any Logos application can publish files to Logos Storage, broadcast their
CIDs over Logos Delivery, and anchor them on-chain — without re-implementing
that plumbing.

The Whistleblower Basecamp app in this repo is one consumer of chronicle.
Other Logos apps can declare chronicle as a dependency and use it the same way.

- **API reference:** [`docs/api.md`](docs/api.md)
- **On-chain anchoring deep-dive:** [`docs/anchoring.md`](docs/anchoring.md)
- **Integration smoke scripts:** [`scripts/`](scripts/) (use as runnable examples)

---

## What it does

| Stage | Module dependency | Chronicle method |
|---|---|---|
| Stage + upload a local file | `storage_module` | `publishFileJson` (or `uploadFileJson` for low-level) |
| Build and sign a metadata envelope (cid, title, content_type, size, hash, …) | — | `buildMetadataEnvelopeJson`, `hashMetadataJson` |
| Broadcast the envelope on the Chronicle delivery topic | `delivery_module` | `broadcastEnvelopeJson` (auto-fired by `publishFileJson`) |
| Anchor `(cid, metadata_hash, anchor_timestamp)` on-chain | LEZ via FFI | `anchorBatchJson` |
| Read the on-chain registry back | LEZ via FFI | `getRegistryJson`, `lookupAnchorJson` |
| Track local publish + anchor state | — | `listPublishedJson`, `listAnchorsJson` |

All chronicle methods accept and return compact JSON strings; every response
includes an `"ok"` boolean. See [`docs/api.md`](docs/api.md) for the full
method list and request/response schemas.

---

## Integration steps

### 1. Add chronicle as a module dependency

In your Logos module's `metadata.json`, declare chronicle:

```json
{
  "name": "your_app",
  "type": "ui_qml",
  "main": "your_app_plugin",
  "dependencies": ["chronicle"]
}
```

At install time, `lgpm` will fetch chronicle (and chronicle's own dependencies
— `storage_module` and `delivery_module`) automatically.

### 2. Call chronicle from your plugin

Chronicle is loaded into its own subprocess by the Logos host. From your
plugin, talk to it over the standard `LogosAPI` client. The minimal upload +
broadcast call:

```cpp
// C++ inside your Qt plugin
auto* client = new LogosAPIClient(
    QStringLiteral("chronicle"),   // module to call
    QStringLiteral("your_app"),    // your module's name
    m_logosAPI->getTokenManager(),
    this);

QJsonObject req;
req.insert("path",         "/abs/path/to/file.pdf");
req.insert("content_type", "application/pdf");
req.insert("title",        "Q1 results");
req.insert("broadcast",    true);

const QVariant resp = client->invokeRemoteMethod(
    QStringLiteral("chronicle"),
    QStringLiteral("publishFileJson"),
    QString::fromUtf8(QJsonDocument(req).toJson(QJsonDocument::Compact)));
```

The response is a JSON string. `publishFileJson` returns immediately with a
`publish_id`; poll `publishStatusJson(publishId)` until `ok` is `true` or an
`error` is set.

QML callers can use the host's auto-generated proxy directly:

```qml
readonly property var chronicle: logos.module("chronicle")

function publish(path) {
    var req = JSON.stringify({
        path: path, content_type: "text/plain",
        title: "memo", broadcast: true
    })
    chronicle.publishFileJson(req)
}
```

### 3. Configure on-chain anchoring (optional)

Anchoring is gated on a small config block. Call once on first use, then
chronicle persists it locally:

```js
chronicle.setAnchorConfigJson(JSON.stringify({
    program_id:        "6ac5aa11...d2cf0252",  // 32-byte hex from `spel deploy`
    sequencer_url:     "http://127.0.0.1:3040",
    wallet_home:       "/abs/path/to/.scaffold/wallet",
    signer_account_id: "CbgR6tj5kWx5oziiFptM7jMvrQeYY3Mzaao6ciuhSr2r"
}))
```

`anchorCapabilitiesJson()` reports `{configured, missing_fields}` for first-
launch UIs that want to prompt the user. The Whistleblower app's
`AnchorConfigDialog.qml` is a working example.

To anchor a previously published CID:

```js
chronicle.anchorBatchJson(JSON.stringify({
    entries: [{
        cid:           "zDvZRwzm...gVG3",
        metadata_hash: "v1:360b0df8...98d2",  // "v1:" prefix is stripped before FFI
        publish_id:    "fabc8ab4-1c43-...",
        timestamp:     1778577475
    }]
}))
```

`anchorBatchJson` blocks until the tx is signed and the sequencer accepts it;
returns `{ok, tx_hash}`. See [`docs/anchoring.md`](docs/anchoring.md) for the
FFI contract and how the registry account is laid out on-chain.

### 4. Build and install

The module ships as a portable `.lgx` package. From the repo root:

```bash
nix build .#chronicle-lgx-portable -o /tmp/chronicle.lgx
lgpm install /tmp/chronicle.lgx --modules-dir ~/.local/share/Logos/LogosBasecamp/modules
```

The package bundles `chronicle_plugin.so` plus the FFI sibling
`libchronicle_registry_ffi.so`; chronicle resolves the FFI via its own
plugin-dir at runtime, so no env var is required.

Dev variant (writes to `LogosBasecampDev`): `nix build .#chronicle-install`.

---

## Files

```
logos-chronicle/
  src/
    chronicle_interface.h        // Q_INVOKABLE method signatures (the SDK surface)
    chronicle_plugin.{h,cpp}     // implementation
    chronicle_anchor_client.{h,cpp}  // FFI loader / wrapper
    chronicle_anchor_config.{h,cpp}  // persisted anchor settings
  docs/
    api.md                       // full method reference
    anchoring.md                 // on-chain pipeline + FFI contract
  scripts/
    logoscore-publish-smoke.sh   // upload + broadcast end-to-end
    logoscore-anchor-smoke.sh    // anchor + on-chain readback
    logoscore-broadcast-smoke.sh // broadcast-only path
    logoscore-storage-smoke.sh   // storage init sanity
  metadata.json                  // declares deps: storage_module, delivery_module
```

The smoke scripts under `scripts/` are runnable integration examples — start
there if you want to see chronicle exercised against a real local sequencer.

---

## Versioning

Chronicle's interface (`chronicle_interface.h`) is the SDK surface. Method
names and JSON schemas in `docs/api.md` are the contract; the C++ class is
implementation-detail and may evolve. Breaking changes to interface methods
will bump the module's `version` in `metadata.json`.
