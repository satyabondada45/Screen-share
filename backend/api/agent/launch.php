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

$processNames = ['DeskStream.exe', 'desktop-agent.exe'];

function find_deskstream_processes(array $processNames): array
{
    $processes = [];
    foreach ($processNames as $processName) {
        $output = [];
        $exitCode = 0;
        exec('tasklist /FI "IMAGENAME eq ' . $processName . '" /FO CSV /NH 2>NUL', $output, $exitCode);
        if ($exitCode !== 0) {
            error_log('[DESKSTREAM LAUNCH] tasklist failed for ' . $processName . ': exit=' . $exitCode);
            continue;
        }
        foreach ($output as $line) {
            if (stripos($line, $processName) === false) {
                continue;
            }
            $parts = str_getcsv($line);
            if (!empty($parts[1]) && is_numeric($parts[1])) {
                $processes[] = ['pid' => (int)$parts[1], 'image' => $processName];
            }
        }
    }
    return $processes;
}

// ─── 1. Check if DeskStream is already running ───────────────────────────────
$runningProcesses = find_deskstream_processes($processNames);
error_log('[DESKSTREAM LAUNCH] Existing process check: ' . json_encode($runningProcesses));

if (!empty($runningProcesses)) {
    echo json_encode([
        "status"          => "success",
        "already_running" => true,
        "pids"            => array_column($runningProcesses, 'pid'),
        "pid"             => $runningProcesses[0]['pid'],
        "process"         => $runningProcesses[0]['image'],
        "executable"      => null,
        "message"         => "DeskStream is already running."
    ]);
    exit();
}

// The release NSIS installer places DeskStream.exe at %LOCALAPPDATA%\DeskStream.
// Keep the legacy payload paths for existing installations, but prefer the current package.
$installCandidates = [];
$localAppData = getenv('LOCALAPPDATA');
if ($localAppData) {
    $installCandidates[] = $localAppData . DIRECTORY_SEPARATOR . 'DeskStream'
        . DIRECTORY_SEPARATOR . 'DeskStream.exe';
    $installCandidates[] = $localAppData . DIRECTORY_SEPARATOR . 'DeskStream'
        . DIRECTORY_SEPARATOR . 'bin' . DIRECTORY_SEPARATOR . 'desktop-agent.exe';
}
$userProfile = getenv('USERPROFILE');
if ($userProfile) {
    $installCandidates[] = $userProfile . DIRECTORY_SEPARATOR . 'AppData'
        . DIRECTORY_SEPARATOR . 'Local' . DIRECTORY_SEPARATOR . 'DeskStream'
        . DIRECTORY_SEPARATOR . 'DeskStream.exe';
    $installCandidates[] = $userProfile . DIRECTORY_SEPARATOR . 'AppData'
        . DIRECTORY_SEPARATOR . 'Local' . DIRECTORY_SEPARATOR . 'DeskStream'
        . DIRECTORY_SEPARATOR . 'bin' . DIRECTORY_SEPARATOR . 'desktop-agent.exe';
}
$programFiles = getenv('ProgramFiles');
if ($programFiles) {
    $installCandidates[] = $programFiles . DIRECTORY_SEPARATOR . 'Screen Share'
        . DIRECTORY_SEPARATOR . 'desktop-agent.exe';
}
$installCandidates[] = 'C:\\DeskStream\\bin\\desktop-agent.exe';

$exePath = false;
foreach ($installCandidates as $candidate) {
    $resolved = realpath($candidate);
    if ($resolved && is_file($resolved) && is_readable($resolved)) {
        $exePath = $resolved;
        break;
    }
}
error_log('[DESKSTREAM LAUNCH] Resolved executable: ' . ($exePath ?: 'not found'));

if (!$exePath) {
    http_response_code(404);
    echo json_encode([
        "status"  => "error",
        "message" => "Installed DeskStream executable was not found. Reinstall DeskStream or start it from the installed shortcut."
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
    error_log('[DESKSTREAM LAUNCH] No active interactive session; launch not attempted.');
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
error_log('[DESKSTREAM LAUNCH] schtasks create exit=' . $createRet . '; output=' . implode(' | ', $createOut));

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
    error_log('[DESKSTREAM LAUNCH] schtasks fallback create exit=' . $createRet . '; output=' . implode(' | ', $createOut));
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
error_log('[DESKSTREAM LAUNCH] schtasks run exit=' . $runRet . '; output=' . implode(' | ', $runOut));

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

// ─── 3. Find the new process using both current and legacy image names ────────
$newProcesses = [];
for ($attempt = 0; $attempt < 5; $attempt++) {
    $newProcesses = find_deskstream_processes($processNames);
    error_log('[DESKSTREAM LAUNCH] Confirmation attempt=' . ($attempt + 1) . '; processes=' . json_encode($newProcesses));
    if (!empty($newProcesses)) {
        break;
    }
    usleep(400000);
}

$pid = !empty($newProcesses) ? $newProcesses[0]['pid'] : null;
$process = !empty($newProcesses) ? $newProcesses[0]['image'] : null;
$started = !empty($newProcesses);
error_log('[DESKSTREAM LAUNCH] Result=' . ($started ? 'running' : 'not-running')
    . '; pid=' . ($pid ?? 'none') . '; image=' . ($process ?? 'none') . '; path=' . $exePath);

echo json_encode([
    "status"          => $started ? "success" : "error",
    "already_running" => false,
    "started"         => $started,
    "pid"             => $pid,
    "process"         => $process,
    "executable"      => $exePath,
    "session"         => "interactive",
    "launch_method"   => "schtasks",
    "message"         => $started
        ? "Desktop Agent started in the interactive user session."
        : "Failed to start Desktop Agent. schtasks returned: " . implode(' ', $runOut),
    "schtasks_create" => implode("\n", $createOut),
    "schtasks_run"    => implode("\n", $runOut),
]);
