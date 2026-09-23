<?php
// backend/api/agent/download.php

$exePath = __DIR__ . '/../../../desktop-agent/target/release/desktop-agent.exe';
$version = '1.1.3';

if (!is_file($exePath) || !is_readable($exePath)) {
    http_response_code(404);
    die('Error: Agent executable not found on server.');
}

$fileSize = filesize($exePath);
if ($fileSize === false) {
    http_response_code(500);
    die('Error: Unable to determine agent executable size.');
}

while (ob_get_level() > 0) {
    ob_end_clean();
}

header('Content-Type: application/octet-stream');
header('Content-Disposition: attachment; filename="DeskStream-Agent-v' . $version . '-x64.exe"');
header('Content-Length: ' . $fileSize);

// Cache-busting headers
header('Cache-Control: no-store, no-cache, must-revalidate, max-age=0');
header('Cache-Control: post-check=0, pre-check=0', false);
header('Pragma: no-cache');
header('Expires: 0');

readfile($exePath);
exit;
