const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

const dashboard = fs.readFileSync(path.join(__dirname, "..", "assets", "dashboard.html"), "utf8");
const controller = fs.readFileSync(path.join(__dirname, "..", "assets", "controller_session.html"), "utf8");
const remoteSession = fs.readFileSync(path.join(__dirname, "..", "assets", "remote_session.html"), "utf8");
const sessionRouter = fs.readFileSync(path.join(__dirname, "..", "assets", "session.html"), "utf8");
const retryStart = dashboard.indexOf("function reconnectDelay(attempt) {");
const retryEnd = dashboard.indexOf("function isFreshAuthorizationEligible", retryStart);
const eligibilityStart = retryEnd;
const eligibilityEnd = dashboard.indexOf("function waitForReconnectDelay", eligibilityStart);
const requestStart = dashboard.indexOf("async function requestFreshAuthorization(targetId, retry) {");
const requestEnd = dashboard.indexOf("async function runFreshAuthorizationReconnect", requestStart);
assert.notEqual(retryStart, -1, "reconnect retry helpers must exist");
assert.notEqual(retryEnd, -1, "retry helpers must have a boundary");
assert.notEqual(requestStart, -1, "fresh authorization request function must exist");
assert.notEqual(requestEnd, -1, "fresh authorization request boundary must exist");

const retryContext = {};
vm.runInNewContext(
    `${dashboard.slice(retryStart, retryEnd)}; globalThis.retryDelay = reconnectDelay; globalThis.retryUntilCancelled = retryUntilCancelled;`,
    retryContext
);

test("fresh authorization retries continue beyond five failures then succeed", async () => {
    let attempts = 0;
    const delays = [];
    const result = await retryContext.retryUntilCancelled(
        async attempt => {
            attempts++;
            if (attempt < 12) throw new Error("temporary network failure");
            return "fresh-authorized-session";
        },
        () => true,
        async delay => { delays.push(delay); return true; },
        () => {}
    );

    assert.equal(result.success, true);
    assert.equal(result.attempt, 12);
    assert.equal(result.result, "fresh-authorized-session");
    assert.equal(attempts, 12);
    assert.equal(delays.length, 11);
});

test("retry backoff remains bounded by 30 seconds including jitter", () => {
    for (let attempt = 1; attempt <= 100; attempt++) {
        const delay = retryContext.retryDelay(attempt);
        assert.ok(delay >= 1000);
        assert.ok(delay <= 30000);
    }
});

test("End Session while waiting cancels further retries", async () => {
    let active = true;
    let attempts = 0;
    const result = await retryContext.retryUntilCancelled(
        async () => { attempts++; throw new Error("temporary failure"); },
        () => active,
        async () => { active = false; return false; },
        () => {}
    );

    assert.equal(result.success, false);
    assert.equal(attempts, 1);
});

test("a stale in-flight success after End Session cannot reactivate", async () => {
    let active = true;
    let resolveAttempt;
    const resultPromise = retryContext.retryUntilCancelled(
        () => new Promise(resolve => { resolveAttempt = resolve; }),
        () => active,
        async () => true,
        () => {}
    );
    await Promise.resolve();
    active = false;
    resolveAttempt("stale-session");

    const result = await resultPromise;
    assert.equal(result.success, false);
});

test("retry callbacks become ineligible when the session is replaced", () => {
    const context = {
        freshAuthorizationRequests: new Map(),
        activeSessions: {}
    };
    vm.runInNewContext(
        `${dashboard.slice(eligibilityStart, eligibilityEnd)}; globalThis.isEligible = isFreshAuthorizationEligible;`,
        context
    );
    const currentSession = {};
    const retry = { cancelled: false, session: currentSession };
    context.activeSessions["123456789"] = currentSession;
    context.freshAuthorizationRequests.set("123456789", retry);
    assert.equal(context.isEligible("123456789", retry), true);

    context.activeSessions["123456789"] = {};
    assert.equal(context.isEligible("123456789", retry), false);
    context.activeSessions["123456789"] = currentSession;
    retry.cancelled = true;
    assert.equal(context.isEligible("123456789", retry), false);
});

