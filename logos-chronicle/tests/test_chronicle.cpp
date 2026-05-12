#include <logos_test.h>
#include <QJsonArray>

#include "../src/chronicle_helpers.h"

using namespace chronicle;

LOGOS_TEST(normalizeContentType_strips_parameters_and_lowercases) {
    LOGOS_ASSERT_EQ(
        normalizeContentType("Text/Plain; charset=utf-8").toStdString(),
        std::string("text/plain"));
}

LOGOS_TEST(normalizeContentType_applies_alias) {
    LOGOS_ASSERT_EQ(normalizeContentType("image/jpg").toStdString(),
                    std::string("image/jpeg"));
}

LOGOS_TEST(normalizeContentType_falls_back_for_bad_input) {
    LOGOS_ASSERT_EQ(normalizeContentType("garbage").toStdString(),
                    std::string("application/octet-stream"));
}

LOGOS_TEST(sanitizeTitle_replaces_path_separators) {
    LOGOS_ASSERT_EQ(sanitizeTitle("a/b\\c").toStdString(),
                    std::string("a_b_c"));
}

LOGOS_TEST(metadataCaps_trim_and_limit_user_fields) {
    LOGOS_ASSERT_EQ(sanitizeTitle(QString(MAX_TITLE_LEN + 10, 'a')).size(),
                    MAX_TITLE_LEN);
    LOGOS_ASSERT_EQ(
        sanitizeDescription(QString(MAX_DESCRIPTION_LEN + 10, 'b')).size(),
        MAX_DESCRIPTION_LEN);
    LOGOS_ASSERT_EQ(sanitizeTag(QString(MAX_TAG_LEN + 10, 'c')).size(),
                    MAX_TAG_LEN);

    QStringList rawTags;
    for (int i = 0; i < MAX_TAGS + 5; ++i) {
        rawTags.append(QStringLiteral("tag%1").arg(i));
    }
    LOGOS_ASSERT_EQ(normalizeTags(rawTags).size(), MAX_TAGS);
}

LOGOS_TEST(synthesizeFilename_uses_content_type_extension) {
    LOGOS_ASSERT_EQ(
        synthesizeFilename("Report", "application/pdf").toStdString(),
        std::string("Report.pdf"));
}

LOGOS_TEST(synthesizeFilename_avoids_duplicate_extension) {
    LOGOS_ASSERT_EQ(
        synthesizeFilename("Report.pdf", "application/pdf").toStdString(),
        std::string("Report.pdf"));
}

LOGOS_TEST(synthesizeFilename_uses_bin_for_octet_stream) {
    LOGOS_ASSERT_EQ(
        synthesizeFilename("Payload", "application/octet-stream").toStdString(),
        std::string("Payload.bin"));
}

LOGOS_TEST(computeBackoff_attempt1_around_one_second) {
    auto ms = computeBackoff(1).count();
    LOGOS_ASSERT_GE(ms, 750);
    LOGOS_ASSERT_LE(ms, 1250);
}

LOGOS_TEST(isTransientError_rejects_validation_errors) {
    LOGOS_ASSERT_FALSE(isTransientError("validation failed"));
    LOGOS_ASSERT_TRUE(isTransientError("connection timed out"));
}

LOGOS_TEST(uploadAttemptTimeout_scales_with_size) {
    LOGOS_ASSERT_EQ(uploadAttemptTimeoutMs(0), BASE_ATTEMPT_TIMEOUT_MS);
    LOGOS_ASSERT_EQ(uploadAttemptTimeoutMs(1),
                    BASE_ATTEMPT_TIMEOUT_MS + PER_MIB_ATTEMPT_TIMEOUT_MS);
    LOGOS_ASSERT_EQ(uploadAttemptTimeoutMs(1024 * 1024),
                    BASE_ATTEMPT_TIMEOUT_MS + PER_MIB_ATTEMPT_TIMEOUT_MS);
    LOGOS_ASSERT_GT(uploadAttemptTimeoutMs(50 * 1024 * 1024),
                    uploadAttemptTimeoutMs(1024 * 1024));
}

LOGOS_TEST(uploadRetryBudget_allows_multiple_attempts) {
    const qint64 timeout = uploadAttemptTimeoutMs(10 * 1024 * 1024);
    LOGOS_ASSERT_GE(uploadRetryBudgetMs(10 * 1024 * 1024), timeout * 3);
    LOGOS_ASSERT_GE(uploadRetryBudgetMs(1), MIN_RETRY_BUDGET_MS);
}

