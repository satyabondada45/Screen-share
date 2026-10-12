<?php
require 'admin/includes/db.php';
$pdo = admin_db();
$stmt = $pdo->query('SHOW COLUMNS FROM devices');
print_r($stmt->fetchAll(PDO::FETCH_ASSOC));
