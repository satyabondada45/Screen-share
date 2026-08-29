<?php
require 'backend/config/database.php';
$stmt = $pdo->query("SELECT NOW() as db_time, last_seen_at, (last_seen_at >= (NOW() - INTERVAL 30 SECOND)) as is_recent FROM devices WHERE system_id = '238548998'");
print_r($stmt->fetchAll(PDO::FETCH_ASSOC));
?>
