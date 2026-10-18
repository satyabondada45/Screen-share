<?php
// backend/api/auth/register.php
// Include database connection using absolute directory path
require_once __DIR__ . '/../../config/database.php';

// Ensure database connection object is available ($conn and $pdo supported)
$db = $conn ?? $pdo ?? null;

if (!$db) {
    http_response_code(500);
    header("Location: /DeskStream/register.php?error=" . urlencode("Database connection unavailable."));
    exit();
}

// Check if request is POST
if ($_SERVER['REQUEST_METHOD'] === 'POST') {
    // Handle both application/x-www-form-urlencoded and application/json requests
    $contentType = $_SERVER['CONTENT_TYPE'] ?? '';
    if (stripos($contentType, 'application/json') !== false) {
        $jsonInput = json_decode(file_get_contents('php://input'), true);
        $username = trim($jsonInput['username'] ?? $jsonInput['name'] ?? '');
        $email = trim($jsonInput['email'] ?? '');
        $password = $jsonInput['password'] ?? '';
        $isJson = true;
    } else {
        $username = trim($_POST['username'] ?? $_POST['name'] ?? '');
        $email = trim($_POST['email'] ?? '');
        $password = $_POST['password'] ?? '';
        $isJson = false;
    }

    // Basic validation
    if (empty($username) || empty($email) || empty($password)) {
        if ($isJson) {
            http_response_code(400);
            echo json_encode(["status" => "error", "message" => "All fields are required."]);
            exit();
        }
        header("Location: /DeskStream/register.php?error=" . urlencode("All fields are required."));
        exit();
    }

    if (!filter_var($email, FILTER_VALIDATE_EMAIL)) {
        if ($isJson) {
            http_response_code(400);
            echo json_encode(["status" => "error", "message" => "Invalid email format."]);
            exit();
        }
        header("Location: /DeskStream/register.php?error=" . urlencode("Invalid email format."));
        exit();
    }

    try {
        // Check if email already exists
        $stmt = $db->prepare("SELECT id, email FROM users WHERE email = ? LIMIT 1");
        $stmt->execute([$email]);
        $existingUser = $stmt->fetch(PDO::FETCH_ASSOC);

        if ($existingUser) {
            if ($isJson) {
                http_response_code(409);
                echo json_encode(["status" => "error", "message" => "Email is already registered."]);
                exit();
            }
            header("Location: /DeskStream/register.php?error=" . urlencode("Email is already registered."));
            exit();
        }

        // Generate a permanent unique User ID (collision-safe 9-digit)
        do {
            $uniqueUserId = (string) random_int(100000000, 999999999);
            $check = $db->prepare("SELECT id FROM users WHERE unique_user_id = ? LIMIT 1");
            $check->execute([$uniqueUserId]);
        } while ($check->fetch());

        // Hash the password securely
        $password_hash = password_hash($password, PASSWORD_DEFAULT);

        // Start transaction for atomic user and subscription creation
        $db->beginTransaction();

        // Insert new user with permanent unique_user_id
        $insert_stmt = $db->prepare("INSERT INTO users (name, username, email, password_hash, unique_user_id, created_at) VALUES (?, ?, ?, ?, ?, NOW())");
        $insert_stmt->execute([$username, $username, $email, $password_hash, $uniqueUserId]);
        $newUserId = (int)$db->lastInsertId();

        // Automatically assign the Welcome Plan with immutable expiry
        require_once __DIR__ . '/../../services/SubscriptionService.php';
        $welcomeSub = SubscriptionService::assignWelcomePlanIfEligible($newUserId, true);
        if (!$welcomeSub) {
            throw new RuntimeException("Welcome Plan auto-activation could not be verified for new account.");
        }

        $db->commit();

        if ($isJson) {
            echo json_encode([
                "status" => "success",
                "message" => "Registration successful! Welcome Plan activated.",
                "user_id" => $newUserId,
                "unique_user_id" => $uniqueUserId,
                "welcome_plan" => [
                    "plan_name" => $welcomeSub['plan_name'] ?? 'Welcome Plan',
                    "duration_value" => $welcomeSub['duration_value'] ?? 6,
                    "duration_unit" => $welcomeSub['duration_unit'] ?? 'months',
                    "starts_at" => $welcomeSub['starts_at'] ?? null,
                    "expires_at" => $welcomeSub['expires_at'] ?? null,
                ]
            ]);
            exit();
        }

        // Redirect to login with success message
        header("Location: /DeskStream/index.php?success=" . urlencode("Registration successful! Welcome Plan activated. Please login."));
        exit();

    } catch (Throwable $e) {
        if ($db->inTransaction()) {
            $db->rollBack();
        }
        error_log("Registration error: " . $e->getMessage());
        if ($isJson) {
            http_response_code(500);
            echo json_encode(["status" => "error", "message" => "Registration failed: " . $e->getMessage()]);
            exit();
        }
        header("Location: /DeskStream/register.php?error=" . urlencode("Registration failed: " . $e->getMessage()));
        exit();
    }
} else {
    // If someone accesses the API file directly via GET
    header("Location: /DeskStream/register.php");
    exit();
}