LOGOS_TEST(canonicalMetadataJson_is_stable_and_sorted) {
    const QByteArray canonical = canonicalMetadataJson(
        "Text/Plain; charset=UTF-8",
        21,
        "TextTitle",
        "",
        QStringList{});
    LOGOS_ASSERT_EQ(
        canonical.toStdString(),
        std::string(
            "{\"content_type\":\"text/plain\",\"description\":\"\","
            "\"size_bytes\":21,\"tags\":[],\"title\":\"TextTitle\"}"));
}

LOGOS_TEST(hashMetadata_uses_versioned_canonical_metadata_without_timestamp_or_cid) {
    const QString hash = hashMetadata(
        "text/plain",
        21,
        "TextTitle",
        "",
        QStringList{});
    LOGOS_ASSERT_EQ(
        hash.toStdString(),
        std::string(
            "v1:fc0237529ab19d2bffa3f202c737e61ea3b7794840f59a70077ec3efabc9f462"));
}

LOGOS_TEST(hashMetadata_normalizes_aliases_and_parameters) {
    const QString canonicalHash = hashMetadata(
        "image/jpeg",
        1024,
        "Photo",
        "",
        QStringList{});
    const QString aliasHash = hashMetadata(
        "IMAGE/JPG; charset=binary",
        1024,
        "Photo",
        "",
        QStringList{});

    LOGOS_ASSERT_EQ(aliasHash.toStdString(), canonicalHash.toStdString());
}

LOGOS_TEST(buildMetadataEnvelope_applies_cap_and_optional_fields) {
    const QString hash = hashMetadata("text/html", 36, "WebPage", "desc",
                                      QStringList{"alpha", "beta"});
    const QJsonObject envelope = buildMetadataEnvelope(
        "zDvExample",
        "text/html; charset=utf-8",
        36,
        1736294400,
        "WebPage",
        "desc",
        QStringList{"alpha", "beta"},
        hash);

    LOGOS_ASSERT_TRUE(envelopeWithinCap(envelope));
    LOGOS_ASSERT_EQ(envelope.value("content_type").toString().toStdString(),
                    std::string("text/html"));
    LOGOS_ASSERT_EQ(envelope.value("metadata_hash").toString().toStdString(),
                    hash.toStdString());
}

LOGOS_TEST(buildMetadataEnvelope_timestamp_does_not_change_metadata_hash) {
    const QString hash = hashMetadata("application/pdf", 128, "Report", "",
                                      QStringList{});
    const QJsonObject first = buildMetadataEnvelope(
        "zDvExample",
        "application/pdf",
        128,
        1736294400,
        "Report",
        "",
        QStringList{},
        hash);
    const QJsonObject second = buildMetadataEnvelope(
        "zDvExample",
        "application/pdf",
        128,
        1736299999,
        "Report",
        "",
        QStringList{},
        hash);

    LOGOS_ASSERT_EQ(first.value("metadata_hash").toString().toStdString(),
                    second.value("metadata_hash").toString().toStdString());
    LOGOS_ASSERT_TRUE(first.value("timestamp").toInteger() !=
                      second.value("timestamp").toInteger());
}

LOGOS_TEST(buildMetadataEnvelope_includes_empty_description_and_tags) {
    const QString hash = hashMetadata("text/plain", 21, "TextTitle", "",
                                      QStringList{});
    const QJsonObject envelope = buildMetadataEnvelope(
        "zDvExample",
        "text/plain",
        21,
        1736294400,
        "TextTitle",
        "",
        QStringList{},
        hash);

    LOGOS_ASSERT_TRUE(envelope.contains("description"));
    LOGOS_ASSERT_TRUE(envelope.contains("tags"));
    LOGOS_ASSERT_EQ(envelope.value("description").toString().toStdString(),
                    std::string(""));
    LOGOS_ASSERT_EQ(envelope.value("tags").toArray().size(), 0);
}

LOGOS_TEST(envelopeWithinCap_rejects_oversized_envelope) {
    QJsonObject envelope;
    envelope.insert("v", 1);
    envelope.insert("cid", "zDvExample");
    envelope.insert("content_type", "text/plain");
    envelope.insert("size_bytes", 1);
    envelope.insert("timestamp", 1736294400);
    envelope.insert("title", QString(MAX_ENVELOPE_BYTES, 'x'));
    envelope.insert("description", "");
    envelope.insert("tags", QJsonArray{});
    envelope.insert("metadata_hash", "v1:hash");

    LOGOS_ASSERT_FALSE(envelopeWithinCap(envelope));
}
