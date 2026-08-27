<?php
$host = '127.0.0.1';
$db   = 'screen_share_db';
$user = 'root';
$pass = '3141';
$charset = 'utf8mb4';

$dsn = "mysql:host=$host;dbname=$db;charset=$charset";
$options = [
    PDO::ATTR_ERRMODE            => PDO::ERRMODE_EXCEPTION,
    PDO::ATTR_DEFAULT_FETCH_MODE => PDO::FETCH_ASSOC,
    PDO::ATTR_EMULATE_PREPARES   => false,
];

date_default_timezone_set('UTC');

try {
    $pdo = new PDO($dsn, $user, $pass, $options);
    $pdo->exec("SET time_zone = '+00:00'");
    $conn = $pdo; // Alias for backward compatibility across all modules
} catch (\PDOException $e) {
    http_response_code(500);
    echo json_encode(["status" => "error", "message" => "Database connection failed"]);
    exit;
}