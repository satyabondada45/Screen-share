const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

const remoteSession = fs.readFileSync(
    path.join(__dirname, "..", "assets", "remote_session.html"),
    "utf8"
);
const dashboard = fs.readFileSync(
    path.join(__dirname, "..", "assets", "dashboard.html"),
    "utf8"
);
const webviewApp = fs.readFileSync(
    path.join(__dirname, "..", "src", "webview_app.rs"),
    "utf8"
);
const controller = fs.readFileSync(
    path.join(__dirname, "..", "assets", "controller_session.html"),
    "utf8"
);
const positionerMatch = remoteSession.match(
    /window\.positionBOptionsMenu = function\(\) \{[\s\S]*?\n            \};/
);
const armReverseAutoAcceptMatch = dashboard.match(
    /function armReverseAutoAccept\(requesterId, requestId = ''\) \{[\s\S]*?\n        \}/
);
const approvedReverseRequesterMatch = dashboard.match(
    /function isApprovedReverseRequester\(requesterId\) \{[\s\S]*?\n        \}/
);
const reverseAutoAcceptRequestMatch = remoteSession.match(
    /function requestBReverseAutoAccept\(peerSystemId, sessionToken, requestId\) \{[\s\S]*?\n        \}/
);
const reverseAutoAcceptCancelMatch = remoteSession.match(
    /function cancelBReverseAutoAccept\(peerSystemId, sessionToken, requestId\) \{[\s\S]*?\n        \}/
);
const layoutSyncMatch = remoteSession.match(
    /function syncBOverlayLayout\(\) \{[\s\S]*?\n        \}/
);
const closeMenuMatch = remoteSession.match(
    /window\.closeBOptions = function\(\) \{[\s\S]*?\n            \};/
);
const disconnectHandlerMatch = remoteSession.match(
    /window\.handleBSessionDisconnectClick = function\(event\) \{[\s\S]*?\n            \};/
);
const disconnectFunctionStart = remoteSession.indexOf(
    "function disconnectBSession(reverseTargetId = null, source = 'END_SESSION_BUTTON', reverseRequestId = null) {"
);
const disconnectFunctionEnd = remoteSession.indexOf(
    "function toggleBSessionChat()",
    disconnectFunctionStart
);
const disconnectFunctionSource = remoteSession.slice(disconnectFunctionStart, disconnectFunctionEnd);

assert.ok(positionerMatch, "menu positioning helper must exist");
assert.ok(layoutSyncMatch, "overlay layout synchronization helper must exist");
assert.ok(closeMenuMatch, "menu close handler must exist");
assert.ok(disconnectHandlerMatch, "overlay disconnect handler must exist");
assert.notEqual(disconnectFunctionStart, -1, "B-session disconnect implementation must exist");
assert.notEqual(disconnectFunctionEnd, -1, "B-session disconnect implementation must have a boundary");

test("embedded dashboard and session scripts remain syntactically valid", () => {
    for (const [filename, html] of [
        ["dashboard.html", dashboard],
        ["remote_session.html", remoteSession],
        ["controller_session.html", controller]
    ]) {
        const scripts = [...html.matchAll(/<script\b[^>]*>([\s\S]*?)<\/script>/gi)];
        assert.ok(scripts.length > 0, `${filename} must contain embedded scripts`);
        for (const [index, script] of scripts.entries()) {
            if (script[1].trim()) {
                assert.doesNotThrow(
                    () => new vm.Script(script[1], { filename: `${filename}#script-${index + 1}` }),
                    `${filename} script ${index + 1} must parse`
                );
            }
        }
    }
});

test("open options menu is repositioned after native overlay expansion", () => {
    const menu = {
        offsetWidth: 204,
        offsetHeight: 224,
        style: {},
        classList: { contains: name => name === "open" }
    };
    const window = { innerWidth: 48, innerHeight: 48 };
    const button = {
        getBoundingClientRect: () => ({
            left: window.innerWidth - 44,
            right: window.innerWidth - 4,
            top: 4
        })
    };
    const layoutSnapshots = [];
    const context = {
        window,
        document: {
            getElementById(id) {
                if (id === "optionsMenu") return menu;
                if (id === "optionsToggleBtn") return button;
                return null;
            }
        },
        setBOverlayLayout(layout) {
            layoutSnapshots.push({ layout, left: menu.style.left, top: menu.style.top });
        }
    };

    vm.runInNewContext(
        `${positionerMatch[0]}\n${layoutSyncMatch[0]}; globalThis.syncLayout = syncBOverlayLayout;`,
        context
    );

    context.window.positionBOptionsMenu();
    assert.equal(menu.style.left, "-160px", "idle viewport reproduces stale off-screen placement");

    window.innerWidth = 268;
    window.innerHeight = 300;
    context.syncLayout();

    assert.equal(menu.style.left, "14px");
    assert.equal(menu.style.top, "4px");
    assert.ok(parseFloat(menu.style.left) >= 0);
    assert.ok(parseFloat(menu.style.left) + menu.offsetWidth <= window.innerWidth);
    assert.ok(parseFloat(menu.style.top) + menu.offsetHeight <= window.innerHeight);
    assert.deepEqual(layoutSnapshots.at(-1), { layout: "menu", left: "14px", top: "4px" });
});

