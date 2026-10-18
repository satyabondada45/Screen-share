<?php
$c = file_get_contents(__DIR__ . '/../DESKSTREAM/DeskStream.exe');
$pos = strpos($c, 'stable.json');
if ($pos !== false) {
    echo "Around stable.json:\n";
    echo substr($c, $pos - 60, 160) . "\n\n";
}

$posFallback = strpos($c, 'check.php');
if ($posFallback !== false) {
    echo "Around check.php:\n";
    echo substr($c, $posFallback - 60, 160) . "\n\n";
} else {
    echo "check.php NOT found in binary!\n";
}
