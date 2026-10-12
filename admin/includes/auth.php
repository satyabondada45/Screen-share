<?php
require_once __DIR__ . '/db.php';

function admin_require_auth(): void
{
    if (empty($_SESSION['admin_logged_in']) || empty($_SESSION['admin_user_id'])) {
        header('Location: ' . admin_url('login.php'));
        exit();
    }
}

function admin_logout(): void
{
    $_SESSION = [];
    if (ini_get('session.use_cookies')) {
        $params = session_get_cookie_params();
        setcookie(session_name(), '', time() - 42000, $params['path'], $params['domain'], $params['secure'], $params['httponly']);
    }
    session_destroy();
    header('Location: ' . admin_url('login.php'));
    exit();
}
