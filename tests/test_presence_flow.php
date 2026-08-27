<?php
// tests/test_presence_flow.php
require_once __DIR__ . '/../backend/config/database.php';

echo "=== TESTING TWO-LAPTOP IDENTITY & PRESENCE FLOW ===\n\n";

function postJson($url, $data) {
    $ch = curl_init($url);
    curl_setopt($ch, CURLOPT_POSTFIELDS, json_encode($data));
    curl_setopt($ch, CURLOPT_HTTPHEADER, ['Content-Type: application/json']);
    curl_setopt($ch, CURLOPT_RETURNTRANSFER, true);
    $res = curl_exec($ch);
    $info = curl_getinfo($ch, CURLINFO_HTTP_CODE);
    curl_close($ch);
    return ['code' => $info, 'body' => json_decode($res, true), 'raw' => $res];
}

// TEST 1: Register Laptop A
echo "1. Registering Laptop A (SQUIRREL - UUID 990f6284-edef-4762-a627-0eb1face7c12)...\n";
$laptopA = postJson('http://127.0.0.1/Screen%20Share/backend/api/devices/register.php', [
    'machine_identifier' => '990f6284-edef-4762-a627-0eb1face7c12',
    'system_id' => '238548998',
    'hostname' => 'SQUIRREL',
    'os_info' => 'windows',
    'user_id' => 3
]);
echo "Laptop A Status: " . ($laptopA['code'] === 200 ? "OK" : "FAILED") . "\n";
echo "Laptop A System ID: " . ($laptopA['body']['system']['system_id'] ?? 'N/A') . "\n";

// TEST 2: Register Laptop B under SAME user (user_id = 3)
echo "\n2. Registering Laptop B (LAPTOP-B - UUID 11223344-5566-7788-99aa-bbccddeeff00)...\n";
$laptopB = postJson('http://127.0.0.1/Screen%20Share/backend/api/devices/register.php', [
    'machine_identifier' => '11223344-5566-7788-99aa-bbccddeeff00',
    'system_id' => '834721906',
    'hostname' => 'LAPTOP-B',
    'os_info' => 'windows',
    'user_id' => 3
]);
echo "Laptop B Status: " . ($laptopB['code'] === 200 ? "OK" : "FAILED") . "\n";
echo "Laptop B System ID: " . ($laptopB['body']['system']['system_id'] ?? 'N/A') . "\n";

// Verify Laptop A != Laptop B
$idA = $laptopA['body']['system']['system_id'] ?? '';
$idB = $laptopB['body']['system']['system_id'] ?? '';
if ($idA !== $idB && !empty($idA) && !empty($idB)) {
    echo "SUCCESS: Laptop A ($idA) != Laptop B ($idB)\n";
} else {
    echo "FAILED: IDs collided or invalid! ($idA vs $idB)\n";
}

// TEST 3: Send Heartbeats for both
echo "\n3. Sending heartbeats for both laptops...\n";
$hbA = postJson('http://127.0.0.1/Screen%20Share/backend/api/devices/heartbeat.php', [
    'machine_identifier' => '990f6284-edef-4762-a627-0eb1face7c12',
    'system_id' => '238548998'
]);
$hbB = postJson('http://127.0.0.1/Screen%20Share/backend/api/devices/heartbeat.php', [
    'machine_identifier' => '11223344-5566-7788-99aa-bbccddeeff00',
    'system_id' => '834721906'
]);
echo "Heartbeat A: " . ($hbA['body']['status'] ?? 'err') . ", Heartbeat B: " . ($hbB['body']['status'] ?? 'err') . "\n";

// TEST 4: Query devices list
echo "\n4. Checking devices list API...\n";
$list = postJson('http://127.0.0.1/Screen%20Share/backend/api/devices/list.php', []);
if (isset($list['body']['devices'])) {
    foreach ($list['body']['devices'] as $d) {
        if ($d['system_id'] === $idA || $d['system_id'] === $idB) {
            echo sprintf("Device: %s | SystemID: %s | Online: %d\n", $d['name'], $d['system_id'], $d['is_online']);
        }
    }
}

// TEST 5: Create remote request from Laptop A to Laptop B
echo "\n5. Requesting remote connection: Laptop A ($idA) -> Laptop B ($idB)...\n";
$req = postJson('http://127.0.0.1/Screen%20Share/backend/api/connections/request.php', [
    'target_system_id' => $idB,
    'requester_system_id' => $idA,
    'requester_name' => 'SQUIRREL'
]);
echo "Request response: " . json_encode($req['body']) . "\n";
$token = $req['body']['request']['request_token'] ?? null;

// TEST 6: Check incoming requests on Laptop B
if ($token) {
    echo "\n6. Polling incoming request for Laptop B ($idB)...\n";
    $inc = postJson("http://127.0.0.1/Screen%20Share/backend/api/connections/incoming.php?system_id={$idB}", []);
    echo "Incoming request on B: " . json_encode($inc['body']) . "\n";

    // TEST 7: Accept connection on Laptop B
    echo "\n7. Accepting connection on Laptop B...\n";
    $acc = postJson('http://127.0.0.1/Screen%20Share/backend/api/connections/accept.php', [
        'request_token' => $token,
        'target_system_id' => $idB
    ]);
    echo "Accept response: " . json_encode($acc['body']) . "\n";

    // TEST 8: Check status from Laptop A
    echo "\n8. Checking status from Laptop A...\n";
    $stat = postJson("http://127.0.0.1/Screen%20Share/backend/api/connections/status.php?token={$token}", []);
    echo "Request status: " . ($stat['body']['request_status'] ?? 'unknown') . "\n";
}

echo "\n=== ALL PRESENCE & IDENTITY TESTS COMPLETED ===\n";
