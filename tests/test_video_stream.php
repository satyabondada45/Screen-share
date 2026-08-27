<?php
/**
 * End-to-End WebSocket Stream Test for DeskStream Video & Audio
 */

$targetId = "238548998";
$relayHost = "127.0.0.1";
$relayPort = 9001;

echo "========================================\n";
echo "DeskStream Video Delivery Diagnostic Test\n";
echo "Target ID: {$targetId}\n";
echo "Relay: ws://{$relayHost}:{$relayPort}\n";
echo "========================================\n\n";

$sock = @fsockopen($relayHost, $relayPort, $errno, $errstr, 5);
if (!$sock) {
    die("FAILED to connect to relay server on {$relayHost}:{$relayPort}: $errstr ($errno)\n");
}

echo "[1] Connected TCP socket to Relay on port {$relayPort}\n";

// Perform WebSocket Handshake
$secKey = base64_encode(random_bytes(16));
$headers = "GET / HTTP/1.1\r\n" .
           "Host: {$relayHost}:{$relayPort}\r\n" .
           "Upgrade: websocket\r\n" .
           "Connection: Upgrade\r\n" .
           "Sec-WebSocket-Key: {$secKey}\r\n" .
           "Sec-WebSocket-Version: 13\r\n\r\n";

fwrite($sock, $headers);

$response = "";
while (!feof($sock)) {
    $line = fgets($sock);
    $response .= $line;
    if (rtrim($line) === "") break;
}

if (!str_contains($response, "101 Switching Protocols")) {
    die("WebSocket Handshake Failed:\n" . $response);
}

echo "[2] WebSocket Upgrade Handshake -> 101 Switching Protocols (OK)\n";

// Function to send masked binary WebSocket frame
function sendWsBinary($sock, $data) {
    $len = strlen($data);
    $frame = chr(0x82); // Final frame, Binary opcode
    $mask = random_bytes(4);
    
    if ($len <= 125) {
        $frame .= chr(0x80 | $len);
    } elseif ($len <= 65535) {
        $frame .= chr(0x80 | 126) . pack("n", $len);
    } else {
        $frame .= chr(0x80 | 127) . pack("J", $len);
    }
    
    $frame .= $mask;
    $maskedData = "";
    for ($i = 0; $i < $len; $i++) {
        $maskedData .= chr(ord($data[$i]) ^ ord($mask[$i % 4]));
    }
    $frame .= $maskedData;
    fwrite($sock, $frame);
}

// Function to read unmasked / masked WebSocket frame payload
function readWsFrame($sock) {
    $hdr = fread($sock, 2);
    if (strlen($hdr) < 2) return null;
    $b1 = ord($hdr[0]);
    $b2 = ord($hdr[1]);
    $isMasked = ($b2 & 0x80) !== 0;
    $len = $b2 & 0x7F;
    if ($len === 126) {
        $ext = fread($sock, 2);
        if (strlen($ext) < 2) return null;
        $len = unpack("n", $ext)[1];
    } elseif ($len === 127) {
        $ext = fread($sock, 8);
        if (strlen($ext) < 8) return null;
        $len = unpack("J", $ext)[1];
    }
    $maskKey = "";
    if ($isMasked) {
        $maskKey = fread($sock, 4);
    }
    $payload = "";
    $remaining = $len;
    while ($remaining > 0) {
        $chunk = fread($sock, min(65536, $remaining));
        if ($chunk === false || strlen($chunk) === 0) break;
        $payload .= $chunk;
        $remaining -= strlen($chunk);
    }
    if ($isMasked && strlen($maskKey) === 4) {
        for ($i = 0; $i < strlen($payload); $i++) {
            $payload[$i] = chr(ord($payload[$i]) ^ ord($maskKey[$i % 4]));
        }
    }
    return $payload;
}

