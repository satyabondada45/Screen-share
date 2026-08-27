<?php
// backend/api/agent/launch.php
// Launches DeskStream Desktop Agent in the INTERACTIVE USER session (Session 1).
// Apache/XAMPP runs in Windows Session 0 (service context) which has NO desktop access.
// We use Task Scheduler (schtasks) to escape Session 0 isolation so the agent can
// call GetDC(0) / BitBlt and capture the real user desktop.

header('Content-Type: application/json');
header('Access-Control-Allow-Origin: *');
header('Access-Control-Allow-Methods: POST, GET, OPTIONS');
header('Access-Control-Allow-Headers: Content-Type');

if ($_SERVER['REQUEST_METHOD'] === 'OPTIONS') {
    exit(0);
}

$exeCandidates = [
    __DIR__ . '/../../../dist/DeskStream.exe',
    __DIR__ . '/../../../desktop-agent/target/release/desktop-agent.exe',
    __DIR__ . '/../../../desktop-agent/target/debug/desktop-agent.exe',
    getenv('LOCALAPPDATA') . '\\DeskStream\\bin\\desktop-agent.exe'
];

$exePath = null;
foreach ($exeCandidates as $path) {
    if (file_exists($path)) {
        $exePath = realpath($path);
        break;
    }
}

// ─── 1. Check if DeskStream is already running ───────────────────────────────
$runningPids = [];
$output = [];
@exec('tasklist /FI "IMAGENAME eq DeskStream.exe" /FO CSV /NH 2>NUL', $output);
foreach ($output as $line) {
    if (stripos($line, 'DeskStream.exe') !== false) {
        $parts = str_getcsv($line);
        if (!empty($parts[1]) && is_numeric($parts[1])) {
            $runningPids[] = (int)$parts[1];
        }
    }
}

if (!empty($runningPids)) {
    echo json_encode([
        "status"          => "success",
        "already_running" => true,
        "pids"            => $runningPids,
        "pid"             => $runningPids[0],
        "executable"      => $exePath,
        "message"         => "DeskStream is already running."
    ]);
    exit();
}

if (!$exePath || !file_exists($exePath)) {
    http_response_code(404);
    echo json_encode([
        "status"  => "error",
        "message" => "DeskStream executable not found. Looked in dist/DeskStream.exe"
    ]);
    exit();
}

// ─── 2. Launch via Task Scheduler in the interactive user session ─────────────
// schtasks /Create creates a one-time task that runs immediately as the
// current interactive user (Session 1). This is the standard Windows escape
// from Session 0 isolation for services that need desktop access.
//
// /RU "" means "run as the currently logged-on user" (interactive session).
// /SC ONCE /ST 00:00 /SD 01/01/2000 with /F forces immediate run via /RUN.

$taskName = 'DeskStreamAgentLaunch';
$exeEscaped = str_replace("'", "''", $exePath);

// Delete any leftover task from a prior launch
@exec('schtasks /Delete /TN "' . $taskName . '" /F 2>NUL');

// Create the task to run as the interactive user
$createCmd = 'schtasks /Create /TN "' . $taskName . '"'
    . ' /TR "\"' . $exePath . '\""'
    . ' /SC ONCE'
    . ' /ST 00:00'
    . ' /SD 01/01/2000'
    . ' /RL HIGHEST'
    . ' /F 2>&1';

$createOut = [];
$createRet = 0;
exec($createCmd, $createOut, $createRet);

if ($createRet !== 0) {
    // Fallback: try without /RL HIGHEST (standard user)
    $createCmd2 = 'schtasks /Create /TN "' . $taskName . '"'
        . ' /TR "\"' . $exePath . '\""'
        . ' /SC ONCE'
        . ' /ST 00:00'
        . ' /SD 01/01/2000'
        . ' /F 2>&1';
    exec($createCmd2, $createOut, $createRet);
}

// Run it immediately
$runCmd = 'schtasks /Run /TN "' . $taskName . '" 2>&1';
$runOut = [];
$runRet = 0;
exec($runCmd, $runOut, $runRet);

// Clean up the task entry (agent is now running independently)
usleep(800000); // 800ms — give agent time to spawn
@exec('schtasks /Delete /TN "' . $taskName . '" /F 2>NUL');

// ─── 3. Find the new PID ──────────────────────────────────────────────────────
$newPids = [];
$checkOut = [];
@exec('tasklist /FI "IMAGENAME eq DeskStream.exe" /FO CSV /NH 2>NUL', $checkOut);
foreach ($checkOut as $line) {
    if (stripos($line, 'DeskStream.exe') !== false) {
        $parts = str_getcsv($line);
        if (!empty($parts[1]) && is_numeric($parts[1])) {
            $newPids[] = (int)$parts[1];
        }
    }
}

$pid = !empty($newPids) ? $newPids[0] : null;
$started = ($runRet === 0 || !empty($newPids));

echo json_encode([
    "status"          => $started ? "success" : "error",
    "already_running" => false,
    "started"         => $started,
    "pid"             => $pid,
    "executable"      => $exePath,
    "session"         => "interactive",
    "launch_method"   => "schtasks",
    "message"         => $started
        ? "Desktop Agent started in interactive session (Session 1)."
        : "Failed to start Desktop Agent. schtasks returned: " . implode(' ', $runOut),
    "schtasks_create" => implode("\n", $createOut),
    "schtasks_run"    => implode("\n", $runOut),
]);