test("pending fresh authorization is polled without creating duplicate requests", async () => {
    const calls = [];
    const logs = [];
    const statuses = [
        { status: "success", request_status: "pending" },
        { status: "success", request_status: "accepted" }
    ];
    const context = {
        BACKEND: "https://backend.test",
        currentHostUid: "111222333",
        currentHostAlias: "Device",
        retry: { controller: new AbortController(), attempt: 1 },
        isFreshAuthorizationEligible: () => true,
        waitForReconnectDelay: async () => true,
        console: { info: message => logs.push(message), warn: message => logs.push(message) },
        fetch: async (url, options) => {
            calls.push({ url, options });
            if (url.endsWith("request.php")) {
                return {
                    ok: true,
                    json: async () => ({
                        status: "success",
                        request: { request_token: "secret-test-token", status: "pending" }
                    })
                };
            }
            return { ok: true, json: async () => statuses.shift() };
        }
    };
    vm.runInNewContext(
        `${dashboard.slice(requestStart, requestEnd)}; globalThis.requestFreshAuthorization = requestFreshAuthorization;`,
        context
    );

    const token = await context.requestFreshAuthorization("444555666", context.retry);
    assert.equal(token, "secret-test-token");
    assert.equal(calls.filter(call => call.url.endsWith("request.php")).length, 1);
    assert.equal(calls.filter(call => call.url.includes("status.php")).length, 2);
    assert.ok(calls.every(call => call.options.credentials === "include"));
    assert.ok(logs.every(message => !message.includes("secret-test-token")));
});

test("controller shows End Session during retry and terminal Ended state after cancellation", () => {
    const stateStart = controller.indexOf("function showSessionEnded() {");
    const stateEnd = controller.indexOf("function switchMonitor(index) {", stateStart);
    assert.notEqual(stateStart, -1, "controller session state handlers must exist");
    assert.notEqual(stateEnd, -1, "controller state handler boundary must exist");
    const elements = new Map();
    const context = {
        document: {
            getElementById(id) {
                if (!elements.has(id)) {
                    elements.set(id, { textContent: "", style: { display: "" } });
                }
                return elements.get(id);
            }
        }
    };
    vm.runInNewContext(
        `${controller.slice(stateStart, stateEnd)}; globalThis.showRetry = showSessionReconnecting; globalThis.showEnded = showSessionEnded;`,
        context
    );

    context.showRetry();
    assert.equal(elements.get("sessionEndedTitle").textContent, "Reconnecting");
    assert.equal(elements.get("cancelSessionReconnectBtn").style.display, "inline-flex");
    assert.equal(elements.get("sessionEndedOkBtn").style.display, "none");

    context.showEnded();
    assert.equal(elements.get("sessionEndedTitle").textContent, "Session Ended");
    assert.equal(elements.get("cancelSessionReconnectBtn").style.display, "none");
    assert.equal(elements.get("sessionEndedOkBtn").style.display, "inline-flex");
});

test("explicit peer rejection stops retries without another authorization request", async () => {
    let attempts = 0;
    let waits = 0;
    const result = await retryContext.retryUntilCancelled(
        async () => {
            attempts++;
            const error = new Error("fresh authorization rejected by peer");
            error.retryable = false;
            throw error;
        },
        () => true,
        async () => { waits++; return true; },
        () => {}
    );

    assert.equal(result.success, false);
    assert.equal(result.attempt, 1);
    assert.equal(result.error.retryable, false);
    assert.equal(attempts, 1);
    assert.equal(waits, 0);
});

test("embedded page scripts parse without JavaScript syntax errors", () => {
    for (const [name, html] of Object.entries({
        controller_session: controller,
        dashboard,
        remote_session: remoteSession,
        session: sessionRouter
    })) {
        let scriptIndex = 0;
        for (const match of html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script\s*>/gi)) {
            const attributes = match[1] || "";
            if (/\bsrc\s*=/.test(attributes) || /application\/json/i.test(attributes)) continue;
            scriptIndex++;
            assert.doesNotThrow(
                () => new vm.Script(match[2], { filename: `${name}:inline-script-${scriptIndex}` }),
                `${name} inline script ${scriptIndex} must parse`
            );
        }
        assert.ok(scriptIndex > 0, `${name} should contain an inline script`);
    }
});