// Send Viewer Handshake (Type 2, System ID padded/fixed, 32 bytes auth hash)
$authHash = hash('sha256', 'DESKSTREAM_AUTH_' . $targetId, true);
$paddedId = str_pad($targetId, 32, "\0");
$handshakePayload = chr(2) . $paddedId . $authHash;

echo "[3] Sending Viewer Pairing Handshake for Target ID: {$targetId}...\n";
sendWsBinary($sock, $handshakePayload);

// Receive frames
$startTime = time();
$videoFrameCount = 0;
$audioFrameCount = 0;
$receivedKeyframe = false;

stream_set_timeout($sock, 5);

while (time() - $startTime < 6) {
    $payload = readWsFrame($sock);
    if ($payload === null || strlen($payload) === 0) {
        continue;
    }
    
    $type = ord($payload[0]);
    $bytes = strlen($payload);
    
    if ($type === 2) {
        echo "[4] Received Stream-Active Approval Packet (Type 2)\n";
    } elseif ($type === 13 || $type === 15) {
        $videoFrameCount++;
        $width = unpack("N", substr($payload, 1, 4))[1];
        $height = unpack("N", substr($payload, 5, 4))[1];
        $h264Size = unpack("N", substr($payload, 9, 4))[1];
        
        $h264Data = substr($payload, 13);
        
        // Inspect NALs
        $nals = [];
        $hasSps = false;
        $hasPps = false;
        $hasIdr = false;
        $len = strlen($h264Data);
        for ($i = 0; $i + 3 < $len; $i++) {
            if (($h264Data[$i] === "\0" && $h264Data[$i+1] === "\0" && $h264Data[$i+2] === "\x01") ||
                ($i + 4 <= $len && $h264Data[$i] === "\0" && $h264Data[$i+1] === "\0" && $h264Data[$i+2] === "\0" && $h264Data[$i+3] === "\x01")) {
                $sc = ($h264Data[$i+2] === "\x01") ? 3 : 4;
                if ($i + $sc < $len) {
                    $nType = ord($h264Data[$i + $sc]) & 0x1F;
                    $nals[] = $nType;
                    if ($nType === 7) $hasSps = true;
                    if ($nType === 8) $hasPps = true;
                    if ($nType === 5) $hasIdr = true;
                }
                $i += $sc - 1;
            }
        }
        
        if ($hasIdr) $receivedKeyframe = true;
        
        if ($videoFrameCount <= 3 || $videoFrameCount % 30 === 0) {
            echo "[VIDEO] Frame #{$videoFrameCount}: {$width}x{$height}, H264: {$h264Size} bytes, Total: {$bytes} bytes | NALs: [" . implode(",", $nals) . "] | SPS:" . ($hasSps ? "YES" : "NO") . " PPS:" . ($hasPps ? "YES" : "NO") . " IDR:" . ($hasIdr ? "YES" : "NO") . "\n";
        }
    } elseif ($type === 17) {
        $audioFrameCount++;
        if ($audioFrameCount === 1 || $audioFrameCount % 50 === 0) {
            echo "[AUDIO] Packet #{$audioFrameCount}: Total {$bytes} bytes\n";
        }
    } elseif ($type === 14) {
        echo "[HEARTBEAT] Ping received\n";
    }
}

fclose($sock);

echo "\n========================================\n";
echo "SUMMARY RESULTS:\n";
echo "Total Video Frames Received: {$videoFrameCount}\n";
echo "Total Audio Packets Received: {$audioFrameCount}\n";
echo "Keyframe (SPS/PPS/IDR) Received: " . ($receivedKeyframe ? "YES (SUCCESS)" : "NO (FAIL)") . "\n";
echo "========================================\n";

if ($videoFrameCount > 0 && $receivedKeyframe) {
    echo "TEST PASSED: Video pipeline delivers continuous H.264 video with valid keyframes!\n";
    exit(0);
} else {
    echo "TEST FAILED: No video frames received.\n";
    exit(1);
}
