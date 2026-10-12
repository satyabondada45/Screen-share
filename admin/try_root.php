<?php
$pdo = new PDO("mysql:host=localhost;port=3306", "root", "");
$stmt = $pdo->query("SHOW DATABASES");
foreach ($stmt->fetchAll(PDO::FETCH_ASSOC) as $row) {
    echo $row['Database'] . "\n";
}
