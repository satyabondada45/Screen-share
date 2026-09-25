<?php
header('Access-Control-Allow-Origin: *');
header('Access-Control-Allow-Methods: GET, POST, OPTIONS');
header('Access-Control-Allow-Headers: *');
file_put_contents('headers.log', json_encode(getallheaders(), JSON_PRETTY_PRINT));
echo 'ok';
