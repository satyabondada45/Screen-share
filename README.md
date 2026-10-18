# Screen Share — Bidirectional Remote Desktop

## Overview

Screen Share is a bidirectional remote desktop application that allows you to control any computer from a web browser or native desktop viewer. Every installed machine acts as both **HOST** (screen capture) and **VIEWER** (remote control).

## Architecture

```
[Viewer] --(WebSocket)--> [Relay Server] --(TCP)--> [Host Agent]
                                      <--(TCP)---
     |                                    |
   Browser                  Native desktop apps (Windows EXE)
```

- **desktop-agent.exe** — Headless background service. Captures the screen (H.264 hardware encoder), sends video to relay, receives mouse/keyboard input and injects it into Windows.
- **relay-server.exe** — TCP/WebSocket relay running on port 9001. Routes video and input between viewers and hosts.
- **viewer.exe** — Native desktop viewer with a minifb window (optional alternative to browser).
- **ScreenShare-Tray.ps1** — PowerShell tray launcher with menu (Open Dashboard, Start/Stop Host, Exit).
- **ScreenShare-Setup.ps1** — Windows installer that builds, deploys, and registers everything.

## Deployment

### Prerequisites
- Rust toolchain (rustup): `https://rustup.rs/`
- XAMPP with Apache + MySQL (LAMP stack) for the web dashboard
- Windows 10/11 (host machines)

### Install on each Windows machine

```powershell
# Run the installer (requires admin for firewall rule)
.\desktop-agent\ScreenShare-Setup.ps1
```

This will:
1. Build release binaries from source
2. Install to `%LOCALAPPDATA%\DeskStream\bin\`
3. Register auto-start (relay + tray launcher)
4. Create Windows Firewall rule for TCP 9001
5. Start all services

### Web Dashboard

Open `http://localhost:8080/dashboard.php` in any browser. The dashboard discovers local and registered remote devices.

### Subscription plans and payments

The PHP backend resolves the Welcome, Free, Monthly, and Yearly plans from the `plans` table. Apply the schema/data migrations in order against the configured database before deploying subscription changes:

```powershell
php database/migrations/014_subscription_plans_enhancement.php
php database/migrations/015_subscription_snapshots_and_payments.php
php database/migrations/016_session_entitlement_tracking.php
```

Back up the production database before applying migrations. Migration 014 adds the canonical `plans.slug` column required by authenticated entitlement resolution; applying it before deploying the updated login endpoint prevents a missing-column failure. The production API base is `https://friendssoftwaresolutions.in/DeskStream/api`; deploy `backend/api`, `backend/config`, and `backend/services` to the matching `/api`, `/config`, and `/services` directories under the DeskStream application root. Deploy `frontend/api_path.php` as `/api_path.php` at the DeskStream application root so the API and root-level pages resolve deployment paths consistently.

Subscription assignment records snapshot the plan terms and immutable activation/expiry timestamps. Editing the Welcome Plan changes terms for future registrations only. Expired Welcome and paid subscriptions resolve to Free Plan entitlements on server-side entitlement checks. The Free Plan allows one active remote session per account across both viewer and host roles.

Configure `RAZORPAY_KEY_ID`, `RAZORPAY_KEY_SECRET`, and `RAZORPAY_WEBHOOK_SECRET` in the server environment or the project `.env` file. The subscription page creates INR orders on the server and verifies captured payments with Razorpay before activation. Configure the Razorpay webhook URL as `/DeskStream/api/payments/webhook.php` and subscribe it to the `payment.captured` event. Do not expose the secret values in source control.

## Protocol

### Video Packet (Type 13 / Type 15)
```
[1 byte type] [4 bytes width BE] [4 bytes height BE] [4 bytes payload size BE] [8 bytes timestamp BE] [H.264 NALU data]
```
- Type 13 is the primary format (includes timestamp)
- Type 15 is the legacy format (no timestamp)
- Both are supported by all viewers

### Input Packets (9 bytes)
```
[1 byte type] [2 bytes u16 BE value A] [2 bytes u16 BE value B] [4 bytes reserved]
```
| Type | Event | A | B |
|------|-------|---|---|
| 0 | MOUSE_MOVE | norm_x (0-65535) | norm_y (0-65535) |
| 1 | MOUSE_DOWN (left) | norm_x | norm_y |
| 2 | MOUSE_UP (left) | norm_x | norm_y |
| 3 | MOUSE_DOWN (right) | norm_x | norm_y |
| 4 | MOUSE_UP (right) | norm_x | norm_y |
| 5 | KEY_DOWN | vk_code | reserved |
| 6 | KEY_UP | vk_code | reserved |
| 7 | MOUSE_DOWN (middle) | norm_x | norm_y |
| 8 | MOUSE_UP (middle) | norm_x | norm_y |
| 9 | MOUSE_WHEEL | reserved | scroll_y (i16) |
| 10 | MONITOR_SWITCH | monitor_index | reserved |
| 14 | HEARTBEAT | timestamp (u64) | — |

## Recent Fixes

1. **TYPE 13 video protocol in Rust viewer** — Added missing TYPE 13 packet handler with 20-byte header (4 width + 4 height + 4 size + 8 timestamp). Previously only TYPE 15 was handled, causing video decode failures.

2. **Monitor switch key collision** — F1/F2/F3 now send type 10 (MONITOR_SWITCH) instead of type 7 (MOUSE_DOWN middle), which previously triggered unwanted middle-click events.

3. **Mouse wheel type collision** — Scroll events now send type 9 (MOUSE_WHEEL) instead of type 8 (MOUSE_UP middle), which previously triggered unwanted middle-click-up events.

4. **Cross-machine relay URL** — `session.php` now uses the target device's registered IP address from the database for the WebSocket connection, instead of always defaulting to `ws://localhost:9001`.

5. **Agent auto-approval** — The host agent now sends `[1u8]` approval immediately upon receiving a connection request (type 3), no manual UI interaction needed.

6. **Rust compilation fixes** — Fixed missing imports (`use crate::status;`, `use std::env;`), borrow-after-move in `device_id.rs`, unreachable pattern for type 14, and panic hook lifetime issue.
