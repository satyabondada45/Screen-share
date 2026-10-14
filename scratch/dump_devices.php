<?php
try {
    $pdo = new PDO("mysql:host=127.0.0.1;port=3306;dbname=screen_share_db", "root", "3141");
    $stmt = $pdo->query("SELECT * FROM devices");
    while ($row = $stmt->fetch(PDO::FETCH_ASSOC)) {
        echo json_encode($row) . "\n";
    }
} catch (Exception $e) {
    echo "Error: " . $e->getMessage() . "\n";
}
