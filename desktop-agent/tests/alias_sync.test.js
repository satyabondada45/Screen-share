const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

const dashboardHtml = fs.readFileSync(
    path.join(__dirname, "..", "assets", "dashboard.html"),
    "utf8"
);

function createMockEnvironment(initialState = {}) {
    const storage = new Map();
    if (initialState.localStorage) {
        for (const [k, v] of Object.entries(initialState.localStorage)) {
            storage.set(k, String(v));
        }
    }

    const localStorage = {
        getItem: (k) => (storage.has(k) ? storage.get(k) : null),
        setItem: (k, v) => storage.set(k, String(v)),
        removeItem: (k) => storage.delete(k),
        clear: () => storage.clear()
    };

    const elements = new Map();
    function createMockElement(id, tag = "div") {
        const el = {
            id,
            tagName: tag.toUpperCase(),
            textContent: "",
            value: "",
            style: {},
            classList: {
                classes: new Set(),
                add(c) { this.classes.add(c); },
                remove(c) { this.classes.delete(c); },
                toggle(c, force) {
                    if (force !== undefined) {
                        force ? this.classes.add(c) : this.classes.delete(c);
                    } else {
                        this.classes.has(c) ? this.classes.delete(c) : this.classes.add(c);
                    }
                },
                contains(c) { return this.classes.has(c); }
            },
            disabled: false,
            focusCalled: false,
            selectCalled: false,
            focus() { this.focusCalled = true; },
            select() { this.selectCalled = true; },
            scrollIntoView() {},
            addEventListener: () => {},
            removeEventListener: () => {},
            replaceChildren: () => {},
            appendChild: () => {}
        };
        elements.set(id, el);
        return el;
    }

    createMockElement("mainHostAlias", "span");
    createMockElement("sidebarAliasDisplay", "strong");
    createMockElement("hostAliasInput", "input");
    createMockElement("mainHostId", "div");
    createMockElement("sidebarIdDisplay", "div");
    createMockElement("topbarUsername", "span");
    createMockElement("topbarUserBadge", "div");
    createMockElement("loginOverlay", "div");
    createMockElement("registerOverlay", "div");
    createMockElement("mainTopbar", "div");
    createMockElement("mainWorkspace", "div");
    createMockElement("recentDropdown", "div");
    createMockElement("remoteIdInput", "input");
    createMockElement("agentStatusFooter", "div");
    createMockElement("agentReconnectBtn", "button");
    createMockElement("toastBox", "div");
    createMockElement("toastMsg", "div");

    const toasts = [];
    const fetchCalls = [];
    let fetchHandler = async (url, opts) => ({
        ok: true,
        status: 200,
        json: async () => ({ status: "success", alias: "Updated Alias" }),
        text: async () => JSON.stringify({ status: "success", alias: "Updated Alias" })
    });

    const document = {
        getElementById: (id) => elements.get(id) || null,
        querySelectorAll: (selector) => {
            const result = [];
            const ids = selector.split(",").map(s => s.trim().replace(/^#/, ""));
            for (const id of ids) {
                if (elements.has(id)) result.push(elements.get(id));
            }
            return result;
        },
        querySelector: () => null,
        activeElement: null,
        addEventListener: () => {},
        createElement: (tag) => createMockElement("dyn_" + Math.random(), tag)
    };

    const window = {
        location: { href: "http://127.0.0.1:49182/dashboard.html", pathname: "/dashboard.html", search: "" },
        addEventListener: () => {},
        deskStreamBSession: { sync: () => Promise.resolve() }
    };

    const context = {
        window,
        document,
        localStorage,
        URLSearchParams,
        URL,
        DOMParser: class {
            parseFromString() {
                return {
                    getElementById: (id) => elements.get(id) || null,
                    querySelector: () => null,
                    querySelectorAll: () => [],
                    body: { innerHTML: "" }
                };
            }
        },
        navigator: { clipboard: { writeText: async () => {} } },
        fetch: async (url, opts) => {
            fetchCalls.push({ url, opts });
            return fetchHandler(url, opts);
        },
        console: { info: () => {}, warn: () => {}, error: () => {}, log: () => {} },
        setTimeout: (fn) => fn(),
        setInterval: () => 123,
        clearInterval: () => {},
        showDashboardPage: () => {},
        pollAgentHealth: async () => {},
        renderRecentSessionHistory: () => {},
        requestWindowState: () => {},
        loadBSessionUi: () => Promise.resolve(),
        bSessionUiLoaded: true,
        IS_B_SESSION_OVERLAY: false,
        updateIdDisplays: () => {},
        updateHostBadge: () => {},
        setAgentHealthState: () => {},
        getRecentSessionEntries: () => [],
        populateRecentDropdown: () => {},
        checkIncoming: async () => {},
        processPendingReverseAcceptance: () => {},
        formatId: (id) => id
    };

    vm.createContext(context);

    // Run all script blocks
    const scriptMatches = [...dashboardHtml.matchAll(/<script[\s\S]*?>([\s\S]*?)<\/script>/gi)];
    for (const match of scriptMatches) {
        vm.runInContext(match[1], context);
    }

    // Intercept showToast to capture toasts
    const origShowToast = context.window.showToast || context.showToast;
    const capturedToast = (msg) => {
        toasts.push(msg);
        if (typeof origShowToast === "function") origShowToast(msg);
    };
    context.window.showToast = capturedToast;
    context.showToast = capturedToast;

    return {
        context,
        elements,
        toasts,
        fetchCalls,
        setFetchHandler: (fn) => { fetchHandler = fn; },
        storage,
        window: context.window
    };
}

function getAlias(env) {
    return env.window.currentHostAlias || env.context.currentHostAlias || env.elements.get("mainHostAlias").textContent;
}

test("1. Change alias from 'satya' to another name and verify both visible cards update immediately", async () => {
    const env = createMockEnvironment({
        localStorage: {
            ds_username: "satya",
            ds_session_token: "test_token_123"
        }
    });

    env.window.setCurrentHostUid("123456789");

    // Set initial state matching problem: alias currently displaying username 'satya'
    env.window.updateAuthoritativeAlias("satya");
    assert.equal(env.elements.get("mainHostAlias").textContent, "satya");
    assert.equal(env.elements.get("sidebarAliasDisplay").textContent, "satya");

    // Trigger edit
    env.window.triggerAliasEdit();
    const input = env.elements.get("hostAliasInput");
    assert.equal(input.style.display, "inline-block");
    assert.equal(input.value, "satya");

    // Mock successful backend response
    env.setFetchHandler(async (url, opts) => {
        const body = JSON.parse(opts.body);
        assert.equal(body.device_uid, "123456789");
        assert.equal(body.alias, "Office-Workstation");
        return {
            ok: true,
            status: 200,
            json: async () => ({ status: "success", alias: "Office-Workstation" }),
            text: async () => JSON.stringify({ status: "success", alias: "Office-Workstation" })
        };
    });

    // Enter new name and save
    input.value = "Office-Workstation";
    await env.window.saveAliasInline();

    // Verify both visible cards update immediately
    assert.equal(env.elements.get("mainHostAlias").textContent, "Office-Workstation");
    assert.equal(env.elements.get("sidebarAliasDisplay").textContent, "Office-Workstation");
    assert.equal(getAlias(env), "Office-Workstation");

    // Verify authoritative cache is updated
    assert.equal(env.storage.get("ds_device_alias"), "Office-Workstation");

    // Verify input editor closed
    assert.equal(input.style.display, "none");
    assert.equal(env.elements.get("mainHostAlias").style.display, "inline-block");
    assert.ok(env.toasts.some(t => t.includes('Device alias updated to "Office-Workstation"')));
});

test("2. Change alias again without restarting application", async () => {
    const env = createMockEnvironment({
        localStorage: {
            ds_username: "satya",
            ds_device_alias: "Office-Workstation"
        }
    });

    env.window.setCurrentHostUid("123456789");
    env.window.updateAuthoritativeAlias("Office-Workstation");

    // Second edit
    env.window.triggerAliasEdit();
    const input = env.elements.get("hostAliasInput");
    input.value = "Dev-Box-2";

    env.setFetchHandler(async (url, opts) => {
        const body = JSON.parse(opts.body);
        assert.equal(body.alias, "Dev-Box-2");
        return {
            ok: true,
            status: 200,
            json: async () => ({ status: "success", alias: "Dev-Box-2" }),
            text: async () => JSON.stringify({ status: "success", alias: "Dev-Box-2" })
        };
    });

    await env.window.saveAliasInline();

    // Immediately reflects second change
    assert.equal(env.elements.get("mainHostAlias").textContent, "Dev-Box-2");
    assert.equal(env.elements.get("sidebarAliasDisplay").textContent, "Dev-Box-2");
    assert.equal(getAlias(env), "Dev-Box-2");
    assert.equal(env.storage.get("ds_device_alias"), "Dev-Box-2");
});

test("3. Refresh or polling does not overwrite new alias with stale user account name", async () => {
    const env = createMockEnvironment({
        localStorage: {
            ds_username: "satya",
            ds_device_alias: "Dev-Box-2"
        }
    });

    env.window.setCurrentHostUid("123456789");
    env.context.isAuthenticated = true;
    env.window.updateAuthoritativeAlias("Dev-Box-2");

    // Simulate fetchAllDevices background poll where server returns device list with saved alias
    env.setFetchHandler(async (url) => {
        if (url.includes("/api/devices/list.php")) {
            return {
                ok: true,
                status: 200,
                json: async () => ({
                    status: "success",
                    devices: [
                        { system_id: "123456789", device_uid: "123456789", name: "Dev-Box-2" },
                        { system_id: "999888777", device_uid: "999888777", name: "Other PC" }
                    ]
                }),
                text: async () => JSON.stringify({ status: "success", devices: [] })
            };
        }
        return { ok: true, status: 200, json: async () => ({ status: "success" }) };
    });

    await env.context.fetchAllDevices(false);

    // CRITICAL: Must NOT be overwritten by ds_username "satya"
    assert.equal(env.elements.get("mainHostAlias").textContent, "Dev-Box-2");
    assert.equal(env.elements.get("sidebarAliasDisplay").textContent, "Dev-Box-2");
    assert.equal(getAlias(env), "Dev-Box-2");
});

test("4. Startup initialization loads the saved alias from authoritative storage", () => {
    const env = createMockEnvironment({
        localStorage: {
            ds_username: "satya",
            ds_device_alias: "Saved-Server-Alias"
        }
    });

    env.window.applyDisplayIdentity();

    // Must resolve to "Saved-Server-Alias", NOT "satya"
    assert.equal(env.elements.get("mainHostAlias").textContent, "Saved-Server-Alias");
    assert.equal(env.elements.get("sidebarAliasDisplay").textContent, "Saved-Server-Alias");
    assert.equal(getAlias(env), "Saved-Server-Alias");
});

test("5. Authentication handler preserves device alias and does not overwrite with username", async () => {
    const env = createMockEnvironment({
        localStorage: {
            ds_username: "satya",
            ds_device_alias: "My-Workstation"
        }
    });

    env.window.updateAuthoritativeAlias("My-Workstation");

    // Call onAuthenticated("satya")
    await env.context.onAuthenticated("satya");

    // Topbar has user badge
    assert.equal(env.elements.get("topbarUsername").textContent, "👤 satya");

    // Device alias cards must RETAIN "My-Workstation"
    assert.equal(env.elements.get("mainHostAlias").textContent, "My-Workstation");
    assert.equal(env.elements.get("sidebarAliasDisplay").textContent, "My-Workstation");
    assert.equal(getAlias(env), "My-Workstation");
});

test("6. Simulate API failure: UI does not falsely report success and retains last confirmed alias", async () => {
    const env = createMockEnvironment({
        localStorage: {
            ds_username: "satya",
            ds_device_alias: "Original-Name"
        }
    });

    env.window.setCurrentHostUid("123456789");
    env.window.updateAuthoritativeAlias("Original-Name");

    env.window.triggerAliasEdit();
    const input = env.elements.get("hostAliasInput");
    input.value = "Failed-New-Name";

    // Simulate 500 server error
    env.setFetchHandler(async () => ({
        ok: false,
        status: 500,
        json: async () => ({ status: "error", message: "Database failure" }),
        text: async () => JSON.stringify({ status: "error", message: "Database failure" })
    }));

    await env.window.saveAliasInline();

    // Must retain original confirmed alias
    assert.equal(getAlias(env), "Original-Name");
    assert.equal(env.elements.get("mainHostAlias").textContent, "Original-Name");
    assert.equal(env.elements.get("sidebarAliasDisplay").textContent, "Original-Name");

    // Must NOT report success; must show actual error
    assert.ok(env.toasts.some(t => t.includes("Database failure") || t.includes("Server error")));
    assert.ok(!env.toasts.some(t => t.includes('Device alias updated to "Failed-New-Name"')));
});

test("7. Edge case: Empty or whitespace-only input falls back to Workstation", async () => {
    const env = createMockEnvironment({
        localStorage: {
            ds_username: "satya",
            ds_device_alias: "Existing-Alias"
        }
    });

    env.window.setCurrentHostUid("123456789");
    env.window.updateAuthoritativeAlias("Existing-Alias");

    env.window.triggerAliasEdit();
    const input = env.elements.get("hostAliasInput");
    input.value = "     ";

    env.setFetchHandler(async (url, opts) => {
        const body = JSON.parse(opts.body);
        assert.equal(body.alias, "Workstation");
        return {
            ok: true,
            status: 200,
            json: async () => ({ status: "success", alias: "Workstation" }),
            text: async () => JSON.stringify({ status: "success", alias: "Workstation" })
        };
    });

    await env.window.saveAliasInline();

    assert.equal(getAlias(env), "Workstation");
    assert.equal(env.elements.get("mainHostAlias").textContent, "Workstation");
    assert.equal(env.elements.get("sidebarAliasDisplay").textContent, "Workstation");
});

test("8. Edge case: Duplicate save requests are prevented while save is in flight", async () => {
    const env = createMockEnvironment({
        localStorage: {
            ds_username: "satya",
            ds_device_alias: "Name-1"
        }
    });

    env.window.setCurrentHostUid("123456789");
    env.window.updateAuthoritativeAlias("Name-1");

    let fetchResolve;
    env.setFetchHandler((url) => {
        if (url.includes("/api/devices/update.php")) {
            return new Promise(resolve => {
                fetchResolve = resolve;
            });
        }
        return { ok: true, status: 200, json: async () => ({ status: "success" }) };
    });

    env.window.triggerAliasEdit();
    const input = env.elements.get("hostAliasInput");
    input.value = "New-Name";

    // Launch first save (will stall on fetch)
    const p1 = env.window.saveAliasInline();

    // Attempt second save concurrently
    const p2 = env.window.saveAliasInline();

    const updateCalls = env.fetchCalls.filter(c => c.url.includes("/api/devices/update.php"));
    assert.equal(updateCalls.length, 1, "Only one network request should be sent");

    fetchResolve({
        ok: true,
        status: 200,
        json: async () => ({ status: "success", alias: "New-Name" }),
        text: async () => JSON.stringify({ status: "success", alias: "New-Name" })
    });

    await Promise.all([p1, p2]);
    assert.equal(getAlias(env), "New-Name");
});

test("9. Edge case: Escape key cancels edit without sending network request", async () => {
    const env = createMockEnvironment({
        localStorage: {
            ds_username: "satya",
            ds_device_alias: "Stable-Name"
        }
    });

    env.window.setCurrentHostUid("123456789");
    env.window.updateAuthoritativeAlias("Stable-Name");

    env.window.triggerAliasEdit();
    const input = env.elements.get("hostAliasInput");
    input.value = "Cancelled-Typing";

    // Clear any calls from initialization
    env.fetchCalls.length = 0;

    // Press Escape
    env.window.handleAliasKey({ key: "Escape", preventDefault: () => {} });

    const updateCalls = env.fetchCalls.filter(c => c.url.includes("/devices/update.php"));
    assert.equal(updateCalls.length, 0, "No network request should be made on Escape");
    assert.equal(input.style.display, "none");
    assert.equal(env.elements.get("mainHostAlias").style.display, "inline-block");
    assert.equal(env.elements.get("mainHostAlias").textContent, "Stable-Name");
    assert.equal(getAlias(env), "Stable-Name");
});
