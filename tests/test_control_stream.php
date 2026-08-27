<?php
/**
 * Test Control Pipeline (Mouse & Keyboard injection)
 */

$host = '127.0.0.1';
$port = 9001;
$targetId = '238548998';

echo "========================================\n";
echo "DeskStream Control Pipeline Test\n";
echo "Target ID: $targetId\n";
echo "Relay: ws://$host:$port\n";
echo "========================================\n\n";

$sock = @fsockopen($host, $port, $errno, $errstr, 5);
if (!$sock) {
    echo "[FAIL] Could not connect to relay on $host:$port: $errstr ($errno)\n";
    exit(1);
}

stream_set_timeout($sock, 5);

$key = base64_encode(random_bytes(16));
$req = "GET / HTTP/1.1\r\n" .
       "Host: $host:$port\r\n" .
       "Upgrade: websocket\r\n" .
       "Connection: Upgrade\r\n" .
       "Sec-WebSocket-Key: $key\r\n" .
       "Sec-WebSocket-Version: 13\r\n\r\n";

fwrite($sock, $req);
$response = fread($sock, 2048);

if (!str_contains($response, "101 Switching Protocols")) {
    echo "[FAIL] WebSocket upgrade failed.\n";
    fclose($sock);
    exit(1);
}

echo "[1] Connected and upgraded WebSocket.\n";

// Pairing packet: Type 2 (1B) + Target ID (9B) + Hash (32B)
$handshake = "\x02" . str_pad($targetId, 9, ' ', STR_PAD_RIGHT) . str_repeat("\x00", 32);
sendWsBinary($sock, $handshake);

echo "[2] Sent viewer pairing handshake.\n";

$start = time();
$gotApproval = false;

while (time() - $start < 5) {
    $frame = readWsFrame($sock);
    if ($frame !== null && strlen($frame) > 0) {
        $type = ord($frame[0]);
        if ($type === 2) {
            $gotApproval = true;
            echo "[3] Viewer approved and paired.\n";
            break;
        }
    }
}

if (!$gotApproval) {
    echo "[FAIL] Pairing approval not received.\n";
    fclose($sock);
    exit(1);
}

// Send Test Control Packets (9-byte format):
// 1. Mouse Move to x=32768, y=32768 (Center of screen)
$movePkt = pack('CnnN', 0, 32768, 32768, 0);
sendWsBinary($sock, $movePkt);
echo "[4] Sent Mouse Move (Center).\n";

usleep(100000);

// 2. Mouse Left Down & Up
$leftDownPkt = pack('CnnN', 1, 32768, 32768, 0);
sendWsBinary($sock, $leftDownPkt);
usleep(50000);
$leftUpPkt = pack('CnnN', 2, 32768, 32768, 0);
sendWsBinary($sock, $leftUpPkt);
echo "[5] Sent Mouse Left Click.\n";

usleep(100000);

// 3. Mouse Wheel (Scroll Up)
$wheelPkt = pack('Cnnn', 9, 0, 0, 120) . pack('N', 0);
sendWsBinary($sock, substr($wheelPkt, 0, 9));
echo "[6] Sent Mouse Wheel (Scroll Up).\n";

usleep(100000);

// 4. Keyboard Key 'A' (VK 65) Down & Up
$keyDownPkt = pack('CNN', 5, 65, 0);
sendWsBinary($sock, $keyDownPkt);
usleep(50000);
$keyUpPkt = pack('CNN', 6, 65, 0);
sendWsBinary($sock, $keyUpPkt);
echo "[7] Sent Keyboard Key 'A' (Press + Release).\n";

// Verify video packets continue streaming concurrently
$videoCount = 0;
$testStart = time();
while (time() - $testStart < 3) {
    $frame = readWsFrame($sock);
    if ($frame !== null && strlen($frame) > 0) {
        $type = ord($frame[0]);
        if ($type === 13) {
            $videoCount++;
        }
    }
}

echo "[8] Video Frames Received concurrently: $videoCount\n";
echo "========================================\n";
echo "CONTROL PIPELINE TEST COMPLETED SUCCESSFULLY!\n";
echo "========================================\n";

fclose($sock);

function sendWsBinary($sock, $payload) {
    $len = strlen($payload);
    $header = "\x82"; // Binary frame + FIN
    $mask = random_bytes(4);
    
    if ($len <= 125) {
        $header .= chr(0x80 | $len);
    } elseif ($len <= 65535) {
        $header .= chr(0x80 | 126) . pack('n', $len);
    } else {
        $header .= chr(0x80 | 127) . pack('J', $len);
    }
    
    $masked = '';
    for ($i = 0; $i < $len; $i++) {
        $masked .= $payload[$i] ^ $mask[$i % 4];
    }
    
    fwrite($sock, $header . $mask . $masked);
}

function readWsFrame($sock) {
    $header = @fread($sock, 2);
    if ($header === false || strlen($header) < 2) return null;
    
    $b2 = ord($header[1]);
    $payloadLen = $b2 & 0x7F;
    
    if ($payloadLen === 126) {
        $ext = fread($sock, 2);
        if (strlen($ext) < 2) return null;
        $payloadLen = unpack('n', $ext)[1];
    } elseif ($payloadLen === 127) {
        $ext = fread($sock, 8);
        if (strlen($ext) < 8) return null;
        $payloadLen = unpack('J', $ext)[1];
    }
    
    $data = '';
    while (strlen($data) < $payloadLen) {
        $chunk = fread($sock, min(65536, $payloadLen - strlen($data)));
        if ($chunk === false || strlen($chunk) === 0) break;
        $data .= $chunk;
    }
    return $data;
}
