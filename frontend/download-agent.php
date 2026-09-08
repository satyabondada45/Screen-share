<?php
// Screen Share — Windows download page
?>
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Download Screen Share for Windows</title>
  <style>
    body { font-family: "Segoe UI", system-ui, sans-serif; background:#0f172a; color:#e2e8f0; margin:0; }
    .wrap { max-width: 720px; margin: 8vh auto; padding: 0 20px; }
    h1 { font-size: 28px; }
    .card { background:#1e293b; border:1px solid #334155; border-radius:14px; padding:28px; }
    .btn {
      display:inline-block; margin-top:18px; padding:14px 22px; border-radius:10px;
      background:#2563eb; color:#fff; text-decoration:none; font-weight:600; font-size:16px;
    }
    .btn:hover { background:#1d4ed8; }
    .muted { color:#94a3b8; font-size:14px; }
    ul { line-height:1.7; }
  </style>
</head>
<body>
  <div class="wrap">
    <div class="card">
      <h1>Download Screen Share for Windows</h1>
      <p class="muted">Remote desktop agent for Windows 10 / 11 (64-bit). Includes the Screen Share
        control application and the capture/streaming agent in a single installer.</p>
      <ul>
        <li>Professional system-tray GUI</li>
        <li>Persistent, authoritative System ID</li>
        <li>Online / offline, relay &amp; backend status</li>
        <li>Starts with Windows (optional)</li>
        <li>Clean uninstall via Windows Settings</li>
      </ul>
      <a class="btn" href="downloads/ScreenShare-Setup.exe">Download Screen Share for Windows</a>
      <p class="muted" style="margin-top:18px;">
        Legacy agent-only package:
        <a href="downloads/DeskStream-Agent-Installer.zip">DeskStream-Agent-Installer.zip</a>
      </p>
    </div>
  </div>
</body>
</html>
