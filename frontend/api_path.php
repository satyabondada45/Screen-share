<?php

function deskstream_application_base_path(): string
{
    $scriptPath = parse_url((string)($_SERVER['SCRIPT_NAME'] ?? ''), PHP_URL_PATH);
    $scriptPath = str_replace('\\', '/', is_string($scriptPath) ? $scriptPath : '');

    foreach (['/backend/api/', '/api/', '/frontend/', '/remote/', '/admin/'] as $marker) {
        $position = strpos($scriptPath, $marker);
        if ($position !== false) {
            return rtrim(substr($scriptPath, 0, $position), '/');
        }
    }

    return rtrim(str_replace('\\', '/', dirname($scriptPath)), '/');
}

function deskstream_application_root(): string
{
    $documentRoot = (string)($_SERVER['DOCUMENT_ROOT'] ?? '');
    if ($documentRoot === '') {
        throw new RuntimeException('The DeskStream document root is unavailable.');
    }

    $basePath = rawurldecode(deskstream_application_base_path());
    $root = rtrim($documentRoot, "\\/") . DIRECTORY_SEPARATOR
        . str_replace('/', DIRECTORY_SEPARATOR, ltrim($basePath, '/'));
    $resolvedRoot = realpath($root);
    if ($resolvedRoot === false || !is_dir($resolvedRoot)) {
        throw new RuntimeException('The DeskStream application root could not be resolved.');
    }

    return $resolvedRoot;
}

function deskstream_api_base_path(): string
{
    $root = deskstream_application_root();
    $basePath = deskstream_application_base_path();

    if (is_file($root . DIRECTORY_SEPARATOR . 'api' . DIRECTORY_SEPARATOR . 'auth' . DIRECTORY_SEPARATOR . 'login.php')) {
        return $basePath . '/api';
    }
    if (is_file($root . DIRECTORY_SEPARATOR . 'backend' . DIRECTORY_SEPARATOR . 'api' . DIRECTORY_SEPARATOR . 'auth' . DIRECTORY_SEPARATOR . 'login.php')) {
        return $basePath . '/backend/api';
    }

    throw new RuntimeException('The DeskStream API directory could not be found.');
}

function deskstream_config_file(string $filename): string
{
    if ($filename === '' || basename($filename) !== $filename) {
        throw new InvalidArgumentException('A configuration filename is required.');
    }

    $root = deskstream_application_root();
    foreach (['config', 'backend' . DIRECTORY_SEPARATOR . 'config'] as $directory) {
        $path = $root . DIRECTORY_SEPARATOR . $directory . DIRECTORY_SEPARATOR . $filename;
        if (is_file($path)) {
            return $path;
        }
    }

    throw new RuntimeException('The requested DeskStream configuration file could not be found.');
}

function deskstream_service_file(string $filename): string
{
    if ($filename === '' || basename($filename) !== $filename) {
        throw new InvalidArgumentException('A service filename is required.');
    }

    $root = deskstream_application_root();
    foreach (['services', 'backend' . DIRECTORY_SEPARATOR . 'services'] as $directory) {
        $path = $root . DIRECTORY_SEPARATOR . $directory . DIRECTORY_SEPARATOR . $filename;
        if (is_file($path)) {
            return $path;
        }
    }

    throw new RuntimeException('The requested DeskStream service could not be found.');
}

function deskstream_frontend_url(string $filename): string
{
    if ($filename === '' || basename($filename) !== $filename) {
        throw new InvalidArgumentException('A frontend filename is required.');
    }

    $root = deskstream_application_root();
    $basePath = deskstream_application_base_path();
    $frontendFile = $root . DIRECTORY_SEPARATOR . 'frontend' . DIRECTORY_SEPARATOR . $filename;

    if (is_file($frontendFile)) {
        return $basePath . '/frontend/' . rawurlencode($filename);
    }

    return $basePath . '/' . rawurlencode($filename);
}

function deskstream_login_url(): string
{
    $root = deskstream_application_root();
    $basePath = deskstream_application_base_path();
    $frontendLogin = $root . DIRECTORY_SEPARATOR . 'frontend' . DIRECTORY_SEPARATOR . 'index.php';

    if (is_file($frontendLogin)) {
        return $basePath . '/frontend/index.php';
    }

    return $basePath . '/';
}
