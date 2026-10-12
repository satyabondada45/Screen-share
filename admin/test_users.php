<?php
$_GET['view'] = '9';
$_SERVER['REQUEST_METHOD'] = 'GET';
session_start();
$_SESSION['admin_user_id'] = 1;
require 'users.php';
