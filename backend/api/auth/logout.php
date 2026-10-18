<?php
// backend/api/auth/logout.php
$pathHelper = __DIR__ . '/../../api_path.php';
if (!is_file($pathHelper)) {
    $pathHelper = __DIR__ . '/../../../frontend/api_path.php';
}
require_once $pathHelper;

if (session_status() === PHP_SESSION_NONE) {
    session_start();
}

// Clear all session variables
$_SESSION = [];

// Clear session cookie
if (ini_get("session.use_cookies")) {
    $params = session_get_cookie_params();
    setcookie(session_name(), '', time() - 42000,
        $params["path"], $params["domain"],
        $params["secure"], $params["httponly"]
    );
}

// Destroy session
session_destroy();

if (isset($_SERVER['HTTP_ACCEPT']) && strpos($_SERVER['HTTP_ACCEPT'], 'application/json') !== false) {
    header('Content-Type: application/json');
    echo json_encode(["status" => "success", "message" => "Logged out successfully."]);
    exit();
}

header("Location: " . deskstream_login_url() . "?success=" . urlencode("Logged out successfully."));
exit();