test("dashboard's B-session health waiter prevents overlapping and stale polls", async () => {
    const start = dashboard.indexOf("function waitForRealBSession() {");
    const end = dashboard.indexOf("window.addEventListener('pagehide', cancelWaitForRealBSession", start);
    assert.notEqual(start, -1);
    assert.notEqual(end, -1);
    const source = dashboard.slice(start, end);
    const timers = new Map();
    let nextTimerId = 0;
    let resolveHealth;
    let healthRequests = 0;
    let syncCalls = 0;
    const context = {
        Date,
        console: { warn() {} },
        setTimeout(callback, delay) {
            const id = ++nextTimerId;
            timers.set(id, { callback, delay });
            return id;
        },
        clearTimeout(id) { timers.delete(id); },
        fetchHealth() {
            healthRequests++;
            return new Promise(resolve => { resolveHealth = resolve; });
        },
        bSessionUiLoaded: true,
        window: { deskStreamBSession: { async sync() { syncCalls++; } } }
    };
    vm.runInNewContext(
        `${source}; globalThis.waitForRealBSession = waitForRealBSession; globalThis.cancelWaitForRealBSession = cancelWaitForRealBSession;`,
        context
    );

    context.waitForRealBSession();
    context.waitForRealBSession();
    assert.equal(timers.size, 1, "only one timeout should be scheduled");
    const [timerId, timer] = timers.entries().next().value;
    timers.delete(timerId);
    const poll = timer.callback();
    context.waitForRealBSession();
    assert.equal(healthRequests, 1, "only one health request should be in flight");
    assert.equal(timers.size, 0, "a duplicate waiter must not be scheduled during a poll");

    context.cancelWaitForRealBSession();
    resolveHealth({ ok: true, async json() { return { in_session: true }; } });
    await poll;
    assert.equal(syncCalls, 0, "a result from a cancelled waiter must not enter a session");
    assert.equal(timers.size, 0, "cancelled polling must not leave a timer behind");
});

test("B chat appends a message once and does not mark it rendered before insertion", () => {
    const start = remoteSession.indexOf("function appendBSessionChat(message) {");
    const end = remoteSession.indexOf("async function pollBSessionChat()", start);
    assert.notEqual(start, -1);
    assert.notEqual(end, -1);
    const nodes = [];
    const messages = {
        scrollHeight: 10,
        scrollTop: 0,
        appendChild(node) { nodes.push(node); }
    };
    const panel = { classList: { add() {} } };
    const context = {
        _bSessionChatRenderedIds: new Set(),
        document: {
            getElementById(id) {
                if (id === "bSessionChatMessages") return messages;
                if (id === "bSessionChatPanel") return panel;
                return null;
            },
            createElement() { return { dataset: {}, className: "", textContent: "" }; }
        }
    };
    vm.runInNewContext(
        `${remoteSession.slice(start, end)}; globalThis.appendBSessionChat = appendBSessionChat;`,
        context
    );
    const message = { id: "42", from_local: false, text: "hello" };

    context.appendBSessionChat(message);
    context.appendBSessionChat(message);
    assert.equal(nodes.length, 1);
    assert.equal(context._bSessionChatRenderedIds.has("42"), true);

    context.document.getElementById = id => id === "bSessionChatMessages" ? null : panel;
    assert.throws(() => context.appendBSessionChat({ id: "43", from_local: true, text: "retry" }));
    assert.equal(context._bSessionChatRenderedIds.has("43"), false);
});

test("controller stream stop releases keys and mouse buttons and canvas binding is guarded", () => {
    const stopStart = controller.indexOf("function stopWebStream() {");
    const stopEnd = controller.indexOf("/* ============================================================\n           STREAM TOGGLE", stopStart);
    const inputStart = controller.indexOf("function releaseAllKeys() {");
    const inputEnd = controller.indexOf("function sendKeyboard", inputStart);
    assert.notEqual(stopStart, -1);
    assert.notEqual(stopEnd, -1);
    assert.notEqual(inputStart, -1);
    assert.notEqual(inputEnd, -1);
    assert.match(controller.slice(stopStart, stopEnd), /releaseAllKeys\(\);/);
    assert.match(controller.slice(inputStart, inputEnd), /activeMouseButtons\.clear\(\)/);
    assert.match(controller, /if \(canvasEventsAttached\) return;/);
    assert.match(controller, /window\.addEventListener\("mouseup", sendMouseUp\)/);
});

test("session router preserves mode, query parameters, and hash when selecting a side", () => {
    const script = sessionRouter.match(/<script>([\s\S]*?)<\/script>/i)?.[1];
    assert.ok(script, "session router script must exist");
    let destination = "";
    const location = {
        href: "http://deskstream.local/session.html?layer=A&integrated=1&embed=1&token=sample#panel",
        replace(value) { destination = value; }
    };
    vm.runInNewContext(script, { URL, URLSearchParams, window: { location } });
    const url = new URL(destination);
    assert.equal(url.pathname.endsWith("/controller_session.html"), true);
    assert.equal(url.searchParams.get("layer"), "A");
    assert.equal(url.searchParams.get("integrated"), "1");
    assert.equal(url.searchParams.get("embed"), "1");
    assert.equal(url.searchParams.get("token"), "sample");
    assert.equal(url.hash, "#panel");
});