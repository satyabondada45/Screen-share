<?php
// backend/api/agent/download.php
// Serves the unified DeskStream.exe — the single application containing
// both the Controller UI and the embedded Windows Agent engine.
// The user downloads one file. No separate DeskStream-Agent.exe needed.

$exePath = __DIR__ . '/../../../DESKSTREAM/DeskStream.exe';
$version = '1.2.0';

if (!is_file($exePath) || !is_readable($exePath)) {
    http_response_code(404);
    die('Error: DeskStream executable not found on server.');
}

$fileSize = filesize($exePath);
if ($fileSize === false) {
    http_response_code(500);
    die('Error: Unable to determine executable size.');
}

while (ob_get_level() > 0) {
    ob_end_clean();
}

header('Content-Type: application/octet-stream');
header('Content-Disposition: attachment; filename="DeskStream-v' . $version . '-x64.exe"');
header('Content-Length: ' . $fileSize);

// Cache-busting headers
header('Cache-Control: no-store, no-cache, must-revalidate, max-age=0');
header('Cache-Control: post-check=0, pre-check=0', false);
header('Pragma: no-cache');
header('Expires: 0');

readfile($exePath);
exit;
