CREATE DATABASE IF NOT EXISTS screen_share_db;

USE screen_share_db;


-- ==========================================
-- USERS
-- ==========================================

CREATE TABLE IF NOT EXISTS users (
    id INT PRIMARY KEY AUTO_INCREMENT,
    username VARCHAR(255) NOT NULL,
    email VARCHAR(255) NOT NULL UNIQUE,
    password_hash VARCHAR(255) NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);


-- ==========================================
-- DEVICES
-- ==========================================

CREATE TABLE IF NOT EXISTS devices (
    id INT PRIMARY KEY AUTO_INCREMENT,
    user_id INT NOT NULL,
    device_id VARCHAR(255) NOT NULL UNIQUE,
    device_name VARCHAR(255) NOT NULL,
    device_type VARCHAR(255) NOT NULL,
    operating_system VARCHAR(255) NOT NULL,
    status VARCHAR(255) DEFAULT 'inactive',
    last_seen DATETIME NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,

    FOREIGN KEY (user_id)
        REFERENCES users(id)
        ON DELETE CASCADE
);


-- ==========================================
-- REMOTE SESSIONS
-- ==========================================

CREATE TABLE IF NOT EXISTS sessions (
    id INT PRIMARY KEY AUTO_INCREMENT,
    session_id VARCHAR(255) NOT NULL UNIQUE,
    streaming_device_id INT NOT NULL,
    viewing_device_id INT NOT NULL,
    status VARCHAR(255) DEFAULT 'inactive',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,

    FOREIGN KEY (streaming_device_id)
        REFERENCES devices(id)
        ON DELETE CASCADE,

    FOREIGN KEY (viewing_device_id)
        REFERENCES devices(id)
        ON DELETE CASCADE
);