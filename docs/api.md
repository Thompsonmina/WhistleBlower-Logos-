# Chronicle Module — API Reference

Chronicle is a Logos Core module that implements the upload → broadcast → anchor
pipeline for censorship-resistant document publication. It wraps `storage_module`
for file storage and `delivery_module` for peer-to-peer metadata broadcast.

All methods accept and return compact UTF-8 JSON strings. Every response includes
an `"ok"` boolean at the top level.

---

## Method Groups

| Group | Methods |
|---|---|
| **Publish** (high-level) | `publishFileJson`, `publishStatusJson`, `listPublishedJson` |
| **Upload** (low-level) | `uploadFileJson`, `uploadStatusJson` |
| **Envelope** utilities | `normalizeContentTypeJson`, `hashMetadataJson`, `buildMetadataEnvelopeJson` |
| **Broadcast** (low-level) | `startBroadcasterJson`, `broadcastEnvelopeJson`, `broadcastStatusJson` |
| **Diagnostics** | `health` |

---

## Publish API

These are the primary methods for application use. They sequence the full
upload → envelope → broadcast pipeline and persist results locally.

### `publishFileJson(requestJson)`

Stages and queues a file for upload to `storage_module`, then automatically
broadcasts the resulting metadata envelope through `delivery_module`.

**Request**

```json
{
  "path":         "/absolute/path/to/file.pdf",
  "content_type": "application/pdf",
  "title":        "Public title used as staged filename",
  "description":  "",
  "tags":         [],
  "broadcast":    true
}
```

| Field | Required | Default | Notes |
|---|---|---|---|
| `path` | yes | — | Absolute path to a local file. |
| `content_type` | yes | — | MIME type. Normalized internally (see constraints). |
| `title` | yes | — | Becomes the staged filename base. Max 200 chars. |
| `description` | no | `""` | Max 2 000 chars. |
| `tags` | no | `[]` | Array of strings. Max 20 tags, max 64 chars each. |
| `broadcast` | no | `true` | Set `false` to upload without broadcasting. |

**Response — queued**

```json
{
  "queued":     true,
  "ok":         false,
  "publish_id": "57b59365-cad1-42ab-bf6e-f135db0df470",
  "upload_id":  "2b745811-4a06-4a60-8491-6df40e163d4f",
  "status":     "queued"
}
```

`ok` is `false` here because the work is not yet complete. Poll
`publishStatusJson` until `ok` becomes `true`.

**Error response**

```json
{
  "queued": false,
  "ok":     false,
  "code":   "MISSING_FIELD",
  "error":  "title is required"
}
```

**Validation error codes**

| Code | Meaning |
|---|---|
| `INVALID_REQUEST` | `requestJson` is not a valid JSON object. |
| `MISSING_FIELD` | `path`, `content_type`, or `title` is missing or blank. |
| `INVALID_TAGS` | `tags` is not an array of strings. |
| `FILE_READ_FAILED` | File does not exist or could not be staged. |
| `OVERSIZED` | File exceeds 100 MiB. |
| `EMPTY_TITLE` | Title is blank after sanitization. |

---

### `publishStatusJson(publishId)`

Returns the current state of a publish job.

**Terminal success response**

```json
{
  "ok":           true,
  "publish_id":   "57b59365-cad1-42ab-bf6e-f135db0df470",
  "upload_id":    "2b745811-4a06-4a60-8491-6df40e163d4f",
  "broadcast_id": "603f2d4b-778a-49bf-a73f-bdc1e39ca61b",
  "status":       "broadcast_sent",
  "content_type": "application/pdf",
  "title":        "Quarterly evidence",
  "description":  "",
  "tags":         [],
  "size_bytes":   1048576,
  "cid":          "zDvZRwzm2yPe4HhAvYqnaVpw7AWkT8FM6UmUvrT4T9WmPrcnvBxM",
  "metadata_hash":"v1:fc0237529ab19d2bffa3f202c737e61ea3b7794840f59a70077ec3efabc9f462",
  "envelope": {
    "v":            1,
    "cid":          "zDvZRwzm2yPe4HhAvYqnaVpw7AWkT8FM6UmUvrT4T9WmPrcnvBxM",
    "content_type": "application/pdf",
    "size_bytes":   1048576,
    "timestamp":    1736294400,
    "title":        "Quarterly evidence",
    "description":  "",
    "tags":         [],
    "metadata_hash":"v1:fc0237529ab19d2bffa3f202c737e61ea3b7794840f59a70077ec3efabc9f462"
  },
  "created_at_ms": 1736294350000,
  "updated_at_ms": 1736294400000
}
```

