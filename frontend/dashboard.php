<?php
session_start();
if (!isset($_SESSION['user_id'])) {
    header("Location: login.php");
    exit();
}

require_once '../backend/config/database.php';

// Fetch devices for the logged-in user
$stmt = $conn->prepare("SELECT * FROM devices WHERE user_id = ?");
$stmt->execute([$_SESSION['user_id']]);
$devices = $stmt->fetchAll(PDO::FETCH_ASSOC);
?>
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Dashboard</title>
    <style>
        body { font-family: Arial, sans-serif; background-color: #f4f4f9; padding: 20px; }
        .device-card { background: white; padding: 15px; margin-bottom: 10px; border-radius: 5px; box-shadow: 0 2px 5px rgba(0,0,0,0.1); }
    </style>
</head>
<body>
    <h1>Welcome back, <?php echo htmlspecialchars($_SESSION['username']); ?>!</h1>
    
    <h2>Your Devices</h2>
    <?php if (count($devices) > 0): ?>
        <?php foreach ($devices as $device): ?>
            <div class="device-card">
                <strong><?php echo htmlspecialchars($device['device_name']); ?></strong> (ID: <?php echo htmlspecialchars($device['device_id']); ?>)<br>
                Status: <?php echo htmlspecialchars($device['status']); ?> | OS: <?php echo htmlspecialchars($device['operating_system']); ?>
            </div>
        <?php endforeach; ?>
    <?php else: ?>
        <p>No devices registered yet. Run your Rust agent!</p>
    <?php endif; ?>
    
    <br><a href="logout.php">Logout</a>
</body>
</html> 