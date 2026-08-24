<?php
require_once '../../config/database.php';

// Start a PHP session so we can remember who is logged in
session_start();

if ($_SERVER['REQUEST_METHOD'] === 'POST') {
    $email = trim($_POST['email'] ?? '');
    $password = $_POST['password'] ?? '';

    if (empty($email) || empty($password)) {
        header("Location: ../../../frontend/login.php?error=" . urlencode("All fields are required."));
        exit();
    }

    try {
        // Find user by email
        $stmt = $conn->prepare("SELECT id, username, password_hash FROM users WHERE email = ?");
        $stmt->execute([$email]);
        $user = $stmt->fetch(PDO::FETCH_ASSOC);

        // Check if user exists and password is correct
        if ($user && password_verify($password, $user['password_hash'])) {
            // Set session variables
            $_SESSION['user_id'] = $user['id'];
            $_SESSION['username'] = $user['username'];

            // Redirect to dashboard
            header("Location: ../../../frontend/dashboard.php");
            exit();
        } else {
            header("Location: ../../../frontend/login.php?error=" . urlencode("Invalid email or password."));
            exit();
        }

    } catch (PDOException $e) {
        header("Location: ../../../frontend/login.php?error=" . urlencode("Database error: " . $e->getMessage()));
        exit();
    }
} else {
    header("Location: ../../../frontend/login.php");
    exit();
}
?>