**Publish status values**

| Status | `ok` | Meaning |
|---|---|---|
| `queued` | false | Accepted; upload not yet started. |
| `uploading` | false | Active upload attempt in progress. |
| `uploaded` | false | Storage returned a CID. |
| `envelope_built` | false | Metadata envelope and hash computed. |
| `broadcasting` | false | Broadcast queued and being dispatched. |
| `broadcast_sent` | true | Envelope dispatched to delivery network (optimistic). |
| `error` | false | Terminal failure. See `code` and `error` fields. |

**Error codes on terminal failure**

| Code | Meaning |
|---|---|
| `FILE_READ_FAILED` | Staging failed. |
| `OVERSIZED` | File exceeds 100 MiB cap. |
| `EMPTY_TITLE` | Title empty after sanitization. |
| `STORAGE_UNAVAILABLE` | Transient Storage failure; retries exhausted. |
| `STORAGE_REJECTED` | Storage rejected the upload (non-retryable). |
| `RETRIES_EXHAUSTED` | Retry budget expired. |
| `ENVELOPE_TOO_LARGE` | Metadata envelope exceeds 8 KiB. |
| `DELIVERY_UNAVAILABLE` | Could not initialize or reach delivery node. |
| `BROADCAST_FAILED` | Delivery rejected the send. |
| `DUPLICATE` | Same CID + metadata hash already published. `original_publish_id` present. |
| `interrupted` | Module restarted while job was in flight. |
| `INTERNAL` | Module was not initialized (`initLogos` not called). |

**Unknown ID**

```json
{
  "queued": false,
  "ok":     false,
  "code":   "UNKNOWN_PUBLISH",
  "error":  "unknown publish_id: ..."
}
```

---

### `listPublishedJson()`

Returns all persisted publish records ordered newest-first. Includes active,
completed, and failed records. Excludes low-level upload retry details.

```json
{
  "ok": true,
  "records": [
    {
      "ok":           true,
      "publish_id":   "57b59365-...",
      "upload_id":    "2b745811-...",
      "broadcast_id": "603f2d4b-...",
      "status":       "broadcast_sent",
      "content_type": "application/pdf",
      "title":        "Quarterly evidence",
      "description":  "",
      "tags":         [],
      "size_bytes":   1048576,
      "cid":          "zDvZRwzm...",
      "metadata_hash":"v1:...",
      "envelope":     {},
      "created_at_ms": 1736294350000,
      "updated_at_ms": 1736294400000
    }
  ]
}
```

