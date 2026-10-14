<?php
require_once __DIR__ . '/../backend/config/database.php';
echo "=== ALL USERS ===\n";
print_r($pdo->query("SELECT id, name, username, email, role, status FROM users")->fetchAll(PDO::FETCH_ASSOC));
