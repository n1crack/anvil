<?php
// Anvil browser bridge, written by Anvil; replaced when Anvil starts. PHP runs it before
// every request of the apps on this PHP (auto_prepend_file, while any app's "Browser
// errors" switch is on). Kept small: only a page load (Sec-Fetch-Dest where the browser
// sends it, else what it accepts first) of an app with the switch on (ANVIL_INJECT, from
// the web server) loads inject.php.
if (!empty($_SERVER['ANVIL_INJECT'])
    && (isset($_SERVER['HTTP_SEC_FETCH_DEST'])
        ? in_array($_SERVER['HTTP_SEC_FETCH_DEST'], ['document', 'iframe'], true)
        : stripos($_SERVER['HTTP_ACCEPT'] ?? '', 'text/html') === 0)) {
    require __DIR__ . '/inject.php';
}
// The app's own auto_prepend_file (php.ini in Anvil's Settings), which this file took the place of.
if (getenv('ANVIL_PREPEND')) {
    require getenv('ANVIL_PREPEND');
}
