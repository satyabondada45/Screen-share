<?php
require_once __DIR__ . '/api_path.php';
header("Location: " . deskstream_api_base_path() . '/auth/logout.php');
exit();
