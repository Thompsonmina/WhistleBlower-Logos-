# Chronicle Metadata Envelope

Chronicle broadcasts a compact JSON envelope through Logos Messaging for each
stored document. The
envelope carries the Storage CID plus the normalized metadata needed by
indexers, verifiers, and future registry anchoring.

## Envelope

Version 1 envelope:

```json
{
  "v": 1,
  "cid": "zDv...",
  "content_type": "application/pdf",
  "size_bytes": 1048576,
  "timestamp": 1736294400,
  "title": "Quarterly evidence",
  "description": "",
  "tags": [],
  "metadata_hash": "v1:<sha256-hex>"
}
```

Required fields:

| Field | Type | Notes |
|---|---|---|
| `v` | number | Envelope wire format version. |
| `cid` | string | Storage CID returned after upload. |
| `content_type` | string | Normalized MIME type. |
| `size_bytes` | number | Original file size in bytes. |
| `timestamp` | number | Unix timestamp for when Chronicle created the envelope. |
| `title` | string | Public title and Storage filename base. |
| `description` | string | Empty string when not supplied. |
| `tags` | array of strings | Empty array when not supplied. |
| `metadata_hash` | string | Versioned deterministic hash of stable metadata. |

## Normalization

`content_type` is normalized before hashing and broadcast:

1. Trim whitespace.
2. Lowercase.
3. Strip parameters after `;`.
4. Apply known aliases, for example `image/jpg` to `image/jpeg`.
5. Fall back to `application/octet-stream` if invalid or empty.

`title`, `description`, and `tags` are trimmed and capped by Chronicle before
the envelope is built. Empty `description` and `tags` are still included.

Current content type aliases:

| Input | Normalized |
|---|---|
| `application/x-pdf` | `application/pdf` |
| `image/jpg` | `image/jpeg` |
| `audio/mp3` | `audio/mpeg` |
| `text/javascript` | `application/javascript` |
| `application/x-javascript` | `application/javascript` |
| `text/x-markdown` | `text/markdown` |

## Metadata Hash

`metadata_hash` is versioned separately from envelope `v`.

Envelope `v` describes the Logos Messaging JSON wire format. The hash prefix
describes the metadata identity scheme: which fields are included and how they
are normalized.

Version 1 hash:

```text
metadata_hash = "v1:" + sha256(canonical_json(metadata_subset))
```

Version 1 metadata subset:

```json
{
  "content_type": "application/pdf",
  "description": "",
  "size_bytes": 1048576,
  "tags": [],
  "title": "Quarterly evidence"
}
```

Excluded fields:

| Field | Reason |
|---|---|
| `cid` | Anchored alongside the hash as its own value. |
| `v` | Envelope framing, not document metadata. |
| `timestamp` | Changes across rebroadcasts. |
| `metadata_hash` | The value being computed. |

## Canonical JSON

For version 1, Chronicle canonicalizes the hash input with this exact key order:

```text
content_type, description, size_bytes, tags, title
```

The compact JSON form has no insignificant whitespace. Strings use normal JSON
escaping. Tags preserve their normalized order.

Example canonical input:

```json
{"content_type":"text/plain","description":"","size_bytes":21,"tags":[],"title":"TextTitle"}
```

SHA-256 of that byte string is:

```text
fc0237529ab19d2bffa3f202c737e61ea3b7794840f59a70077ec3efabc9f462
```

So the final metadata hash is:

```text
v1:fc0237529ab19d2bffa3f202c737e61ea3b7794840f59a70077ec3efabc9f462
```

## Verification Helpers

Chronicle exposes JSON helper methods so external tools can reproduce the same
normalization and hash without uploading a file:

```cpp
QString normalizeContentTypeJson(QString contentType);
QString hashMetadataJson(QString contentType,
                         QString sizeBytes,
                         QString title,
                         QString description,
                         QString tagsJson);
QString buildMetadataEnvelopeJson(QString envelopeInputJson);
```

`tagsJson` must be a JSON string array, for example:

```json
["finance","audit"]
```

`buildMetadataEnvelopeJson(...)` takes one JSON object string:

```json
{
  "cid": "zDv...",
  "content_type": "application/pdf",
  "size_bytes": 1048576,
  "timestamp": 1736294400,
  "title": "Quarterly evidence",
  "description": "",
  "tags": []
}
```
