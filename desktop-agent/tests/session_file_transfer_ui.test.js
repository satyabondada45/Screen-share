const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { File } = require("node:buffer");

const htmlPath = path.join(__dirname, "..", "assets", "controller_session.html");
const html = fs.readFileSync(htmlPath, "utf8");
const readStart = html.indexOf("async function readTransferSourceBytes(source, context) {");
const start = html.indexOf("async function sendFiles(files) {");
const end = html.indexOf("        let incomingFiles = {};", start);
assert.notEqual(readStart, -1, "readTransferSourceBytes function must exist");
assert.notEqual(start, -1, "sendFiles function must exist");
assert.notEqual(end, -1, "sendFiles function boundary must exist");
const sendFilesSource = html.slice(start, end);
const readSourceEnd = html.indexOf("        let incomingFiles = {};", readStart);
const readSource = html.slice(readStart, readSourceEnd);

test("A-side source read logs exact operation, range, and browser error", async () => {
    const diagnostics = [];
    const context = {
        Uint8Array,
        Number,
        JSON,
        String,
        Error,
        console: {
            info: message => diagnostics.push(message),
            error: message => diagnostics.push(message)
        }
    };
    vm.runInNewContext(
        `${readSource}; globalThis.readTransferSourceBytes = readTransferSourceBytes;`,
        context
    );
    const metadata = {
        sessionId: "session-test",
        transferId: "42",
        filename: "nested/naïve file.bin",
        operation: "File.arrayBuffer for SHA-256",
        offset: 0,
        requestedBytes: 3,
        fileSize: 3,
        lastModified: 7
    };

    const bytes = await context.readTransferSourceBytes(
        new File([new Uint8Array([1, 2, 3])], metadata.filename),
        metadata
    );
    assert.deepEqual(Array.from(bytes), [1, 2, 3]);

    const largePayload = new Uint8Array(2 * 1024 * 1024).fill(0x5a);
    const largeMetadata = {
        ...metadata,
        filename: "nested folder/naïve report (final).bin",
        operation: "File.slice(0,2097152).arrayBuffer",
        requestedBytes: largePayload.length,
        fileSize: largePayload.length
    };
    const largeFile = new File([largePayload], largeMetadata.filename);
    const largeBytes = await context.readTransferSourceBytes(largeFile, largeMetadata);
    assert.equal(largeBytes.length, largePayload.length);

    await assert.rejects(
        context.readTransferSourceBytes({
            arrayBuffer: async () => {
                throw new DOMException("Access denied", "NotReadableError");
            }
        }, { ...metadata, requestedBytes: 5 }),
        /Source read failed during File\.arrayBuffer/
    );
    assert.ok(diagnostics.some(message =>
        message.includes("error_name=NotReadableError")
            && message.includes("requested_bytes=5")
            && message.includes("actual_bytes=0")
            && message.includes('filename="nested/naïve file.bin"')
    ));
});

async function runSendFiles({ files, sendSingleFile, isSocketOpen }) {
    const alerts = [];
    const context = {
        files,
        ws: { readyState: 1 },
        isStreaming: true,
        isSocketOpen,
        sendSingleFile,
        alert: message => alerts.push(message),
        console: { error() {}, log() {} }
    };
    await vm.runInNewContext(`(async () => { ${sendFilesSource} await sendFiles(files); })()`, context);
    return alerts;
}

test("A-side folder batch continues after a file read failure", async () => {
    const attempts = [];
    const alerts = await runSendFiles({
        files: [{ name: "__init__.cpython-314.pyc" }, { name: "next.txt" }],
        isSocketOpen: () => true,
        sendSingleFile: async file => {
            attempts.push(file.name);
            if (file.name.startsWith("__init__")) {
                throw new Error("source file could not be read");
            }
        }
    });

    assert.deepEqual(attempts, ["__init__.cpython-314.pyc", "next.txt"]);
    assert.equal(alerts.length, 1);
    assert.match(alerts[0], /__init__\.cpython-314\.pyc/);
    assert.match(alerts[0], /source file could not be read/);
    assert.match(alerts[0], /1 failed item/);
});

test("A-side folder batch stops after a failure disconnects the session", async () => {
    const attempts = [];
    let socketOpen = true;
    const alerts = await runSendFiles({
        files: [{ name: "first.txt" }, { name: "unattempted.txt" }],
        isSocketOpen: () => socketOpen,
        sendSingleFile: async file => {
            attempts.push(file.name);
            socketOpen = false;
            throw new Error("WebSocket disconnected.");
        }
    });

    assert.deepEqual(attempts, ["first.txt"]);
    assert.equal(alerts.length, 1);
    assert.match(alerts[0], /1 item\(s\) were not attempted/);
});