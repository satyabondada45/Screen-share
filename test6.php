<?php
require 'backend/config/database.php';
$stmt = $pdo->query("SELECT id, user_id, system_id, machine_identifier, name, is_online, last_seen_at FROM devices");
print_r($stmt->fetchAll(PDO::FETCH_ASSOC));
?>
