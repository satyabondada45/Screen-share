<?php
try {
    $pdo = new PDO('mysql:host=127.0.0.1;dbname=u588459601_deskstream', 'root', '');
    
    echo "--- chat_conversations ---\n";
    $stmt1 = $pdo->query('SHOW COLUMNS FROM chat_conversations');
    if ($stmt1) {
        print_r($stmt1->fetchAll(PDO::FETCH_ASSOC));
    } else {
        echo "Table chat_conversations not found.\n";
    }
    
    echo "\n--- chat_messages ---\n";
    $stmt2 = $pdo->query('SHOW COLUMNS FROM chat_messages');
    if ($stmt2) {
        print_r($stmt2->fetchAll(PDO::FETCH_ASSOC));
    } else {
        echo "Table chat_messages not found.\n";
    }
} catch (PDOException $e) {
    echo 'Error: ' . $e->getMessage();
}
?>
