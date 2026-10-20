<?php
// Anvil browser bridge (__VERSION__), required by prepend.php for a page load of an app
// whose "Browser errors" switch is on: one <script> goes before </head> of a text/html
// page. JSON, fragments, redirects, downloads and streams (SSE) are left as they are, and
// so is a page whose Content-Security-Policy wouldn't let the script run or report.
(static function (): void {
    $https = ($_SERVER['HTTPS'] ?? '') !== '' && ($_SERVER['HTTPS'] ?? '') !== 'off';
    $host = strtolower($_SERVER['HTTP_HOST'] ?? '');
    // True when a policy lets a same-origin script run and send to its origin. A nonce or
    // hash with 'strict-dynamic' ignores 'self'; Anvil never rewrites the app's policy.
    $allows = static function (string $policy) use ($https, $host): bool {
        $d = [];
        foreach (explode(';', $policy) as $part) {
            $words = preg_split('/\s+/', strtolower(trim($part)), -1, PREG_SPLIT_NO_EMPTY);
            if ($words && !isset($d[$words[0]])) {
                $d[array_shift($words)] = $words;
            }
        }
        $ok = static function (?array $src) use ($https, $host): bool {
            if ($src === null) {
                return true;
            }
            if (in_array("'strict-dynamic'", $src, true)) {
                return false;
            }
            foreach ($src as $s) {
                if ($s === "'self'" || $s === '*' || $s === ($https ? 'https:' : 'http:') || ($https && $s === 'http:')) {
                    return true;
                }
                // A host source naming the page's host (HTTP_HOST has a port only when
                // it isn't the default one, as a source must then too).
                $s = rtrim(preg_replace('#^https?://#', '', $s), '/');
                if ($s === $host || (str_starts_with($s, '*.') && str_ends_with($host, substr($s, 1)))) {
                    return true;
                }
            }
            return false;
        };
        return $ok($d['script-src-elem'] ?? $d['script-src'] ?? $d['default-src'] ?? null)
            && $ok($d['connect-src'] ?? $d['default-src'] ?? null);
    };
    ob_start(static function (string $buf, int $phase) use ($allows): string {
        static $done = false;
        if ($done) {
            return $buf;
        }
        $code = http_response_code();
        if ($code !== false && ($code < 200 || ($code >= 300 && $code < 400) || $code === 204)) {
            $done = true;
            return $buf;
        }
        foreach (headers_list() as $h) {
            [$name, $value] = array_map('trim', explode(':', $h, 2) + [1 => '']);
            $name = strtolower($name);
            if (($name === 'content-type' && stripos($value, 'text/html') !== 0)
                || $name === 'content-disposition') {
                $done = true;
                return $buf;
            }
            if ($name === 'content-security-policy' && !$allows($value)) {
                $done = true;
                if (!headers_sent()) {
                    header('X-Anvil-Bridge: blocked by Content-Security-Policy');
                }
                return $buf;
            }
        }
        $at = stripos($buf, '</head>');
        if ($at === false) {
            if ($phase & PHP_OUTPUT_HANDLER_FINAL) {
                $done = true;
            }
            return $buf;
        }
        $done = true;
        // A policy in the page itself.
        if (preg_match_all('/<meta[^>]+http-equiv\s*=\s*["\']?content-security-policy[^>]*>/i', substr($buf, 0, $at), $m)) {
            foreach ($m[0] as $tag) {
                if (preg_match('/content\s*=\s*("([^"]*)"|\'([^\']*)\')/i', $tag, $c) && !$allows(html_entity_decode($c[2] !== '' ? $c[2] : ($c[3] ?? '')))) {
                    if (!headers_sent()) {
                        header('X-Anvil-Bridge: blocked by Content-Security-Policy');
                    }
                    return $buf;
                }
            }
        }
        // The page grew: the web server works out its length again.
        if (!headers_sent()) {
            header_remove('Content-Length');
        }
        // Not deferred: it runs before the body's inline scripts, so their console
        // errors and fetches are seen too. Cached for good after the first page.
        return substr_replace($buf, '<script src="/__anvil/a.js?v=__VERSION__"></script>', $at, 0);
    }, 0);
})();
