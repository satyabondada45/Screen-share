<?php
// Include database connection
require_once '../../config/database.php';

// Check if request is POST
if ($_SERVER['REQUEST_METHOD'] === 'POST') {
    // Collect and sanitize inputs
    $username = trim($_POST['username'] ?? '');
    $email = trim($_POST['email'] ?? '');
    $password = $_POST['password'] ?? '';

    // Basic validation
    if (empty($username) || empty($email) || empty($password)) {
        header("Location: ../../../frontend/register.php?error=" . urlencode("All fields are required."));
        exit();
    }

    if (!filter_var($email, FILTER_VALIDATE_EMAIL)) {
        header("Location: ../../../frontend/register.php?error=" . urlencode("Invalid email format."));
        exit();
    }

    try {
        // Check if email already exists
        $stmt = $conn->prepare("SELECT id FROM users WHERE email = ?");
        $stmt->execute([$email]);

        if ($stmt->rowCount() > 0) {
            header("Location: ../../../frontend/register.php?error=" . urlencode("Email is already registered."));
            exit();
        }

        // Hash the password securely
        $password_hash = password_hash($password, PASSWORD_BCRYPT);

        // Insert new user using a prepared statement
        $insert_stmt = $conn->prepare("INSERT INTO users (username, email, password_hash) VALUES (?, ?, ?)");
        $insert_stmt->execute([$username, $email, $password_hash]);

        // Redirect to login with success message
        header("Location: ../../../frontend/login.php?success=" . urlencode("Registration successful! Please login."));
        exit();

    } catch (PDOException $e) {
        header("Location: ../../../frontend/register.php?error=" . urlencode("Database error: " . $e->getMessage()));
        exit();
    }
} else {
    // If someone accesses the API file directly via URL without submitting form
    header("Location: ../../../frontend/register.php");
    exit();
}
?>