test("menu flips right and clamps vertically when space is limited", () => {
    const menu = { offsetWidth: 204, offsetHeight: 224, style: {} };
    const window = { innerWidth: 268, innerHeight: 300 };
    const context = {
        window,
        document: {
            getElementById(id) {
                if (id === "optionsMenu") return menu;
                if (id === "optionsToggleBtn") {
                    return { getBoundingClientRect: () => ({ left: 4, right: 44, top: 250 }) };
                }
                return null;
            }
        }
    };

    vm.runInNewContext(`${positionerMatch[0]}`, context);
    context.window.positionBOptionsMenu();

    assert.equal(menu.style.left, "50px");
    assert.equal(menu.style.top, "72px");
    assert.ok(parseFloat(menu.style.left) + menu.offsetWidth <= window.innerWidth - 4);
    assert.ok(parseFloat(menu.style.top) + menu.offsetHeight <= window.innerHeight - 4);
});

test("overlay keeps one menu and preserves existing control actions", () => {
    for (const id of [
        "optionsToggleBtn",
        "optionsMenu",
        "bOverlayChatBtn",
        "bOverlayFileTransferBtn",
        "bMicBtn",
        "bOverlayRevBtn",
        "bDisconnectBtn"
    ]) {
        assert.equal(
            (remoteSession.match(new RegExp(`id="${id}"`, "g")) || []).length,
            1,
            `${id} must exist exactly once`
        );
    }

    assert.match(remoteSession, /id="bOverlayChatBtn"[\s\S]*?toggleBSessionChat\(\)/);
    assert.match(remoteSession, /id="bOverlayFileTransferBtn"[\s\S]*?showBSessionTransferChoice\(\)/);
    assert.match(remoteSession, /id="bMicBtn"[\s\S]*?toggleBSessionMic\(\)/);
    assert.match(remoteSession, /id="bOverlayRevBtn"[\s\S]*?requestBSessionReverse\(\)/);
    assert.match(remoteSession, /id="bDisconnectBtn"[\s\S]*?handleBSessionDisconnectClick\(event\)/);
});

test("reverse handoff reconnects from the requester and normalizes the approved peer identity", () => {
    assert.match(remoteSession, /requestBReverseAutoAccept\(peerSystemId, handoffToken, requestId\)/);
    assert.match(webviewApp, /"arm_reverse_acceptance" =>/);
    assert.match(webviewApp, /"cancel_reverse_acceptance" =>/);
    assert.match(remoteSession, /complete_reverse_host_handoff:\$\{reverseTargetId\}:\$\{sessionToken\}:\$\{handoffRequestId\}/);
    assert.match(remoteSession, /await requestBReverseAutoAccept\(peerSystemId, handoffToken, requestId\)/);
    assert.match(remoteSession, /disconnectBSession\(peerSystemId,\s*'REMOTE_SESSION_END',\s*requestId\)/);
    assert.match(controller, /type:\s*"reverse_handoff_initiate",\s*target_system_id:\s*DEVICE_ID/);
    assert.match(dashboard, /type === 'reverse_handoff_initiate'/);
    assert.match(dashboard, /sourceSession\.reverseHandoffTargetId = targetId/);
    assert.match(dashboard, /startConnection\(reverseTargetId\)\.catch/);
    assert.match(webviewApp, /stage=OLD_SESSION_CLOSED message_type=31/);
    assert.match(webviewApp, /"arm_reverse_acceptance" =>/);
    assert.match(webviewApp, /"complete_reverse_host_handoff" =>/);
    assert.match(dashboard, /isApprovedReverseRequester\(req\.requester_system_id\)/);
    assert.match(dashboard, /function armReverseAutoAccept\(requesterId, requestId = ''\)/);
    assert.match(dashboard, /function processPendingReverseAcceptance\(\)/);
    assert.match(dashboard, /processPendingReverseAcceptance\(\);/);
    assert.match(dashboard, /AUTO_ACCEPT_ARMED/);
    assert.doesNotMatch(remoteSession, /startConnection\(reverseTargetId\)/);
    assert.doesNotMatch(
        remoteSession,
        /start_reverse_connection:\$\{reverseTargetId\}/
    );
    assert.match(dashboard, /traceReverseHandoff\('REQUEST_CREATED', targetId\)/);
    assert.match(dashboard, /traceReverseHandoff\('SESSION_CONNECTED', sourceTargetId\)/);
    assert.match(
        dashboard,
        /showToast\('Reverse session connected\. The role transition is confirmed\.'\)/
    );
    assert.match(controller, /stage: "TYPE30_RECEIVED"/);
    assert.match(controller, /stage: accepted \? "TYPE31_ACCEPT_SENT" : "TYPE31_REJECT_SENT"/);
    assert.match(dashboard, /isApprovedReverseRequester\(req\.requester_system_id\)/);
    assert.match(dashboard, /traceReverseSession\('AUTO_ACCEPT_MATCHED', approvedReverseRequesterId\)/);
    assert.match(webviewApp, /"reverse_session_trace" =>/);
});

