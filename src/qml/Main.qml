import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Dialogs

Item {
    id: root

    readonly property var backend: logos.module("whistleblower")
    readonly property bool ready: backend !== null && logos.isViewModuleReady("whistleblower")
    readonly property string status: backend ? backend.status : ""
    readonly property bool busy: backend ? backend.busy : false
    readonly property string cid: backend ? backend.cid : ""
    readonly property string metadataHash: backend ? backend.metadataHash : ""
    readonly property string lastError: backend ? backend.lastError : ""
    readonly property bool deliveryReady: backend ? backend.deliveryReady : false

    function statusColor(s) {
        if (s === "broadcast_sent") return "#56d364"
        if (s === "error") return "#f85149"
        if (s === "idle" || s === "") return "#8b949e"
        return "#d29922"  // in-flight states
    }

    FileDialog {
        id: filePicker
        title: "Choose a file to publish"
        onAccepted: {
            // selectedFile is a file:// URL — strip the scheme for chronicle.
            var u = filePicker.selectedFile.toString()
            pathField.text = u.replace(/^file:\/\//, "")
        }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 24
        spacing: 14

        Text {
            text: "Whistleblower — Publish a Document"
            font.pixelSize: 20
            color: "#ffffff"
            Layout.alignment: Qt.AlignHCenter
        }

        RowLayout {
            spacing: 12
            Layout.fillWidth: true

            Text {
                text: root.ready ? (root.deliveryReady ? "Connected · Delivery ready"
                                                       : "Connected · Delivery not ready")
                                 : "Connecting to backend..."
                color: root.ready && root.deliveryReady ? "#56d364" : "#f0883e"
                font.pixelSize: 12
            }
        }

        // ── Form ──────────────────────────────────────────────────────────
        GridLayout {
            columns: 2
            columnSpacing: 12
            rowSpacing: 8
            Layout.fillWidth: true

            Text { text: "File path"; color: "#c9d1d9"; font.pixelSize: 13 }
            RowLayout {
                Layout.fillWidth: true
                spacing: 6
                TextField {
                    id: pathField
                    Layout.fillWidth: true
                    placeholderText: "/absolute/path/to/file"
                    enabled: !root.busy
                }
                Button {
                    text: "Browse…"
                    enabled: !root.busy
                    onClicked: filePicker.open()
                }
            }

            Text { text: "Title"; color: "#c9d1d9"; font.pixelSize: 13 }
            TextField {
                id: titleField
                Layout.fillWidth: true
                placeholderText: "Public title"
                enabled: !root.busy
            }

            Text { text: "Content type"; color: "#c9d1d9"; font.pixelSize: 13 }
            TextField {
                id: contentTypeField
                Layout.fillWidth: true
                placeholderText: "application/pdf  (default: application/octet-stream)"
                enabled: !root.busy
            }

            Text { text: "Description"; color: "#c9d1d9"; font.pixelSize: 13 }
            TextField {
                id: descField
                Layout.fillWidth: true
                placeholderText: "(optional)"
                enabled: !root.busy
            }

            Text { text: "Tags (csv)"; color: "#c9d1d9"; font.pixelSize: 13 }
            TextField {
                id: tagsField
                Layout.fillWidth: true
                placeholderText: "evidence, internal, draft"
                enabled: !root.busy
            }
        }

        RowLayout {
            Layout.alignment: Qt.AlignHCenter
            spacing: 12

            Button {
                text: root.busy ? "Publishing…" : "Publish"
                enabled: root.ready && !root.busy && pathField.text.length > 0
                                                  && titleField.text.length > 0
                onClicked: {
                    logos.watch(backend.publish(
                        pathField.text,
                        contentTypeField.text,
                        titleField.text,
                        descField.text,
                        tagsField.text
                    ), function() {}, function(err) {
                        console.warn("publish call failed:", err)
                    })
                }
            }

            Button {
                text: "Refresh"
                enabled: root.ready && !root.busy
                onClicked: backend.startBroadcaster()
            }
        }

        // ── Status panel ──────────────────────────────────────────────────
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: statusLayout.implicitHeight + 24
            color: "#161b22"
            radius: 6
            border.color: "#30363d"
            border.width: 1

            ColumnLayout {
                id: statusLayout
                anchors { fill: parent; margins: 12 }
                spacing: 6

                RowLayout {
                    spacing: 8
                    Text { text: "Status:"; color: "#8b949e"; font.pixelSize: 13 }
                    Text {
                        text: root.status === "" ? "idle" : root.status
                        color: root.statusColor(root.status)
                        font.pixelSize: 13
                        font.bold: true
                    }
                    BusyIndicator {
                        running: root.busy
                        visible: root.busy
                        Layout.preferredHeight: 16
                        Layout.preferredWidth: 16
                    }
                }

                Text {
                    visible: root.cid !== ""
                    text: "CID: " + root.cid
                    color: "#56d364"
                    font.pixelSize: 12
                    font.family: "monospace"
                    Layout.fillWidth: true
                    wrapMode: Text.WrapAnywhere
                }
                Text {
                    visible: root.metadataHash !== ""
                    text: "Hash: " + root.metadataHash
                    color: "#8b949e"
                    font.pixelSize: 11
                    font.family: "monospace"
                    Layout.fillWidth: true
                    wrapMode: Text.WrapAnywhere
                }
                Text {
                    visible: root.lastError !== ""
                    text: "Error: " + root.lastError
                    color: "#f85149"
                    font.pixelSize: 12
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                }
            }
        }

        Item { Layout.fillHeight: true }
    }
}
