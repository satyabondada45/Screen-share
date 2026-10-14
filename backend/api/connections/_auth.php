<?php

function requireConnectionUser(): int
{
    if (session_status() === PHP_SESSION_NONE) {
        session_start();
    }

    $userId = filter_var(
        $_SESSION['user_id'] ?? null,
        FILTER_VALIDATE_INT,
        ['options' => ['min_range' => 1]]
    );

    if ($userId === false) {
        http_response_code(401);
        echo json_encode([
            'status' => 'error',
            'code' => 'AUTH_REQUIRED',
            'message' => 'Sign in to manage connection requests.'
        ]);
        exit();
    }

    return $userId;
}