Records survive module restarts via the local publish ledger (see
[Persistence](#persistence)).

---

## Upload API

Low-level upload primitives. Used internally by the publish API and available
directly for diagnostics or custom orchestration.

### `uploadFileJson(path, contentType, title)`

Stages a copy of the file under a title-derived filename (hiding the original
path from Storage) and queues an upload to `storage_module`.

Uploads are serialized: if another upload is active the new one stays `queued`
until the active one finishes.

**Response — queued**

```json
{
  "queued":    true,
  "upload_id": "2b745811-4a06-4a60-8491-6df40e163d4f"
}
```

**Error response**

```json
{
  "queued": false,
  "ok":     false,
  "code":   "EMPTY_TITLE",
  "error":  "title is required and must contain visible characters"
}
```

---

### `uploadStatusJson(uploadId)`

Returns the current state of an upload job.

```json
{
  "upload_id":          "2b745811-4a06-4a60-8491-6df40e163d4f",
  "status":             "uploaded",
  "ok":                 true,
  "attempt":            1,
  "attempt_timeout_ms": 47000,
  "size_bytes":         21,
  "content_type":       "text/plain",
  "cid":                "zDvZRwzm2yPe4HhAvYqnaVpw7AWkT8FM6UmUvrT4T9WmPrcnvBxM",
  "timestamp":          1736294400,
  "title":              "TextTitle",
  "metadata_hash":      "v1:fc0237529ab19d2bffa3f202c737e61ea3b7794840f59a70077ec3efabc9f462",
  "envelope": { ... }
}
```

**Upload status values**

| Status | `ok` | Meaning |
|---|---|---|
| `queued` | false | Waiting for previous upload to finish. |
| `uploading` | false | Active attempt in progress. |
| `retrying` | false | Transient failure; retry timer armed. `next_retry_at_ms` present. |
| `uploaded` | true | CID received; envelope built. |
| `error` | false | Terminal failure. |

**Retry and timeout model**

```
attempt_timeout = clamp(45s + ceil(size / 1 MiB) × 2s, 45s, 5min)
retry_budget    = clamp(attempt_timeout × 3, 90s, 15min)
backoff         = 1s, 2s, 4s, 8s, 16s, 30s … (±25% jitter)
```

---

## Envelope Utilities

Pure computation helpers. No network calls.

### `normalizeContentTypeJson(contentType)`

Strips parameters, lowercases, applies aliases, and falls back to
`application/octet-stream` for unrecognized values.

```json
{ "ok": true, "content_type": "text/plain" }
```

**Common aliases**

| Input | Normalized |
|---|---|
| `application/x-pdf` | `application/pdf` |
| `image/jpg` | `image/jpeg` |
| `audio/mp3` | `audio/mpeg` |
| `text/javascript` | `application/javascript` |
| `text/x-markdown` | `text/markdown` |

---

### `hashMetadataJson(contentType, sizeBytes, title, description, tagsJson)`

Computes the deterministic `v1` metadata hash over the canonicalized subset of
user-supplied fields. The hash is stable across re-uploads of the same content
with the same metadata.

`sizeBytes` is passed as a string to avoid 32-bit integer overflow in
cross-process calls.

`tagsJson` is a JSON-encoded array of strings or an empty string.

```json
{
  "ok":            true,
  "metadata_hash": "v1:fc0237529ab19d2bffa3f202c737e61ea3b7794840f59a70077ec3efabc9f462",
  "canonical_json": "{\"content_type\":\"text/plain\",\"description\":\"\",\"size_bytes\":21,\"tags\":[],\"title\":\"TextTitle\"}"
}
```

**Hash input** — canonical JSON (keys sorted, no whitespace) over:

```json
{
  "content_type": "<normalized>",
  "description":  "<sanitized>",
  "size_bytes":   <integer>,
  "tags":         [...],
  "title":        "<sanitized>"
}
```

Fields excluded from the hash: `cid`, envelope `v`, `timestamp`. These describe
where and when the document was published, not the document's stable identity.

---

### `buildMetadataEnvelopeJson(envelopeInputJson)`

Builds the LP-17 metadata envelope from raw fields. Content type is normalized,
metadata hash is computed, and the result is validated against the 8 KiB cap.

**Input**

```json
{
  "cid":          "zDvZRwzm...",
  "content_type": "application/pdf",
  "size_bytes":   1048576,
  "timestamp":    1736294400,
  "title":        "Quarterly evidence",
  "description":  "",
  "tags":         []
}
```

**Response**

```json
{
  "ok": true,
  "metadata_hash": "v1:...",
  "envelope": {
    "v":            1,
    "cid":          "zDvZRwzm...",
    "content_type": "application/pdf",
    "size_bytes":   1048576,
    "timestamp":    1736294400,
    "title":        "Quarterly evidence",
    "description":  "",
    "tags":         [],
    "metadata_hash":"v1:..."
  }
}
```

---

## Broadcast API

Low-level broadcast primitives. Used internally by the publish API. Available
directly when an envelope has already been built externally.

### `startBroadcasterJson()`

Acquires the `delivery_module` object, calls `createNode` with the default
`logos.dev` config, and calls `start`. Must be called once before
`broadcastEnvelopeJson` when using the broadcast API directly.

The publish API does not require a prior `startBroadcasterJson` call from
application code — however, in logoscore CLI sessions it must be called
explicitly before publishing (see [Logoscore Usage](#logoscore-usage)).

```json
{
  "ok":    true,
  "started": true,
  "topic": "/chronicle/1/document-index/json"
}
```

---

### `broadcastEnvelopeJson(envelopeJson)`

Validates the envelope, checks the `metadata_hash`, deduplicates by
`cid:metadata_hash`, and dispatches an async send through `delivery_module`.

The broadcast is marked `sent` optimistically — confirmation comes from Delivery
daemon logs, not from a callback.

**Response — accepted**

```json
{
  "queued":       true,
  "broadcast_id": "603f2d4b-778a-49bf-a73f-bdc1e39ca61b",
  "topic":        "/chronicle/1/document-index/json",
  "deduped":      false
}
```

**Response — deduplicated**

```json
{
  "queued":       true,
  "broadcast_id": "603f2d4b-...",
  "topic":        "/chronicle/1/document-index/json",
  "deduped":      true,
  "status":       "sent"
}
```

**Validation error codes**

| Code | Meaning |
|---|---|
| `INVALID_ENVELOPE` | Input is not a valid JSON object. |
| `UNSUPPORTED_ENVELOPE_VERSION` | Envelope `v` is not `1`. |
| `EMPTY_CID` | `cid` field is missing or blank. |
| `INVALID_CONTENT_TYPE` | `content_type` field missing. |
| `EMPTY_TITLE` | `title` blank after sanitization. |
| `INVALID_NUMBER` | `size_bytes` or `timestamp` not a non-negative integer. |
| `INVALID_TAGS` | `tags` is not an array of strings. |
| `MISSING_METADATA_HASH` | `metadata_hash` field absent. |
| `METADATA_HASH_MISMATCH` | Supplied hash does not match recomputed hash. |
| `ENVELOPE_TOO_LARGE` | Serialized envelope exceeds 8 KiB. |
| `DELIVERY_UNAVAILABLE` | Delivery node could not be initialized. |

---

### `broadcastStatusJson(broadcastId)`

```json
{
  "ok":           true,
  "broadcast_id": "603f2d4b-778a-49bf-a73f-bdc1e39ca61b",
  "status":       "sent",
  "topic":        "/chronicle/1/document-index/json",
  "cid":          "zDvZRwzm...",
  "metadata_hash":"v1:...",
  "deduped":      false,
  "envelope":     {},
  "created_at_ms":1736294350000,
  "updated_at_ms":1736294400000
}
```

**Broadcast status values**

| Status | `ok` | Meaning |
|---|---|---|
| `queued` | false | Accepted; delivery not yet started. |
| `sending` | false | Delivery startup / dispatch in progress. |
| `sent` | true | Async send dispatched (optimistic). |
| `deduped` | true | Already broadcast; existing record returned. |
| `error` | false | Terminal failure. |

---

## Diagnostics

### `health()`

Returns `"ok"` as a plain string when the module is loaded and responsive.

---

## Metadata Envelope Schema

The LP-17 envelope is the payload broadcast on the Logos Delivery network.

```json
{
  "v":            1,
  "cid":          "<content-address returned by storage_module>",
  "content_type": "<normalized MIME type>",
  "size_bytes":   <integer>,
  "timestamp":    <unix seconds, set when Storage returns the CID>,
  "title":        "<sanitized title>",
  "description":  "<sanitized description, may be empty string>",
  "tags":         ["<tag>", ...],
  "metadata_hash":"v1:<sha256-hex>"
}
```

`description` and `tags` are always present even when empty, for a stable wire
shape.

**Delivery topic:** `/chronicle/1/document-index/json`

The `delivery_module` base64-encodes the payload before transmission. Chronicle
passes the raw JSON string — do not pre-encode.

---

## Constraints

| Constraint | Value |
|---|---|
| Max file size | 100 MiB |
| Max title length | 200 chars |
| Max description length | 2 000 chars |
| Max tags | 20 |
| Max tag length | 64 chars |
| Max content-type length | 255 chars |
| Max envelope size | 8 KiB |
| Storage chunk size hint | 64 KiB |
| Upload attempt timeout (base) | 45 s |
| Upload attempt timeout (per MiB) | + 2 s |
| Upload attempt timeout (max) | 5 min |
| Retry budget (min) | 90 s |
| Retry budget (max) | 15 min |
| Concurrent uploads | 1 (serialized) |

---

## Privacy Model

Chronicle stages a copy of the source file before calling `storage_module`:

```
/tmp/chronicle_uploads/<upload_id>/<title-derived-name>.<ext>
```

The original local path is never passed to Storage. Storage records only the
title-derived filename in its manifest. The publish ledger also does not persist
`originalPath`.

Staging directories are removed on terminal upload state (success or error) and
on plugin shutdown.

---

## Persistence

Chronicle appends a JSON Lines ledger after every meaningful publish state
transition.

**Path:** `$XDG_DATA_HOME/<app>/chronicle/publish-ledger.jsonl`  
On Linux with logoscore: `~/.local/share/logos_host_qt/chronicle/publish-ledger.jsonl`

**Line format:**

```json
{"type":"publish_updated","record":{...}}
```

On module startup, Chronicle reads the ledger and reconstructs the latest state
per `publish_id`. Any record that was not in a terminal state (`broadcast_sent`
or `error`) is restored as:

```json
{ "status": "error", "code": "interrupted", "error": "Publish was interrupted before completion" }
```

The deduplication map (`cid:metadata_hash → publish_id`) is rebuilt from all
`broadcast_sent` records on startup.

---

## Deduplication

**Upload-level:** Chronicle serializes uploads — only one active upload at a
time.

**Broadcast-level:** `broadcastEnvelopeJson` deduplicates in-memory by
`cid:metadata_hash`. Re-broadcasting the same pair returns the existing
`broadcast_id` with `deduped: true`.

**Publish-level:** `publishFileJson` deduplicates durably. If an upload
completes with a CID that, combined with the metadata hash, matches a previously
`broadcast_sent` publish, the new job is terminated with:

```json
{
  "status":              "error",
  "code":                "DUPLICATE",
  "error":               "document already published; see original_publish_id",
  "original_publish_id": "<existing publish_id>"
}
```

---

## Logoscore Usage

Chronicle requires `storage_module` and `delivery_module` to be loaded and
initialized before use.

```bash
logoscore call storage_module init '{"data-dir":"/tmp/storage"}'
logoscore call storage_module start

# Initialise the delivery node synchronously before the first publish or
# broadcast. This is required in logoscore CLI sessions because delivery_module
# start() returns a different response shape when called from within an
# asynchronous module callback.
logoscore call chronicle startBroadcasterJson

# Publish a file end-to-end
logoscore call chronicle publishFileJson \
  '{"path":"/tmp/report.pdf","content_type":"application/pdf","title":"Q1 Report"}'

# Poll until broadcast_sent
logoscore call chronicle publishStatusJson "<publish_id>"

# List all persisted records
logoscore call chronicle listPublishedJson
```

See `scripts/logoscore-publish-smoke.sh` for a complete working example.