test("reverse auto-accept matches numeric and string IDs but rejects another peer", () => {
    assert.ok(approvedReverseRequesterMatch, "requester matcher must exist");
    const context = {
        approvedReverseRequesterId: "111222333",
        Boolean,
        String
    };
    vm.runInNewContext(
        `${approvedReverseRequesterMatch[0]}\nglobalThis.matches = isApprovedReverseRequester;`,
        context
    );

    assert.equal(context.matches(111222333), true);
    assert.equal(context.matches("111222333"), true);
    assert.equal(context.matches("999888777"), false);
    assert.equal(context.matches(null), false);
});

test("reverse handoff does not tear down until peer auto-approval is acknowledged", async () => {
    assert.ok(reverseAutoAcceptRequestMatch, "reverse approval handshake must exist");
    const listeners = new Map();
    const posted = [];
    const window = {
        ipc: { postMessage: message => posted.push(message) },
        addEventListener: (name, handler) => listeners.set(name, handler),
        removeEventListener: name => listeners.delete(name)
    };
    const context = {
        window,
        Promise,
        RegExp,
        setTimeout: () => 1,
        clearTimeout: () => {}
    };
    vm.runInNewContext(
        `${reverseAutoAcceptRequestMatch[0]}\nglobalThis.arm = requestBReverseAutoAccept;`,
        context
    );
    const resultPromise = context.arm("111222333", "42", "request-1");
    assert.deepEqual(posted, ["arm_reverse_acceptance:111222333:42:request-1"]);
    const resultHandler = listeners.get("message");
    assert.equal(typeof resultHandler, "function");
    resultHandler({
        source: window,
        data: {
            type: "reverse_autoaccept_arm_result",
            peer_id: "111222333",
            request_id: "request-1",
            armed: true
        }
    });
    assert.equal(await resultPromise, true);
    assert.equal(listeners.has("message"), false);
});

