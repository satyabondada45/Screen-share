<?php
// backend/api/agent/launch.php
// Launches DeskStream Desktop Agent in the INTERACTIVE USER session.
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

$exePath = realpath(__DIR__ . '/../../../desktop-agent/target/release/desktop-agent.exe');
$processName = 'desktop-agent.exe';

if (!$exePath || !is_file($exePath) || !is_readable($exePath)) {
    http_response_code(404);
    echo json_encode([
        "status"  => "error",
        "message" => "Desktop Agent executable not found in the release build."
    ]);
    exit();
}

// ─── 1. Check if DeskStream is already running ───────────────────────────────
$runningPids = [];
$output = [];
@exec('tasklist /FI "IMAGENAME eq ' . $processName . '" /FO CSV /NH 2>NUL', $output);
foreach ($output as $line) {
    if (stripos($line, $processName) !== false) {
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

// ─── 2. Launch via Task Scheduler in the interactive user session ─────────────
// schtasks /Create creates a one-time task that runs immediately as the
// logged-on interactive user. This is the standard Windows escape
// from Session 0 isolation for services that need desktop access.

$taskName = 'DeskStreamAgentLaunch';
$taskStart = date('H:i', time() + 60);
$taskDate = date('m/d/Y');

// Find the user who owns an active interactive session. A service account
// must not be used because it would launch the agent in Session 0.
$sessionOut = [];
@exec('quser 2>NUL', $sessionOut);
$interactiveUser = null;
foreach ($sessionOut as $line) {
    if (preg_match('/^\s*>?(\S+)\s+\S+\s+\d+\s+Active\b/i', $line, $matches)) {
        $interactiveUser = $matches[1];
        break;
    }
}

if (!$interactiveUser) {
    http_response_code(409);
    echo json_encode([
        "status"  => "error",
        "message" => "No interactive Windows user is currently logged in; Desktop Agent was not started."
    ]);
    exit();
}

// Delete any leftover task from a prior launch
@exec('schtasks /Delete /TN "' . $taskName . '" /F 2>NUL');

// /RU identifies the logged-on user and /IT requires an interactive logon
// token, preventing the task from running in Session 0.
$createCmd = 'schtasks /Create /TN "' . $taskName . '"'
    . ' /TR "\"' . $exePath . '\""'
    . ' /SC ONCE'
    . ' /ST ' . $taskStart
    . ' /SD ' . $taskDate
    . ' /RU "' . $interactiveUser . '"'
    . ' /IT'
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
        . ' /ST ' . $taskStart
        . ' /SD ' . $taskDate
        . ' /RU "' . $interactiveUser . '"'
        . ' /IT'
        . ' /F 2>&1';
    exec($createCmd2, $createOut, $createRet);
}

if ($createRet !== 0) {
    @exec('schtasks /Delete /TN "' . $taskName . '" /F 2>NUL');
    http_response_code(500);
    echo json_encode([
        "status"          => "error",
        "message"         => "Failed to create the temporary interactive Desktop Agent task.",
        "schtasks_create" => implode("\n", $createOut),
    ]);
    exit();
}

// Run it immediately
$runCmd = 'schtasks /Run /TN "' . $taskName . '" 2>&1';
$runOut = [];
$runRet = 0;
exec($runCmd, $runOut, $runRet);

if ($runRet !== 0) {
    @exec('schtasks /Delete /TN "' . $taskName . '" /F 2>NUL');
    http_response_code(500);
    echo json_encode([
        "status"          => "error",
        "message"         => "Failed to run the temporary interactive Desktop Agent task.",
        "schtasks_run"    => implode("\n", $runOut),
        "schtasks_create" => implode("\n", $createOut),
    ]);
    exit();
}

// Give the agent time to spawn before removing the temporary task entry.
usleep(800000);
@exec('schtasks /Delete /TN "' . $taskName . '" /F 2>NUL');

// ─── 3. Find the new PID ──────────────────────────────────────────────────────
$newPids = [];
for ($attempt = 0; $attempt < 5; $attempt++) {
    $checkOut = [];
    @exec('tasklist /FI "IMAGENAME eq ' . $processName . '" /FO CSV /NH 2>NUL', $checkOut);
    foreach ($checkOut as $line) {
        if (stripos($line, $processName) !== false) {
            $parts = str_getcsv($line);
            if (!empty($parts[1]) && is_numeric($parts[1])) {
                $newPids[] = (int)$parts[1];
            }
        }
    }
    if (!empty($newPids)) {
        break;
    }
    usleep(400000);
}

$pid = !empty($newPids) ? $newPids[0] : null;
$started = !empty($newPids);

echo json_encode([
    "status"          => $started ? "success" : "error",
    "already_running" => false,
    "started"         => $started,
    "pid"             => $pid,
    "executable"      => $exePath,
    "session"         => "interactive",
    "launch_method"   => "schtasks",
    "message"         => $started
        ? "Desktop Agent started in the interactive user session."
        : "Failed to start Desktop Agent. schtasks returned: " . implode(' ', $runOut),
    "schtasks_create" => implode("\n", $createOut),
    "schtasks_run"    => implode("\n", $runOut),
]);
