# ScreenShare-Tray.ps1
# System tray launcher for Screen Share
# Launches desktop-agent.exe and relay-server.exe, shows tray icon with menu

# Self-elevate if needed (for starting/stopping services)
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

$ErrorActionPreference = "SilentlyContinue"

# Determine deploy directory
$deployDir = "$env:LOCALAPPDATA\DeskStream\bin"
$agentExe = "$deployDir\desktop-agent.exe"
$relayExe = "$deployDir\relay-server.exe"

# --- Helper: Check if a process is running ---
function Test-ProcessRunning($name) {
    return (Get-Process -Name $name -ErrorAction SilentlyContinue) -ne $null
}

# --- Helper: Start a process hidden ---
function Start-HiddenProcess($filePath) {
    if (Test-Path $filePath) {
        Start-Process -FilePath $filePath -WindowStyle Hidden
    }
}

# --- Helper: Stop a process ---
function Stop-AgentProcess($name) {
    Get-Process -Name $name -ErrorAction SilentlyContinue | Stop-Process -Force
}

# --- Create the tray icon ---
Add-Type -AssemblyName System.Windows.Forms

$tray = New-Object System.Windows.Forms.NotifyIcon
$tray.Icon = [System.Drawing.SystemIcons]::Application
$tray.Visible = $true
$tray.Text = "Screen Share - Running"

# --- Context menu ---
$contextMenu = New-Object System.Windows.Forms.ContextMenuStrip

$sendFileItem = $contextMenu.Items.Add("Send File to Viewer")
$dashItem = $contextMenu.Items.Add("Open Dashboard")
$toggleItem = $contextMenu.Items.Add("")
$exitItem = $contextMenu.Items.Add("Exit")

$toggleText = "Start Host Agent"
if (Test-ProcessRunning "desktop-agent") {
    $toggleText = "Stop Host Agent"
    $tray.Text = "Screen Share - Connected"
}
$toggleItem.Text = $toggleText

# --- Start processes if not running ---
if (-not (Test-ProcessRunning "relay-server")) {
    Start-HiddenProcess $relayExe
    Start-Sleep -Milliseconds 500
}
if (-not (Test-ProcessRunning "desktop-agent")) {
    Start-HiddenProcess $agentExe
    Start-Sleep -Seconds 2
}

# --- Event handlers ---
$sendFileItem.Add_Click({
    Add-Type -AssemblyName System.Windows.Forms
    $openFileDialog = New-Object System.Windows.Forms.OpenFileDialog
    $openFileDialog.Title = "Select File to Send to Viewer"
    $openFileDialog.Filter = "All Files (*.*)|*.*"
    if ($openFileDialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
        $filePath = $openFileDialog.FileName
        $targetDir = "$env:LOCALAPPDATA\DeskStream"
        if (-not (Test-Path $targetDir)) { New-Item -ItemType Directory -Path $targetDir | Out-Null }
        $filePath | Out-File -FilePath "$targetDir\send_file.txt" -Encoding utf8
    }
})

$dashItem.Add_Click({
    Start-Process "http://localhost:8080/dashboard.php"
})

$toggleItem.Add_Click({
    if (Test-ProcessRunning "desktop-agent") {
        Stop-AgentProcess "desktop-agent"
        $toggleItem.Text = "Start Host Agent"
        $tray.Text = "Screen Share - Stopped"
    } else {
        Start-HiddenProcess $agentExe
        Start-Sleep -Seconds 2
        $toggleItem.Text = "Stop Host Agent"
        $tray.Text = "Screen Share - Connected"
    }
})

$exitItem.Add_Click({
    if (Test-ProcessRunning "desktop-agent") {
        Stop-AgentProcess "desktop-agent"
    }
    if (Test-ProcessRunning "relay-server") {
        Stop-AgentProcess "relay-server"
    }
    $tray.Visible = $false
    [System.Windows.Forms.Application]::Exit()
    exit
})

$tray.ContextMenuStrip = $contextMenu

# Double-click opens dashboard
$tray.Add_DoubleClick({
    Start-Process "http://localhost:8080/dashboard.php"
})

# --- Periodic health check ---
$timer = New-Object System.Windows.Forms.Timer
$timer.Interval = 5000

$timer.Add_Tick({
    if (Test-ProcessRunning "desktop-agent") {
        $tray.Text = "Screen Share - Connected"
    } else {
        $tray.Text = "Screen Share - Disconnected"
        $toggleItem.Text = "Start Host Agent"
    }
})

$timer.Start()

# --- Keep the tray application alive ---
[System.Windows.Forms.Application]::Run()
