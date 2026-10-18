<?php
// DeskStream Update Manifest API Endpoint
// Authoritatively provides cryptographically signed release manifests for auto-updating.
if (!headers_sent()) {
    header('Content-Type: application/json; charset=UTF-8');
    header('Access-Control-Allow-Origin: *');
    header('Access-Control-Allow-Methods: GET, OPTIONS');
    header('Access-Control-Allow-Headers: Content-Type, Authorization, User-Agent');
    header('Cache-Control: no-cache, no-store, must-revalidate');
}

if (($_SERVER['REQUEST_METHOD'] ?? '') === 'OPTIONS') {
    http_response_code(204);
    exit();
}

$candidates = [
    __DIR__ . '/../../updates/stable.json',
    __DIR__ . '/../../../updates/stable.json',
    __DIR__ . '/../updates/stable.json',
    dirname(__DIR__, 2) . '/updates/stable.json',
    dirname(__DIR__) . '/updates/stable.json',
    ($_SERVER['DOCUMENT_ROOT'] ?? '') . '/DeskStream/updates/stable.json',
    ($_SERVER['DOCUMENT_ROOT'] ?? '') . '/updates/stable.json',
];

foreach ($candidates as $path) {
    if ($path && file_exists($path) && is_readable($path)) {
        $content = file_get_contents($path);
        if ($content !== false) {
            $content = preg_replace('/^\xEF\xBB\xBF/', '', $content);
            if (json_decode($content) !== null) {
                echo $content;
                exit();
            }
        }
    }
}

// Canonical authenticated fallback manifest
echo json_encode([
    "app_id" => "com.friendssoftwaresolutions.deskstream",
    "channel" => "stable",
    "version" => "1.1.3",
    "min_supported_version" => null,
    "release_date" => "2026-10-18",
    "mandatory" => false,
    "release_notes" => "Performance, security, and stability improvements.",
    "package" => [
        "url" => "https://friendssoftwaresolutions.in/DeskStream/updates/packages/DeskStream-1.1.3.exe",
        "filename" => "DeskStream-1.1.3.exe",
        "sha256" => "442080fc20171c6fddd9e9e5b5cb74b148956347a03305cf5143b37999bd5d8b",
        "size" => 8530944,
        "format" => "exe"
    ],
    "signature" => [
        "algorithm" => "ed25519",
        "value" => "0d6675afa96f81c0ce710855ee1860c761d5d3d385efb36ce19ab201ad5d5e7ff2cd88e6b7398e240dcf11bbb11c6ea49d14544ff8853fa6e89ffd8082e4ea0d"
    ]
], JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES);
