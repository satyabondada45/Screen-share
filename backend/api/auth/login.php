<?php
// backend/api/auth/login.php
require_once __DIR__ . '/../../config/database.php';

$db = $conn ?? $pdo ?? null;

// Start session
if (session_status() === PHP_SESSION_NONE) {
    session_start();
}

if ($_SERVER['REQUEST_METHOD'] === 'POST') {
    $contentType = $_SERVER['CONTENT_TYPE'] ?? '';
    if (stripos($contentType, 'application/json') !== false) {
        $jsonInput = json_decode(file_get_contents('php://input'), true);
        $email    = trim($jsonInput['email'] ?? $jsonInput['username'] ?? '');
        $password = $jsonInput['password'] ?? '';
        $isJson   = true;
    } else {
        $email    = trim($_POST['email'] ?? $_POST['username'] ?? '');
        $password = $_POST['password'] ?? '';
        $isJson   = false;
    }

    if (empty($email) || empty($password)) {
        if ($isJson) {
            http_response_code(400);
            echo json_encode(["status" => "error", "success" => false, "message" => "All fields are required."]);
            exit();
        }
        header("Location: ../../../frontend/index.php?error=" . urlencode("All fields are required."));
        exit();
    }

    try {
        // Find user by email or username
        $stmt = $db->prepare("SELECT id, username, email, password_hash, unique_user_id FROM users WHERE email = ? OR username = ? LIMIT 1");
        $stmt->execute([$email, $email]);
        $user = $stmt->fetch(PDO::FETCH_ASSOC);

        // Check if user exists and password is correct
        if ($user && password_verify($password, $user['password_hash'])) {
            // Use existing permanent unique_user_id — NEVER generate a new one
            $uniqueUserId = $user['unique_user_id'];
            if (empty($uniqueUserId)) {
                // Legacy account without unique_user_id — generate one now and store permanently
                do {
                    $candidate = (string)random_int(100000000, 999999999);
                    $check = $db->prepare("SELECT id FROM users WHERE unique_user_id = ? LIMIT 1");
                    $check->execute([$candidate]);
                } while ($check->fetch());
                $update = $db->prepare("UPDATE users SET unique_user_id = ? WHERE id = ?");
                $update->execute([$candidate, $user['id']]);
                $uniqueUserId = $candidate;
            }

            // Establish clean server session
            $_SESSION['user_id'] = (int)$user['id'];
            $_SESSION['username'] = $user['username'];
            $_SESSION['email'] = $user['email'];
            $_SESSION['unique_user_id'] = $uniqueUserId;

            // Fetch user's registered system if already registered by the Rust desktop agent
            $sysStmt = $db->prepare("
                SELECT *, 
                       (last_seen_at >= (NOW() - INTERVAL 30 SECOND) AND is_online = 1) AS is_live_online 
                FROM devices 
                WHERE user_id = ? 
                ORDER BY is_live_online DESC, last_seen_at DESC, id ASC 
                LIMIT 1
            ");
            $sysStmt->execute([$user['id']]);
            $userSystem = $sysStmt->fetch(PDO::FETCH_ASSOC);

            if ($isJson) {
                echo json_encode([
                    "status" => "success",
                    "success" => true,
                    "message" => "Login successful",
                    "user" => [
                        "id" => (int)$user['id'],
                        "name" => $user['username'],
                        "username" => $user['username'],
                        "email" => $user['email'],
                        "unique_user_id" => $uniqueUserId
                    ],
                    "system" => $userSystem ? [
                        "system_id" => (string)($userSystem['system_id'] ?: $userSystem['device_uid']),
                        "device_name" => $userSystem['name'],
                        "status" => !empty($userSystem['is_live_online']) ? "online" : "offline"
                    ] : null
                ]);
                exit();
            }

            header("Location: ../../../frontend/dashboard.php");
            exit();
        } else {
            if ($isJson) {
                http_response_code(401);
                echo json_encode(["status" => "error", "success" => false, "message" => "Invalid email or password."]);
                exit();
            }
            header("Location: ../../../frontend/index.php?error=" . urlencode("Invalid email or password."));
            exit();
        }

    } catch (\PDOException $e) {
        if ($isJson) {
            http_response_code(500);
            echo json_encode(["status" => "error", "success" => false, "message" => "Database error: " . $e->getMessage()]);
            exit();
        }
        header("Location: ../../../frontend/index.php?error=" . urlencode("Database error: " . $e->getMessage()));
        exit();
    }
} else {
    header("Location: ../../../frontend/index.php");
    exit();
}