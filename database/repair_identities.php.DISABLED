<?php
// database/repair_identities.php
require_once __DIR__ . '/../backend/config/database.php';

echo "=== REPAIRING DEVICE & USER IDENTITIES ===\n";

try {
    // 1. Repair host device (machine_identifier '990f6284-edef-4762-a627-0eb1face7c12') back to its true permanent system_id '238548998'
    $stmt = $pdo->prepare("
        UPDATE devices 
        SET system_id = '238548998', device_uid = '238548998', name = 'SQUIRREL' 
        WHERE machine_identifier = '990f6284-edef-4762-a627-0eb1face7c12'
    ");
    $stmt->execute();
    echo "Restored SQUIRREL host device to permanent System ID: 238548998\n";

    // 2. Clean up any invalid or orphan device records with duplicate user IDs
    $pdo->exec("
        UPDATE devices 
        SET is_online = 0 
        WHERE last_seen_at IS NULL OR last_seen_at < (NOW() - INTERVAL 30 SECOND)
    ");
    echo "Marked inactive devices as is_online = 0\n";

    // 3. Inspect updated devices
    echo "\n=== CURRENT DEVICES TABLE ===\n";
    $dStmt = $pdo->query("SELECT id, user_id, system_id, device_uid, machine_identifier, name, ip_address, is_online, last_seen_at FROM devices");
    while ($row = $dStmt->fetch(PDO::FETCH_ASSOC)) {
        echo sprintf(
            "ID: %d | UserID: %s | SystemID: %s | Machine: %s | Name: %s | IP: %s | Online: %d | LastSeen: %s\n",
            $row['id'],
            $row['user_id'] ?? 'null',
            $row['system_id'] ?? 'null',
            $row['machine_identifier'] ?? 'null',
            $row['name'],
            $row['ip_address'] ?? 'null',
            $row['is_online'],
            $row['last_seen_at'] ?? 'null'
        );
    }

    echo "\nIdentity repair complete.\n";

} catch (PDOException $e) {
    echo "ERROR: " . $e->getMessage() . "\n";
}
