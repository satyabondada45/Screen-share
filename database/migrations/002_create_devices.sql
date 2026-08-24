CREATE TABLE IF NOT EXISTS devices (
    id INT AUTO_INCREMENT PRIMARY KEY,
    user_id INT NULL,
    device_uid VARCHAR(32) NOT NULL UNIQUE,
    name VARCHAR(255) NOT NULL,
    os_type VARCHAR(50) DEFAULT 'windows',
    ip_address VARCHAR(45) NULL,
    is_online TINYINT(1) DEFAULT 0,
    last_seen_at DATETIME NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_device_uid (device_uid),
    INDEX idx_is_online (is_online)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS sessions (
    id INT AUTO_INCREMENT PRIMARY KEY,
    session_key VARCHAR(64) NOT NULL,
    host_device_id INT NOT NULL,
    client_ip VARCHAR(45) NULL,
    started_at DATETIME NOT NULL,
    ended_at DATETIME NULL,
    status ENUM('active', 'completed', 'rejected') DEFAULT 'active',
    bandwidth_used_mb DECIMAL(10, 2) DEFAULT 0.00,
    FOREIGN KEY (host_device_id) REFERENCES devices(id) ON DELETE CASCADE,
    INDEX idx_session_status (status)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;