test("reverse auto-approval cancellation is peer-bound and correlated", () => {
    assert.ok(reverseAutoAcceptCancelMatch, "reverse auto-approval cancellation helper must exist");
    const posted = [];
    const context = {
        window: { ipc: { postMessage: message => posted.push(message) } },
        String,
        RegExp
    };
    vm.runInNewContext(
        `${reverseAutoAcceptCancelMatch[0]}\nglobalThis.cancel = cancelBReverseAutoAccept;`,
        context
    );

    context.cancel("111222333", "42", "request-1");
    context.cancel("bad-peer", "42", "request-1");
    context.cancel("111222333", "", "request-1");
    assert.deepEqual(posted, ["cancel_reverse_acceptance:111222333:42:request-1"]);
    assert.match(remoteSession, /if \(!handoffAttempted && isCurrentBSession\(generation\)\) \{\s*cancelBReverseAutoAccept\(peerSystemId, handoffToken, requestId\);/);
    assert.match(remoteSession, /if \(decisionReceived === 1\) \{\s*if \(decisionPeerSystemId && decisionRequestId\) \{\s*const pageParams = new URLSearchParams\(window\.location\.search\);\s*cancelBReverseAutoAccept\(/);
});

test("reverse auto-accept is peer-bound, correlated locally, and rejects overlapping handoffs", () => {
    assert.ok(armReverseAutoAcceptMatch, "reverse auto-accept helper must exist");
    const traces = [];
    const toasts = [];
    const context = {
        currentHostUid: "222222222",
        isAuthenticated: true,
        approvedReverseRequesterId: null,
        approvedReverseExpiresAt: 0,
        approvedReverseRequestId: null,
        Date,
        console: { error() {}, warn() {}, info() {} },
        traceReverseHandoff: (...args) => traces.push(args),
        showToast: message => toasts.push(message)
    };
    vm.runInNewContext(
        `${armReverseAutoAcceptMatch[0]}\nglobalThis.arm = armReverseAutoAccept;`,
        context
    );

    assert.equal(context.arm("111111111", "reverse-accepted-1"), true);
    assert.equal(context.approvedReverseRequesterId, "111111111");
    assert.equal(context.approvedReverseRequestId, "reverse-accepted-1");
    assert.equal(context.arm("111111111", "reverse-accepted-1"), true);
    assert.equal(context.arm("333333333", "reverse-accepted-2"), false);
    assert.equal(context.arm("222222222", "reverse-self"), false);
    assert.deepEqual(traces, [
        ["AUTO_ACCEPT_ARMED", "111111111"],
        ["AUTO_ACCEPT_ARM_FAILED", "333333333"],
        ["AUTO_ACCEPT_ARM_FAILED", "222222222"]
    ]);
    assert.equal(toasts.length, 2);
});

test("closing the menu clears its state without hiding the options button", () => {
    const removed = [];
    const attributes = {};
    const button = {
        classList: { remove: value => removed.push(value) },
        setAttribute: (name, value) => { attributes[name] = value; }
    };
    const menu = {
        classList: { remove: value => removed.push(value) },
        setAttribute: (name, value) => { attributes[name] = value; }
    };
    let syncCount = 0;
    const context = {
        window: {},
        document: {
            getElementById(id) {
                if (id === "optionsToggleBtn") return button;
                if (id === "optionsMenu") return menu;
                return null;
            }
        },
        syncBOverlayLayout: () => { syncCount++; }
    };

    vm.runInNewContext(closeMenuMatch[0], context);
    context.window.closeBOptions();

    assert.deepEqual(removed, ["open", "menu-open"]);
    assert.deepEqual(attributes, { "aria-hidden": "true", "aria-expanded": "false" });
    assert.equal(syncCount, 1);
});

test("Disconnect awaits the B-session terminator without closing its WebView early", async () => {
    const events = [];
    const label = { textContent: "Disconnect" };
    const button = {
        disabled: false,
        querySelector: selector => selector === "#bDisconnectLabel" ? label : null
    };
    let terminate;
    const termination = new Promise(resolve => { terminate = resolve; });
    const context = {
        window: {
            deskStreamEndAnySession: () => events.push({ type: "unsafe-dashboard-helper" })
        },
        document: {
            getElementById: id => id === "bDisconnectBtn" ? button : null
        },
        _bSessionActive: true,
        _bSessionDisconnectPromise: null,
        disconnectBSession: (peerId, source) => {
            events.push({ type: "terminate", peerId, source });
            return termination;
        },
        closeBOptions: () => events.push({ type: "close-menu" }),
        showToast: message => events.push({ type: "toast", message }),
        _dismissBSessionOverlay: () => events.push({ type: "dismiss" }),
        Promise
    };

    vm.runInNewContext(disconnectHandlerMatch[0], context);
    const result = context.window.handleBSessionDisconnectClick({ stopPropagation() {} });

    assert.equal(button.disabled, true);
    assert.equal(label.textContent, "Disconnecting…");
    assert.deepEqual(events, [
        { type: "close-menu" },
        { type: "terminate", peerId: null, source: "END_SESSION_BUTTON" }
    ]);
    assert.equal(events.some(event => event.type === "unsafe-dashboard-helper"), false);

    const duplicateResult = context.window.handleBSessionDisconnectClick({ stopPropagation() {} });
    assert.equal(await duplicateResult, false);
    assert.equal(events.filter(event => event.type === "terminate").length, 1);

    terminate(true);
    assert.equal(await result, true);
});

test("Disconnect reports a missing session controller instead of silently closing", async () => {
    const events = [];
    const button = { disabled: false };
    const context = {
        window: {},
        document: { getElementById: id => id === "bDisconnectBtn" ? button : null },
        _bSessionActive: true,
        _bSessionDisconnectPromise: null,
        closeBOptions: () => events.push("close-menu"),
        showToast: message => events.push(message),
        _dismissBSessionOverlay: () => events.push("dismiss"),
        Promise
    };

    vm.runInNewContext(disconnectHandlerMatch[0], context);
    assert.equal(await context.window.handleBSessionDisconnectClick(), false);
    assert.deepEqual(events, ["Could not disconnect: the session controller is unavailable."]);
    assert.equal(button.disabled, false);
});

test("Disconnect cleans up the overlay idempotently after the session already ended", async () => {
    const events = [];
    const context = {
        window: {},
        document: { getElementById: () => null },
        _bSessionActive: false,
        _dismissBSessionOverlay: () => events.push("dismiss-overlay"),
        Promise
    };

    vm.runInNewContext(disconnectHandlerMatch[0], context);
    assert.equal(await context.window.handleBSessionDisconnectClick(), true);
    assert.deepEqual(events, ["dismiss-overlay"]);
});

test("B-session disconnect posts the Rust command and waits for confirmed termination", async () => {
    const fetchCalls = [];
    const state = {
        health: [{ in_session: true }, { in_session: false }],
        generation: 7,
        ended: 0,
        toasts: []
    };
    const context = {
        _bSessionDisconnectPromise: null,
        _bSessionActive: true,
        _bSessionDisconnecting: false,
        _bSessionGeneration: state.generation,
        _bReverseHandoffPending: false,
        _bSessionController: new AbortController(),
        _bSessionWatchdog: null,
        _bSessionChatPoll: null,
        _bSessionTypingPoll: null,
        _bSessionOfferPoll: null,
        _bSessionTimerInterval: null,
        document: {
            getElementById: id => id === "bDisconnectBtn"
                ? { disabled: false, querySelector: () => ({ textContent: "Disconnect" }) }
                : null
        },
        stopBSessionChatTyping() {},
        isCurrentBSession: generation => generation === state.generation,
        fetchHealth: async () => ({
            ok: true,
            json: async () => state.health.shift()
        }),
        fetch: async (url, options) => {
            fetchCalls.push({ url, options });
            return { ok: true };
        },
        setTimeout: callback => {
            callback();
            return 1;
        },
        clearTimeout() {},
        showToast: message => state.toasts.push(message),
        showBSessionEnded: () => { state.ended++; },
        console: { error() {} },
        Promise,
        Date
    };
    vm.runInNewContext(disconnectFunctionSource, context);

    assert.equal(await context.disconnectBSession(null, "END_SESSION_BUTTON"), true);
    assert.equal(fetchCalls.length, 1);
    assert.equal(fetchCalls[0].url, "/desktop-api/session/disconnect");
    assert.equal(fetchCalls[0].options.method, "POST");
    assert.equal(fetchCalls[0].options.headers["X-DeskStream-Diagnostic-Source"], "END_SESSION_BUTTON");
    assert.equal(state.ended, 1);
    assert.ok(state.toasts.includes("Disconnecting session..."));
});

test("B-session disconnect restores controls and reports a failed Rust command", async () => {
    const buttonLabel = { textContent: "Disconnecting..." };
    const button = {
        disabled: true,
        querySelector: () => buttonLabel
    };
    const toasts = [];
    const context = {
        _bSessionDisconnectPromise: null,
        _bSessionActive: true,
        _bSessionDisconnecting: false,
        _bSessionGeneration: 2,
        _bReverseHandoffPending: false,
        _bSessionController: new AbortController(),
        document: { getElementById: id => id === "bDisconnectBtn" ? button : null },
        stopBSessionChatTyping() {},
        isCurrentBSession: generation => generation === 2,
        fetchHealth: async () => ({ ok: true, json: async () => ({ in_session: true }) }),
        fetch: async () => { throw new Error("relay unavailable"); },
        showToast: message => toasts.push(message),
        console: { error() {} },
        Promise
    };
    vm.runInNewContext(disconnectFunctionSource, context);

    assert.equal(await context.disconnectBSession(null, "END_SESSION_BUTTON"), false);
    assert.equal(button.disabled, false);
    assert.equal(buttonLabel.textContent, "Disconnect");
    assert.equal(context._bSessionDisconnecting, false);
    assert.deepEqual(toasts, ["Could not disconnect session: relay unavailable"]);
});
